import { lazy, Suspense, useEffect, useState } from "react";

import { readDocument, type DocumentSnapshot } from "../../../lib/tauri/library";
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
}>;

function nameOf(document: DocumentSnapshot): string {
  return document.relativePath.split("/").at(-1) ?? document.relativePath;
}

export function DocumentEditor({ document, readOnly = false }: DocumentEditorProps) {
  const [editor, setEditor] = useState<EditorSession | null>(null);
  const [mode, setMode] = useState<EditorMode>(readOnly ? "source" : "visual");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    let opened: EditorSession | null = null;
    void readDocument(document.id).then(
      (content) => {
        if (!active) return;
        opened = EditorSession.open(content.bytes);
        setEditor(opened);
        setMode(readOnly || opened.session.isReadOnly ? "source" : opened.mode);
      },
      (reason) => {
        if (active) setError(reason instanceof Error ? reason.message : "Document could not be opened.");
      },
    );
    return () => {
      active = false;
      opened?.dispose();
    };
  }, [document.id, readOnly]);

  function switchMode(next: EditorMode) {
    if (!editor || (readOnly && next === "visual")) return;
    editor.switchMode(next);
    setMode(next);
  }

  return (
    <div className="document-workspace">
      <header className="document-header">
        <div><span className="pane-kicker">Document</span><h2>{nameOf(document)}</h2></div>
        <div className="mode-switch" aria-label="Editor mode">
          <button type="button" data-selected={mode === "visual"} disabled={!editor || editor.session.isReadOnly || readOnly} onClick={() => switchMode("visual")}>Visual</button>
          <button type="button" data-selected={mode === "source"} disabled={!editor} onClick={() => switchMode("source")}>Source</button>
        </div>
      </header>
      {error && <div className="editor-message" role="alert">{error}</div>}
      {!editor && !error && <div className="editor-message">Opening Markdown…</div>}
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
