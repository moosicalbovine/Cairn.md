import { lazy, Suspense, useEffect, useRef, useState } from "react";

import { WorkspaceLayout } from "../../../components/layout/WorkspaceLayout";
import {
  createDocument,
  createProject,
  deleteDocument,
  loadLibraryIndex,
  moveDocument,
  renameDocument,
  renameProject,
  watchLibraryReconciliation,
  type DocumentSnapshot,
  type LibrarySnapshot,
  type ProjectSnapshot,
} from "../../../lib/tauri/library";
import type { TrackedFolderSnapshot } from "../../../lib/tauri/import";
import { AppearanceSelect } from "../../settings/appearance/AppearanceSelect";
import type { Appearance } from "../../settings/appearance/appearance";
import { ImportQueue } from "../import/ImportQueue";
import {
  importChosenMarkdownFiles,
  importDroppedFiles,
  listenForDroppedFiles,
} from "../import/importAdapters";
import {
  addChosenTrackedFolder,
  importTrackedFile,
  loadTrackedFolders,
  stopTrackingFolder,
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
  const canMutate = snapshot.mode === "writable";

  useEffect(
    () =>
      watchLibraryReconciliation(
        setSnapshot,
        (reason) => {
          setNotice(
            reason instanceof Error
              ? reason.message
              : "The library could not refresh external changes.",
          );
        },
      ),
    [],
  );

  useEffect(() => {
    let active = true;
    let stopListening: (() => void) | undefined;

    void listenForDroppedFiles((paths) => {
      if (!active) return;
      if (!selectedProjectId) {
        setNotice("Choose a destination project before dropping Markdown files.");
        return;
      }
      if (!canMutate) {
        setNotice("This library is read-only, so files cannot be imported.");
        return;
      }

      setNotice(null);
      void (async () => {
        try {
          const imported = await importDroppedFiles(
            selectedProjectId,
            paths,
            importQueue.current,
          );
          if (!active) return;
          await loadLibraryIndex((nextSnapshot) => {
            if (active) setSnapshot(nextSnapshot);
          });
          if (!active) return;
          const latest = imported.at(-1);
          if (latest) setSelectedDocumentId(latest.document.id);
        } catch (reason) {
          if (active) {
            setNotice(reason instanceof Error ? reason.message : "Dropped files could not be imported.");
          }
        }
      })();
    }).then(
      (stop) => {
        if (active) stopListening = stop;
        else stop();
      },
      (reason) => {
        if (active) {
          setNotice(reason instanceof Error ? reason.message : "File drop is unavailable.");
        }
      },
    );

    return () => {
      active = false;
      stopListening?.();
    };
  }, [canMutate, selectedProjectId]);

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
            canMutate={canMutate}
            onSelectProject={selectProject}
            onCreateProject={() => void run(async () => {
              const name = askForName("Project name", "New project");
              if (!name) return;
              const project = await createProject(name);
              await refresh();
              setSelectedProjectId(project.id);
              setSelectedDocumentId(null);
            })}
            onRenameProject={(project) => void run(async () => {
              const name = askForName("Project name", project.relativePath);
              if (!name || name === project.relativePath) return;
              await renameProject(project.id, name);
              await refresh();
            })}
            onAddTrackedFolder={() => void run(async () => {
              const folder = await addChosenTrackedFolder();
              if (folder) setTrackedFolders(await loadTrackedFolders());
            })}
            onRemoveTrackedFolder={(folder) => void run(async () => {
              if (!globalThis.confirm(`Stop tracking “${folder.displayName}”? The original files will not be changed.`)) return;
              await stopTrackingFolder(folder.id);
              setTrackedFolders(await loadTrackedFolders());
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
            canMutate={canMutate}
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
            onRenameDocument={(document) => void run(async () => {
              const currentName = document.relativePath.split("/").at(-1) ?? document.relativePath;
              const name = askForName("Document name", currentName);
              if (!name || name === currentName) return;
              await renameDocument(document.id, name);
              await refresh(document.id);
            })}
            onMoveDocument={(document) => void run(async () => {
              const destinations = snapshot.projects.filter((project) => project.id !== selectedProjectId);
              const suggested = destinations[0];
              if (!suggested) throw new Error("Create another project before moving this document.");
              const name = askForName("Move to project", suggested.relativePath);
              if (!name) return;
              const destination = destinations.find(
                (project) => project.relativePath.toLocaleLowerCase() === name.toLocaleLowerCase(),
              );
              if (!destination) throw new Error(`No project named “${name}” was found.`);
              await moveDocument(document.id, destination.id);
              setSelectedProjectId(destination.id);
              await refresh(document.id);
            })}
            onDeleteDocument={(document) => void run(async () => {
              const name = document.relativePath.split("/").at(-1) ?? document.relativePath;
              if (!globalThis.confirm(`Delete “${name}”? Cairn.md will use the Recycle Bin when available.`)) return;
              await deleteDocument(document.id);
              setSelectedDocumentId(null);
              await refresh();
            })}
          />
        }
        editor={
          selectedDocument ? (
            <Suspense fallback={<div className="editor-message">Loading editor…</div>}>
              <DocumentEditor
                key={selectedDocument.id}
                document={selectedDocument}
                readOnly={!canMutate}
                onRecoveryCopySaved={(copy) => void run(async () => {
                  await refresh(copy.id);
                })}
              />
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
