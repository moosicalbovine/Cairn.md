import { useEffect, useRef, useState } from "react";

import type { ProjectSnapshot } from "../../../lib/tauri/library";
import type {
  TrackedFolderEntry,
  TrackedFolderSnapshot,
} from "../../../lib/tauri/import";
import { browseTrackedFolder } from "../tracked-folders/trackedFolderBrowser";

type LibraryPaneProps = Readonly<{
  projects: readonly ProjectSnapshot[];
  selectedProjectId: string | null;
  trackedFolders: readonly TrackedFolderSnapshot[];
  onSelectProject(project: ProjectSnapshot): void;
  onCreateProject(): void;
  onRenameProject(project: ProjectSnapshot): void;
  onAddTrackedFolder(): void;
  onRemoveTrackedFolder(folder: TrackedFolderSnapshot): void;
  onImportTracked(folderId: string, entry: TrackedFolderEntry): void;
  canMutate: boolean;
}>;

export function LibraryPane({
  projects,
  selectedProjectId,
  trackedFolders,
  onSelectProject,
  onCreateProject,
  onRenameProject,
  onAddTrackedFolder,
  onRemoveTrackedFolder,
  onImportTracked,
  canMutate,
}: LibraryPaneProps) {
  const [trackedOpen, setTrackedOpen] = useState(true);
  const [activeFolderId, setActiveFolderId] = useState<string | null>(null);
  const [directory, setDirectory] = useState<string | null>(null);
  const [entries, setEntries] = useState<readonly TrackedFolderEntry[]>([]);
  const [browseError, setBrowseError] = useState<string | null>(null);
  const projectButtons = useRef<Array<HTMLButtonElement | null>>([]);
  const browsedFolderId = trackedFolders.some((folder) => folder.id === activeFolderId)
    ? activeFolderId
    : null;

  useEffect(() => {
    if (browsedFolderId === null) {
      return;
    }
    let current = true;
    void browseTrackedFolder(browsedFolderId, directory).then(
      (value) => {
        if (current) {
          setBrowseError(null);
          setEntries(value);
        }
      },
      (reason) => {
        if (current) {
          setBrowseError(
            reason instanceof Error ? reason.message : "This tracked folder could not be opened.",
          );
        }
      },
    );
    return () => {
      current = false;
    };
  }, [browsedFolderId, directory]);

  function moveProjectFocus(index: number, direction: -1 | 1) {
    if (projects.length === 0) return;
    const nextIndex = (index + direction + projects.length) % projects.length;
    const nextProject = projects[nextIndex];
    if (nextProject) {
      onSelectProject(nextProject);
      projectButtons.current[nextIndex]?.focus();
    }
  }

  return (
    <div className="pane-stack navigation-stack">
      <div className="library-brand">
        <span className="brand-glyph" aria-hidden="true">C</span>
        <div><strong>Cairn.md</strong><small>Local library</small></div>
      </div>
      <div className="section-heading">
        <span>Projects</span>
        <button className="icon-button" type="button" disabled={!canMutate} onClick={onCreateProject} aria-label="Create project">+</button>
      </div>
      <nav className="project-list" aria-label="Projects">
        {projects.map((project, index) => (
          <div className="navigation-item" key={project.id}>
            <button
              ref={(button) => {
                projectButtons.current[index] = button;
              }}
              className="navigation-row"
              data-selected={selectedProjectId === project.id}
              type="button"
              onClick={() => onSelectProject(project)}
              onKeyDown={(event) => {
                if (event.key === "ArrowUp" || event.key === "ArrowDown") {
                  event.preventDefault();
                  moveProjectFocus(index, event.key === "ArrowUp" ? -1 : 1);
                }
              }}
            >
              <span aria-hidden="true">▱</span><span>{project.relativePath}</span>
            </button>
            <button
              className="row-action"
              type="button"
              disabled={!canMutate}
              aria-label={`Rename ${project.relativePath}`}
              onClick={() => onRenameProject(project)}
            >
              ⋯
            </button>
          </div>
        ))}
        {projects.length === 0 && <p className="empty-copy compact">Create your first project.</p>}
      </nav>
      <div className="section-heading tracked-heading">
        <button
          className="section-toggle"
          type="button"
          aria-expanded={trackedOpen}
          onClick={() => setTrackedOpen((open) => !open)}
        >
          <span aria-hidden="true">{trackedOpen ? "⌄" : "›"}</span> Tracked PC folders
        </button>
        <button className="icon-button" type="button" disabled={!canMutate} onClick={onAddTrackedFolder} aria-label="Add tracked PC folder">+</button>
      </div>
      {trackedOpen && (
        <div className="tracked-list">
          {trackedFolders.map((folder) => (
            <div className="navigation-item" key={folder.id}>
              <button
                className="navigation-row"
                data-selected={activeFolderId === folder.id}
                data-unavailable={!folder.available}
                type="button"
                disabled={!folder.available}
                onClick={() => {
                  setEntries([]);
                  setBrowseError(null);
                  setActiveFolderId(folder.id);
                  setDirectory(null);
                }}
              >
                <span aria-hidden="true">▱</span><span>{folder.displayName}</span>
              </button>
              <button
                className="row-action"
                type="button"
                disabled={!canMutate}
                aria-label={`Stop tracking ${folder.displayName}`}
                onClick={() => onRemoveTrackedFolder(folder)}
              >
                ×
              </button>
            </div>
          ))}
          {browsedFolderId && directory && (
            <button className="navigation-row tracked-entry" type="button" onClick={() => setDirectory(directory.includes("/") ? directory.slice(0, directory.lastIndexOf("/")) : null)}>
              <span aria-hidden="true">←</span><span>Back</span>
            </button>
          )}
          {browsedFolderId && entries.map((entry) => (
            <button
              key={entry.relativePath}
              className="navigation-row tracked-entry"
              type="button"
              disabled={!entry.isDirectory && !canMutate}
              onClick={() => entry.isDirectory ? setDirectory(entry.relativePath) : onImportTracked(browsedFolderId, entry)}
            >
              <span aria-hidden="true">{entry.isDirectory ? "▱" : "M↓"}</span>
              <span>{entry.displayName}</span>
            </button>
          ))}
          {browsedFolderId && browseError && <p className="inline-error" role="alert">{browseError}</p>}
        </div>
      )}
    </div>
  );
}
