import type { DocumentSnapshot, ProjectSnapshot } from "../../../lib/tauri/library";

type ContentsPaneProps = Readonly<{
  project: ProjectSnapshot | null;
  selectedDocumentId: string | null;
  onSelectDocument(document: DocumentSnapshot): void;
  onCreateDocument(): void;
  onImportFiles(): void;
  onCollapse(): void;
}>;

function documentName(document: DocumentSnapshot): string {
  return document.relativePath.split("/").at(-1) ?? document.relativePath;
}

export function ContentsPane({
  project,
  selectedDocumentId,
  onSelectDocument,
  onCreateDocument,
  onImportFiles,
  onCollapse,
}: ContentsPaneProps) {
  return (
    <div className="pane-stack">
      <header className="pane-header">
        <div>
          <span className="pane-kicker">Contents</span>
          <h2>{project?.relativePath ?? "Choose a project"}</h2>
        </div>
        <button className="icon-button" type="button" onClick={onCollapse} aria-label="Hide project contents">
          ‹
        </button>
      </header>
      {project && (
        <div className="pane-actions">
          <button type="button" onClick={onImportFiles}>Import files</button>
          <button type="button" onClick={onCreateDocument}>New document</button>
        </div>
      )}
      <div className="document-list" role="listbox" aria-label="Documents">
        {!project && <p className="empty-copy">Select a project from the library.</p>}
        {project?.documents.length === 0 && (
          <p className="empty-copy">No Markdown files yet. Import one or create a new document.</p>
        )}
        {project?.documents.map((document) => (
          <button
            key={document.id}
            className="document-row"
            data-selected={selectedDocumentId === document.id}
            role="option"
            aria-selected={selectedDocumentId === document.id}
            type="button"
            onClick={() => onSelectDocument(document)}
          >
            <span className="file-icon" aria-hidden="true">M↓</span>
            <span>{documentName(document)}</span>
          </button>
        ))}
      </div>
    </div>
  );
}
