import { useEffect, useState } from "react";

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
  onAddTrackedFolder(): void;
  onImportTracked(folderId: string, entry: TrackedFolderEntry): void;
}>;

export function LibraryPane({
  projects,
  selectedProjectId,
  trackedFolders,
  onSelectProject,
  onCreateProject,
  onAddTrackedFolder,
  onImportTracked,
}: LibraryPaneProps) {
  const [trackedOpen, setTrackedOpen] = useState(true);
  const [activeFolderId, setActiveFolderId] = useState<string | null>(null);
  const [directory, setDirectory] = useState<string | null>(null);
  const [entries, setEntries] = useState<readonly TrackedFolderEntry[]>([]);

  useEffect(() => {
    if (activeFolderId === null) {
      setEntries([]);
      return;
    }
    let current = true;
    void browseTrackedFolder(activeFolderId, directory).then((value) => {
      if (current) setEntries(value);
    });
    return () => {
      current = false;
    };
  }, [activeFolderId, directory]);

  return (
    <div className="pane-stack navigation-stack">
      <div className="library-brand">
        <span className="brand-glyph" aria-hidden="true">C</span>
        <div><strong>Cairn.md</strong><small>Local library</small></div>
      </div>
      <div className="section-heading">
        <span>Projects</span>
        <button className="icon-button" type="button" onClick={onCreateProject} aria-label="Create project">+</button>
      </div>
      <nav className="project-list" aria-label="Projects">
        {projects.map((project) => (
          <button
            key={project.id}
            className="navigation-row"
            data-selected={selectedProjectId === project.id}
            type="button"
            onClick={() => onSelectProject(project)}
          >
            <span aria-hidden="true">▱</span><span>{project.relativePath}</span>
          </button>
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
        <button className="icon-button" type="button" onClick={onAddTrackedFolder} aria-label="Add tracked PC folder">+</button>
      </div>
      {trackedOpen && (
        <div className="tracked-list">
          {trackedFolders.map((folder) => (
            <button
              key={folder.id}
              className="navigation-row"
              data-selected={activeFolderId === folder.id}
              data-unavailable={!folder.available}
              type="button"
              disabled={!folder.available}
              onClick={() => {
                setActiveFolderId(folder.id);
                setDirectory(null);
              }}
            >
              <span aria-hidden="true">▱</span><span>{folder.displayName}</span>
            </button>
          ))}
          {activeFolderId && directory && (
            <button className="navigation-row tracked-entry" type="button" onClick={() => setDirectory(directory.includes("/") ? directory.slice(0, directory.lastIndexOf("/")) : null)}>
              <span aria-hidden="true">←</span><span>Back</span>
            </button>
          )}
          {entries.map((entry) => (
            <button
              key={entry.relativePath}
              className="navigation-row tracked-entry"
              type="button"
              onClick={() => entry.isDirectory ? setDirectory(entry.relativePath) : onImportTracked(activeFolderId ?? "", entry)}
            >
              <span aria-hidden="true">{entry.isDirectory ? "▱" : "M↓"}</span>
              <span>{entry.displayName}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
