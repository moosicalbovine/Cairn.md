import { editorViewCtx } from "@milkdown/kit/core";
import { useEffect, useRef, useState } from "react";

import {
  getPerformanceFixturePaths,
  writeInstalledSmokeReport,
  type InstalledSmokeReport,
} from "../../lib/tauri/performance";
import {
  addTrackedFolder,
  importMarkdown,
  listTrackedFolderEntries,
} from "../../lib/tauri/import";
import {
  bindLibraryRoot,
  createDocument,
  createProject,
  readDocument,
} from "../../lib/tauri/library";
import { saveDocument, storeRecoverySnapshot } from "../../lib/tauri/persistence";
import { EditorSession } from "../editor/session/EditorSession";
import { createVisualSegmentEditor } from "../editor/visual/createVisualSegmentEditor";

const originalTrackedSource = "# Original tracked file remains unchanged.";
const visualEdit = "Edited through the installed Cairn.md visual editor.";

function initialReport(message: string): InstalledSmokeReport {
  return {
    projectCreated: false,
    trackedImport: false,
    visualEdit: false,
    sourceMode: false,
    autosave: false,
    message,
  };
}

async function afterPaint(): Promise<void> {
  await new Promise<void>((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
  });
}

export function InstalledSmokeRunner() {
  const editorHost = useRef<HTMLDivElement>(null);
  const [status, setStatus] = useState("Checking the installed Cairn.md workflow…");

  useEffect(() => {
    const host = editorHost.current;
    if (!host) return;
    let active = true;

    void (async () => {
      let report = initialReport("Installed workflow did not complete.");
      let editor: Awaited<ReturnType<typeof createVisualSegmentEditor>> | null = null;
      let session: EditorSession | null = null;
      try {
        const paths = await getPerformanceFixturePaths();
        await bindLibraryRoot(paths.libraryRoot);
        const tracked = await addTrackedFolder(paths.trackedRoot);
        const project = await createProject("Installed Project");
        report = { ...report, projectCreated: true };

        const entries = await listTrackedFolderEntries(tracked.id);
        const trackedEntry = entries.find(
          (entry) => !entry.isDirectory && entry.relativePath === "tracked-proof.md",
        );
        if (!trackedEntry) throw new Error("The tracked Markdown fixture was not found.");
        const imported = await importMarkdown(project.id, {
          kind: "trackedFile",
          trackedFolderId: tracked.id,
          relativePath: trackedEntry.relativePath,
        });
        const importedContent = await readDocument(imported.id);
        if (new TextDecoder().decode(importedContent.bytes) !== originalTrackedSource) {
          throw new Error("The imported Markdown copy did not match its source.");
        }
        report = { ...report, trackedImport: true };

        const document = await createDocument(project.id, "installed-proof.md");
        const content = await readDocument(document.id);
        session = EditorSession.open(content.bytes);
        const segment = session.visual.projection.segments.find(
          (candidate) => candidate.kind === "visual",
        );
        if (!segment) throw new Error("The installed visual editor had no editable region.");
        editor = await createVisualSegmentEditor(host, session.visual, segment.id);
        editor.editor.action((ctx) => {
          const view = ctx.get(editorViewCtx);
          view.dispatch(view.state.tr.insertText(visualEdit));
        });
        await afterPaint();
        if (!session.session.source.includes(visualEdit)) {
          throw new Error("The visual edit did not reach canonical Markdown.");
        }
        report = { ...report, visualEdit: true };

        session.switchMode("source");
        if (!session.source.source.includes(visualEdit)) {
          throw new Error("Source mode did not contain the visual edit.");
        }
        report = { ...report, sourceMode: true };

        const request = {
          documentId: document.id,
          sessionGeneration: crypto.randomUUID(),
          revision: session.session.revision,
          bytes: session.session.toBytes(),
          baseFingerprint: content.baseFingerprint,
        };
        await storeRecoverySnapshot(request);
        const saved = await saveDocument(request);
        const disk = await readDocument(document.id);
        if (
          saved.status !== "saved" ||
          !new TextDecoder().decode(disk.bytes).includes(visualEdit)
        ) {
          throw new Error("The installed autosave pipeline did not persist the visual edit.");
        }
        report = {
          ...report,
          autosave: true,
          message: "Installed project, tracked import, visual edit, Source view, and autosave passed.",
        };
      } catch (reason) {
        report = {
          ...report,
          message: reason instanceof Error ? reason.message : "Installed workflow failed.",
        };
      } finally {
        if (editor) await editor.destroy();
        session?.dispose();
      }

      try {
        await writeInstalledSmokeReport(report);
        if (active) setStatus(report.autosave ? "Installed workflow passed." : report.message);
      } catch (reason) {
        if (active) {
          setStatus(reason instanceof Error ? reason.message : "Installed report could not be written.");
        }
      }
    })();

    return () => {
      active = false;
    };
  }, []);

  return (
    <main className="performance-runner">
      <p>{status}</p>
      <div className="performance-editor" ref={editorHost} />
    </main>
  );
}
