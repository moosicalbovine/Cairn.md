import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { DocumentEditor } from "../../src/features/editor/components/DocumentEditor";
import type { DocumentContent, DocumentSnapshot } from "../../src/lib/tauri/library";
import type { RecoverySnapshot } from "../../src/lib/tauri/persistence";

const mocks = vi.hoisted(() => ({
  readDocument: vi.fn(),
  loadRecoverySnapshot: vi.fn(),
  discardRecoverySnapshot: vi.fn(async () => true),
  saveDocument: vi.fn(),
  storeRecoverySnapshot: vi.fn(),
  saveRecoveryCopy: vi.fn(),
}));

vi.mock("../../src/lib/tauri/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../src/lib/tauri/library")>()),
  readDocument: mocks.readDocument,
}));
vi.mock("../../src/lib/tauri/persistence", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../src/lib/tauri/persistence")>()),
  loadRecoverySnapshot: mocks.loadRecoverySnapshot,
  discardRecoverySnapshot: mocks.discardRecoverySnapshot,
  saveDocument: mocks.saveDocument,
  storeRecoverySnapshot: mocks.storeRecoverySnapshot,
  saveRecoveryCopy: mocks.saveRecoveryCopy,
}));
vi.mock("../../src/features/editor/components/VisualDocumentEditor", () => ({
  VisualDocumentEditor: ({ visual }: { visual: { projection: { revision: number } } }) => (
    <div data-testid="visual-editor">Revision {visual.projection.revision}</div>
  ),
}));
vi.mock("../../src/features/editor/components/SourceDocumentEditor", () => ({
  SourceDocumentEditor: () => <div data-testid="source-editor" />,
}));

const document: DocumentSnapshot = {
  id: "document-1",
  relativePath: "Alpha/draft.md",
  sourcePath: null,
  importedAt: null,
  diskFingerprint: "disk-base",
};

const diskContent: DocumentContent = {
  document,
  bytes: new TextEncoder().encode("disk"),
  baseFingerprint: "disk-base",
};

function recovery(lifecycleState: RecoverySnapshot["lifecycleState"]): RecoverySnapshot {
  return {
    documentId: document.id,
    sessionGeneration: "9fc397f3-5287-4e2d-83c3-b1ad7180f824",
    revision: 4,
    bytes: new TextEncoder().encode("recovered draft"),
    contentHash: "draft-hash",
    baseFingerprint: lifecycleState === "Conflict" ? "older-base" : "disk-base",
    intendedDiskHash: "draft-hash",
    operationId: lifecycleState === "Conflict" ? "operation-1" : null,
    lifecycleState,
    durableAt: 42,
  };
}

beforeEach(() => vi.clearAllMocks());

describe("autosave recovery workspace", () => {
  it("resumes a clean interrupted session at its durable revision", async () => {
    const pending = recovery("Draft");
    mocks.readDocument.mockResolvedValue(diskContent);
    mocks.loadRecoverySnapshot.mockResolvedValue(pending);
    mocks.saveDocument.mockResolvedValue({
      status: "saved",
      revision: pending.revision,
      diskFingerprint: pending.intendedDiskHash,
    });

    render(<DocumentEditor document={document} />);

    expect(await screen.findByText("Recovered")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Keep recovered changes" }));

    expect(await screen.findByTestId("visual-editor")).toHaveTextContent("Revision 4");
    await waitFor(() => expect(mocks.saveDocument).toHaveBeenCalledTimes(1));
    expect(await screen.findByText("Saved")).toBeInTheDocument();
  });

  it("offers a separate Markdown copy when disk changed externally", async () => {
    const pending = recovery("Conflict");
    const copy = { ...document, id: "copy-1", relativePath: "Alpha/draft (recovered).md" };
    const onRecoveryCopySaved = vi.fn();
    mocks.readDocument.mockResolvedValue(diskContent);
    mocks.loadRecoverySnapshot.mockResolvedValue(pending);
    mocks.saveRecoveryCopy.mockResolvedValue(copy);

    render(
      <DocumentEditor
        document={document}
        onRecoveryCopySaved={onRecoveryCopySaved}
      />,
    );

    expect(await screen.findByText(/both versions are preserved/i)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Keep recovered changes" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save as recovered copy" }));

    await waitFor(() => expect(onRecoveryCopySaved).toHaveBeenCalledWith(copy));
    expect(mocks.discardRecoverySnapshot).not.toHaveBeenCalled();
  });

  it("clears a snapshot whose intended bytes are already on disk", async () => {
    const pending = { ...recovery("Saving"), intendedDiskHash: "disk-base" };
    mocks.readDocument.mockResolvedValue(diskContent);
    mocks.loadRecoverySnapshot.mockResolvedValue(pending);

    render(<DocumentEditor document={document} readOnly={true} />);

    expect(await screen.findByTestId("source-editor")).toBeInTheDocument();
    expect(mocks.discardRecoverySnapshot).toHaveBeenCalledWith(
      document.id,
      pending.sessionGeneration,
    );
    expect(screen.queryByText("Recovered")).not.toBeInTheDocument();
  });
});
