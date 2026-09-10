import { useRef } from "react";

import type { DocumentSnapshot, ProjectSnapshot } from "../../../lib/tauri/library";

type ContentsPaneProps = Readonly<{
  project: ProjectSnapshot | null;
  selectedDocumentId: string | null;
  onSelectDocument(document: DocumentSnapshot): void;
  onCreateDocument(): void;
  onImportFiles(): void;
  onRenameDocument(document: DocumentSnapshot): void;
  onMoveDocument(document: DocumentSnapshot): void;
  onDeleteDocument(document: DocumentSnapshot): void;
  onCollapse(): void;
  canMutate: boolean;
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
  onRenameDocument,
  onMoveDocument,
  onDeleteDocument,
  onCollapse,
  canMutate,
}: ContentsPaneProps) {
  const documentButtons = useRef<Array<HTMLButtonElement | null>>([]);
  const selectedDocument =
    project?.documents.find((document) => document.id === selectedDocumentId) ?? null;

  function moveFocus(index: number, direction: -1 | 1) {
    const count = project?.documents.length ?? 0;
    if (count === 0) return;
    const nextIndex = (index + direction + count) % count;
    const nextDocument = project?.documents[nextIndex];
    if (nextDocument) {
      onSelectDocument(nextDocument);
      documentButtons.current[nextIndex]?.focus();
    }
  }

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
          <button type="button" disabled={!canMutate} onClick={onImportFiles}>Import files</button>
          <button type="button" disabled={!canMutate} onClick={onCreateDocument}>New document</button>
        </div>
      )}
      {selectedDocument && (
        <div className="document-actions" aria-label="Selected document actions">
          <button type="button" disabled={!canMutate} onClick={() => onRenameDocument(selectedDocument)}>Rename</button>
          <button type="button" disabled={!canMutate} onClick={() => onMoveDocument(selectedDocument)}>Move</button>
          <button type="button" disabled={!canMutate} onClick={() => onDeleteDocument(selectedDocument)}>Delete</button>
        </div>
      )}
      <div className="document-list" role="listbox" aria-label="Documents">
        {!project && <p className="empty-copy">Select a project from the library.</p>}
        {project?.documents.length === 0 && (
          <p className="empty-copy">No Markdown files yet. Import one or create a new document.</p>
        )}
        {project?.documents.map((document, index) => (
          <button
            key={document.id}
            ref={(button) => {
              documentButtons.current[index] = button;
            }}
            className="document-row"
            data-selected={selectedDocumentId === document.id}
            role="option"
            aria-selected={selectedDocumentId === document.id}
            type="button"
            onClick={() => onSelectDocument(document)}
            onKeyDown={(event) => {
              if (event.key === "ArrowUp" || event.key === "ArrowDown") {
                event.preventDefault();
                moveFocus(index, event.key === "ArrowUp" ? -1 : 1);
              }
            }}
          >
            <span className="file-icon" aria-hidden="true">M↓</span>
            <span>{documentName(document)}</span>
          </button>
        ))}
      </div>
    </div>
  );
}
