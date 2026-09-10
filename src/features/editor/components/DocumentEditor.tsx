import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";

import {
  readDocument,
  type DocumentContent,
  type DocumentSnapshot,
} from "../../../lib/tauri/library";
import {
  discardRecoverySnapshot,
  loadRecoverySnapshot,
  reloadDocumentFromDisk,
  saveRecoveryCopy,
  type RecoverySnapshot,
} from "../../../lib/tauri/persistence";
import {
  AutosaveController,
  type PersistenceProgress,
} from "../persistence/AutosaveController";
import { EditorSession, type EditorMode } from "../session/EditorSession";

const SourceDocumentEditor = lazy(() =>
  import("./SourceDocumentEditor").then((module) => ({
    default: module.SourceDocumentEditor,
  })),
);
const VisualDocumentEditor = lazy(() =>
  import("./VisualDocumentEditor").then((module) => ({
    default: module.VisualDocumentEditor,
  })),
);

type DocumentEditorProps = Readonly<{
  document: DocumentSnapshot;
  readOnly?: boolean;
  onRecoveryCopySaved?(document: DocumentSnapshot): void;
}>;

function nameOf(document: DocumentSnapshot): string {
  return document.relativePath.split("/").at(-1) ?? document.relativePath;
}

export function DocumentEditor({
  document,
  readOnly = false,
  onRecoveryCopySaved,
}: DocumentEditorProps) {
  const [editor, setEditor] = useState<EditorSession | null>(null);
  const [mode, setMode] = useState<EditorMode>(readOnly ? "source" : "visual");
  const [error, setError] = useState<string | null>(null);
  const [persistence, setPersistence] = useState<PersistenceProgress | null>(null);
  const [diskContent, setDiskContent] = useState<DocumentContent | null>(null);
  const [recovery, setRecovery] = useState<RecoverySnapshot | null>(null);
  const editorRef = useRef<EditorSession | null>(null);
  const autosaveRef = useRef<AutosaveController | null>(null);
  const sessionEpochRef = useRef(0);

  const startSession = useCallback((
    bytes: Uint8Array,
    baseFingerprint: string,
    recovered?: RecoverySnapshot,
  ) => {
    const sessionEpoch = sessionEpochRef.current + 1;
    sessionEpochRef.current = sessionEpoch;
    const opened = recovered
      ? EditorSession.openRecovered(bytes, recovered.revision)
      : EditorSession.open(bytes);
    editorRef.current = opened;
    if (!readOnly && !opened.session.isReadOnly) {
      autosaveRef.current = new AutosaveController({
        documentId: document.id,
        session: opened.session,
        baseFingerprint,
        ...(recovered ? { initialSnapshot: recovered } : {}),
        onProgress: setPersistence,
        onConflict: () => {
          void loadRecoverySnapshot(document.id).then(
            (pending) => {
              if (!pending || sessionEpochRef.current !== sessionEpoch) return;
              sessionEpochRef.current += 1;
              const controller = autosaveRef.current;
              autosaveRef.current = null;
              void controller?.dispose();
              editorRef.current?.dispose();
              editorRef.current = null;
              setEditor(null);
              setRecovery(pending);
            },
            (reason) => {
              if (sessionEpochRef.current === sessionEpoch) {
                setError(reason instanceof Error ? reason.message : "Recovery could not be loaded.");
              }
            },
          );
        },
      });
    }
    setEditor(opened);
    setMode(readOnly || opened.session.isReadOnly ? "source" : opened.mode);
  }, [document.id, readOnly]);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        let pending = await loadRecoverySnapshot(document.id);
        if (!active) return;
        let content: DocumentContent;
        try {
          content = await readDocument(document.id);
        } catch (reason) {
          if (!active) return;
          if (!pending) throw reason;
          setRecovery(pending);
          setPersistence({
            label: "Recovered",
            currentRevision: pending.revision,
            durableSnapshotRevision: pending.revision,
            diskRevision: 0,
          });
          return;
        }
        if (!active) return;
        if (pending?.intendedDiskHash === content.baseFingerprint) {
          await discardRecoverySnapshot(document.id, pending.sessionGeneration);
          pending = null;
        }
        if (!active) return;
        setDiskContent(content);
        if (pending) {
          setRecovery(pending);
          setPersistence({
            label: "Recovered",
            currentRevision: pending.revision,
            durableSnapshotRevision: pending.revision,
            diskRevision: 0,
          });
        } else {
          startSession(content.bytes, content.baseFingerprint);
        }
      } catch (reason) {
        if (active) setError(reason instanceof Error ? reason.message : "Document could not be opened.");
      }
    })();
    return () => {
      active = false;
      sessionEpochRef.current += 1;
      const autosave = autosaveRef.current;
      autosaveRef.current = null;
      void autosave?.dispose();
      editorRef.current?.dispose();
      editorRef.current = null;
    };
  }, [document.id, startSession]);

  useEffect(() => {
    const controller = autosaveRef.current;
    if (
      !editor ||
      recovery ||
      !diskContent ||
      document.diskFingerprint === diskContent.baseFingerprint ||
      document.diskFingerprint === controller?.diskFingerprint ||
      controller?.progress.label !== "Saved"
    ) {
      return;
    }

    let active = true;
    void readDocument(document.id).then(
      (content) => {
        if (
          !active ||
          autosaveRef.current !== controller ||
          controller.progress.label !== "Saved"
        ) {
          return;
        }
        const previousController = autosaveRef.current;
        autosaveRef.current = null;
        void previousController?.dispose();
        editorRef.current?.dispose();
        editorRef.current = null;
        setDiskContent(content);
        startSession(content.bytes, content.baseFingerprint);
      },
      (reason) => {
        if (active) {
          setError(reason instanceof Error ? reason.message : "External changes could not be loaded.");
        }
      },
    );
    return () => {
      active = false;
    };
  }, [diskContent, document.diskFingerprint, document.id, editor, recovery, startSession]);

  function switchMode(next: EditorMode) {
    if (!editor || (readOnly && next === "visual")) return;
    editor.switchMode(next);
    setMode(next);
  }

  function keepRecovery() {
    if (!recovery || !diskContent) return;
    setRecovery(null);
    startSession(recovery.bytes, recovery.baseFingerprint, recovery);
    autosaveRef.current?.acceptRecovery();
  }

  async function discardRecovery() {
    if (!recovery) return;
    setError(null);
    try {
      const content = await reloadDocumentFromDisk(
        document.id,
        recovery.sessionGeneration,
      );
      setRecovery(null);
      setPersistence(null);
      setDiskContent(content);
      startSession(content.bytes, content.baseFingerprint);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Recovery could not be discarded.");
    }
  }

  async function saveRecoveredCopy() {
    if (!recovery) return;
    setError(null);
    try {
      const copy = await saveRecoveryCopy(document.id, recovery.sessionGeneration);
      setRecovery(null);
      onRecoveryCopySaved?.(copy);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "The recovered copy could not be saved.");
    }
  }

  const recoveryCanResume =
    recovery !== null &&
    diskContent !== null &&
    recovery.lifecycleState !== "Conflict" &&
    recovery.baseFingerprint === diskContent.baseFingerprint;

  return (
    <div className="document-workspace">
      <header className="document-header">
        <div><span className="pane-kicker">Document</span><h2>{nameOf(document)}</h2></div>
        <div className="document-controls">
          {persistence && (
            <div className="persistence-state" aria-live="polite">
              <span data-state={persistence.label}>{persistence.label}</span>
              {persistence.label === "Save failed" && (
                <button type="button" onClick={() => autosaveRef.current?.retry()}>Retry</button>
              )}
            </div>
          )}
          <div className="mode-switch" aria-label="Editor mode">
            <button type="button" data-selected={mode === "visual"} disabled={!editor || editor.session.isReadOnly || readOnly} onClick={() => switchMode("visual")}>Visual</button>
            <button type="button" data-selected={mode === "source"} disabled={!editor} onClick={() => switchMode("source")}>Source</button>
          </div>
        </div>
      </header>
      {error && <div className="editor-message" role="alert">{error}</div>}
      {recovery && (
        <div className="recovery-panel" role="status">
          <h3>{recoveryCanResume ? "Recovered editing session" : "Recovered changes need a new copy"}</h3>
          <p>
            {recoveryCanResume
              ? "Cairn.md recovered changes that had not reached the Markdown file."
              : diskContent
                ? "The Markdown file changed outside Cairn.md. Both versions are preserved."
                : "The Markdown file is unavailable. Your recovered version is preserved."}
          </p>
          <pre>{new TextDecoder().decode(recovery.bytes)}</pre>
          <div>
            {recoveryCanResume && <button type="button" onClick={keepRecovery}>Keep recovered changes</button>}
            {!recoveryCanResume && <button type="button" onClick={() => void saveRecoveredCopy()}>Save as recovered copy</button>}
            <button type="button" onClick={() => void discardRecovery()}>Discard and reload file</button>
          </div>
        </div>
      )}
      {!editor && !recovery && !error && <div className="editor-message">Opening Markdown…</div>}
      {editor && (
        <Suspense fallback={<div className="editor-message">Loading {mode} mode…</div>}>
          {mode === "visual" ? (
            <VisualDocumentEditor visual={editor.visual} onRequestSource={() => switchMode("source")} />
          ) : (
            <SourceDocumentEditor session={editor.session} readOnly={readOnly} />
          )}
        </Suspense>
      )}
    </div>
  );
}
