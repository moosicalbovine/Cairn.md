import { lazy, Suspense, useRef, useState } from "react";

import { WorkspaceLayout } from "../../../components/layout/WorkspaceLayout";
import {
  createDocument,
  createProject,
  loadLibraryIndex,
  type DocumentSnapshot,
  type LibrarySnapshot,
  type ProjectSnapshot,
} from "../../../lib/tauri/library";
import type { TrackedFolderSnapshot } from "../../../lib/tauri/import";
import { AppearanceSelect } from "../../settings/appearance/AppearanceSelect";
import type { Appearance } from "../../settings/appearance/appearance";
import { ImportQueue } from "../import/ImportQueue";
import { importChosenMarkdownFiles } from "../import/importAdapters";
import {
  addChosenTrackedFolder,
  importTrackedFile,
  loadTrackedFolders,
} from "../tracked-folders/trackedFolderBrowser";
import { ContentsPane } from "./ContentsPane";
import { LibraryPane } from "./LibraryPane";

const DocumentEditor = lazy(() =>
  import("../../editor/components/DocumentEditor").then((module) => ({
    default: module.DocumentEditor,
  })),
);

type WorkspaceProps = Readonly<{
  initialSnapshot: LibrarySnapshot;
  initialTrackedFolders: readonly TrackedFolderSnapshot[];
  appearance: Appearance;
  onAppearanceChange(value: Appearance): void;
}>;

function askForName(message: string, defaultValue: string): string | null {
  const value = globalThis.prompt(message, defaultValue)?.trim();
  return value ? value : null;
}

export function Workspace({
  initialSnapshot,
  initialTrackedFolders,
  appearance,
  onAppearanceChange,
}: WorkspaceProps) {
  const [snapshot, setSnapshot] = useState(initialSnapshot);
  const [trackedFolders, setTrackedFolders] = useState(initialTrackedFolders);
  const [selectedProjectId, setSelectedProjectId] = useState<string | null>(
    initialSnapshot.projects[0]?.id ?? null,
  );
  const [selectedDocumentId, setSelectedDocumentId] = useState<string | null>(null);
  const [contentsVisible, setContentsVisible] = useState(true);
  const [notice, setNotice] = useState<string | null>(null);
  const importQueue = useRef(new ImportQueue());

  const selectedProject =
    snapshot.projects.find((project) => project.id === selectedProjectId) ?? null;
  const selectedDocument =
    selectedProject?.documents.find((document) => document.id === selectedDocumentId) ?? null;

  async function refresh(preferredDocumentId?: string) {
    const next = await loadLibraryIndex(setSnapshot);
    if (preferredDocumentId) setSelectedDocumentId(preferredDocumentId);
    if (!selectedProjectId && next.projects[0]) setSelectedProjectId(next.projects[0].id);
  }

  async function run(action: () => Promise<void>) {
    setNotice(null);
    try {
      await action();
    } catch (reason) {
      setNotice(reason instanceof Error ? reason.message : "Cairn.md could not complete that action.");
    }
  }

  function selectProject(project: ProjectSnapshot) {
    setSelectedProjectId(project.id);
    setSelectedDocumentId(project.documents[0]?.id ?? null);
  }

  function selectDocument(document: DocumentSnapshot) {
    setSelectedDocumentId(document.id);
  }

  return (
    <main className="desktop-shell">
      <header className="app-titlebar">
        <div><strong>Cairn.md</strong><span>{snapshot.binding?.rootPath}</span></div>
        <AppearanceSelect value={appearance} onChange={onAppearanceChange} />
      </header>
      {snapshot.mode === "readOnly" && (
        <div className="read-only-banner" role="status">
          Library opened read-only: {snapshot.readOnlyReason ?? "writes are unavailable"}
        </div>
      )}
      {notice && <div className="notice-banner" role="alert">{notice}</div>}
      <WorkspaceLayout
        contentsVisible={contentsVisible}
        onContentsVisibleChange={setContentsVisible}
        library={
          <LibraryPane
            projects={snapshot.projects}
            selectedProjectId={selectedProjectId}
            trackedFolders={trackedFolders}
            onSelectProject={selectProject}
            onCreateProject={() => void run(async () => {
              const name = askForName("Project name", "New project");
              if (!name) return;
              const project = await createProject(name);
              await refresh();
              setSelectedProjectId(project.id);
              setSelectedDocumentId(null);
            })}
            onAddTrackedFolder={() => void run(async () => {
              const folder = await addChosenTrackedFolder();
              if (folder) setTrackedFolders(await loadTrackedFolders());
            })}
            onImportTracked={(folderId, entry) => void run(async () => {
              if (!selectedProject) throw new Error("Choose a destination project first.");
              const imported = await importTrackedFile(selectedProject.id, folderId, entry, importQueue.current);
              await refresh(imported.document.id);
            })}
          />
        }
        contents={
          <ContentsPane
            project={selectedProject}
            selectedDocumentId={selectedDocumentId}
            onSelectDocument={selectDocument}
            onCollapse={() => setContentsVisible(false)}
            onImportFiles={() => void run(async () => {
              if (!selectedProject) return;
              const imported = await importChosenMarkdownFiles(selectedProject.id, importQueue.current);
              const latest = imported.at(-1);
              if (latest) await refresh(latest.document.id);
            })}
            onCreateDocument={() => void run(async () => {
              if (!selectedProject) return;
              const name = askForName("Document name", "Untitled.md");
              if (!name) return;
              const document = await createDocument(selectedProject.id, name);
              await refresh(document.id);
            })}
          />
        }
        editor={
          selectedDocument ? (
            <Suspense fallback={<div className="editor-message">Loading editor…</div>}>
              <DocumentEditor key={selectedDocument.id} document={selectedDocument} />
            </Suspense>
          ) : (
            <div className="editor-empty">
              <>
                <span className="empty-glyph" aria-hidden="true">M↓</span>
                <h2>Select a Markdown document</h2>
                <p>Your document will open here while the project contents remain available.</p>
              </>
            </div>
          )
        }
      />
    </main>
  );
}
