import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { Workspace } from "../../src/features/library/components/Workspace";
import type { LibrarySnapshot } from "../../src/lib/tauri/library";

const libraryMocks = vi.hoisted(() => ({
  previewLibraryRelink: vi.fn(),
  confirmLibraryRelink: vi.fn(),
  chooseLibraryRoot: vi.fn(),
}));
const editorMocks = vi.hoisted(() => ({
  ensureRecoveryDurable: vi.fn(async (): Promise<void> => undefined),
}));
type CloseHandler = (event: { preventDefault(): void }) => void | Promise<void>;
const windowMocks = vi.hoisted(() => ({
  closeHandler: null as CloseHandler | null,
  destroy: vi.fn(async (): Promise<void> => undefined),
  onCloseRequested: vi.fn(async (handler: CloseHandler): Promise<() => void> => {
    windowMocks.closeHandler = handler;
    return () => undefined;
  }),
}));

vi.mock("../../src/lib/tauri/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../src/lib/tauri/library")>()),
  watchLibraryReconciliation: vi.fn(() => () => undefined),
  reconcileLibraryIndex: vi.fn(async () => undefined),
  previewLibraryRelink: libraryMocks.previewLibraryRelink,
  confirmLibraryRelink: libraryMocks.confirmLibraryRelink,
}));
vi.mock("../../src/features/library/libraryRootPicker", () => ({
  chooseLibraryRoot: libraryMocks.chooseLibraryRoot,
}));
vi.mock("../../src/features/library/tracked-folders/trackedFolderBrowser", () => ({
  addChosenTrackedFolder: vi.fn(),
  browseTrackedFolder: vi.fn(async () => []),
  importTrackedFile: vi.fn(),
  loadTrackedFolders: vi.fn(async () => []),
  stopTrackingFolder: vi.fn(),
}));
vi.mock("../../src/features/library/import/importAdapters", () => ({
  importChosenMarkdownFiles: vi.fn(async () => []),
  importDroppedFiles: vi.fn(async () => []),
  listenForDroppedFiles: vi.fn(async () => () => undefined),
}));
vi.mock("../../src/features/editor/components/DocumentEditor", async () => {
  const { forwardRef, useImperativeHandle } = await import("react");
  return {
    DocumentEditor: forwardRef(function MockDocumentEditor({
      document,
      readOnly,
    }: {
      document: { relativePath: string };
      readOnly?: boolean;
    }, ref) {
      useImperativeHandle(ref, () => ({
        ensureRecoveryDurable: editorMocks.ensureRecoveryDurable,
      }));
      return (
        <div data-testid="document-editor" data-read-only={String(readOnly)}>
          <h2>{document.relativePath.split("/").at(-1)}</h2>
        </div>
      );
    }),
  };
});
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    destroy: windowMocks.destroy,
    onCloseRequested: windowMocks.onCloseRequested,
  }),
}));

const snapshot: LibrarySnapshot = {
  mode: "writable",
  readOnlyReason: null,
  binding: {
    libraryId: "library-1",
    rootPath: "C:\\Notes",
    rootIdentity: "root-1",
    generation: 1,
  },
  projects: [
    {
      id: "project-1",
      relativePath: "Alpha",
      documents: [
        {
          id: "document-1",
          relativePath: "Alpha/brief.md",
          sourcePath: null,
          importedAt: null,
          diskFingerprint: "sha256:abc",
        },
      ],
    },
  ],
};

beforeEach(() => {
  editorMocks.ensureRecoveryDurable.mockReset();
  editorMocks.ensureRecoveryDurable.mockResolvedValue(undefined);
  windowMocks.closeHandler = null;
  windowMocks.destroy.mockClear();
  windowMocks.onCloseRequested.mockClear();
});

describe("three-pane workspace", () => {
  it("keeps the active document visible when contents are collapsed and restored", async () => {
    render(
      <Workspace
        initialSnapshot={snapshot}
        initialTrackedFolders={[]}
        appearance="followWindows"
        onAppearanceChange={vi.fn()}
      />,
    );

    expect(screen.getByLabelText("Library")).toBeInTheDocument();
    expect(screen.getByLabelText("Project contents")).toBeInTheDocument();
    expect(screen.getByLabelText("Document editor")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("option", { name: /brief\.md/i }));
    expect(await screen.findByRole("heading", { name: "brief.md" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Hide project contents" }));
    expect(screen.queryByLabelText("Project contents")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "brief.md" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Show contents" })).toHaveFocus();

    fireEvent.click(screen.getByRole("button", { name: "Show contents" }));
    expect(screen.getByLabelText("Project contents")).toBeInTheDocument();
    expect(screen.getByRole("option", { name: /brief\.md/i })).toHaveFocus();
  });

  it("exposes only the approved appearance choices", () => {
    render(
      <Workspace
        initialSnapshot={snapshot}
        initialTrackedFolders={[]}
        appearance="followWindows"
        onAppearanceChange={vi.fn()}
      />,
    );

    expect(screen.getByRole("combobox", { name: "Appearance" })).toHaveTextContent(
      "Follow WindowsLightDark",
    );
    expect(screen.queryByText(/preview|split/i)).not.toBeInTheDocument();
  });

  it("waits for durable recovery before opening another document", async () => {
    let releaseRecovery: (() => void) | undefined;
    editorMocks.ensureRecoveryDurable.mockImplementationOnce(
      () => new Promise<void>((resolve) => {
        releaseRecovery = resolve;
      }),
    );
    const secondDocument = {
      ...snapshot.projects[0]!.documents[0]!,
      id: "document-2",
      relativePath: "Alpha/notes.md",
    };
    render(
      <Workspace
        initialSnapshot={{
          ...snapshot,
          projects: [{
            ...snapshot.projects[0]!,
            documents: [...snapshot.projects[0]!.documents, secondDocument],
          }],
        }}
        initialTrackedFolders={[]}
        appearance="followWindows"
        onAppearanceChange={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("option", { name: /brief\.md/i }));
    expect(await screen.findByRole("heading", { name: "brief.md" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("option", { name: /notes\.md/i }));

    expect(editorMocks.ensureRecoveryDurable).toHaveBeenCalledOnce();
    expect(screen.getByRole("heading", { name: "brief.md" })).toBeInTheDocument();
    releaseRecovery?.();
    expect(await screen.findByRole("heading", { name: "notes.md" })).toBeInTheDocument();
  });

  it("keeps the window open until the active draft is durable", async () => {
    let releaseRecovery: (() => void) | undefined;
    editorMocks.ensureRecoveryDurable.mockImplementationOnce(
      () => new Promise<void>((resolve) => {
        releaseRecovery = resolve;
      }),
    );
    render(
      <Workspace
        initialSnapshot={snapshot}
        initialTrackedFolders={[]}
        appearance="followWindows"
        onAppearanceChange={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("option", { name: /brief\.md/i }));
    expect(await screen.findByRole("heading", { name: "brief.md" })).toBeInTheDocument();
    await waitFor(() => expect(windowMocks.closeHandler).not.toBeNull());
    const preventDefault = vi.fn();

    const closeResult = windowMocks.closeHandler?.({ preventDefault });
    expect(preventDefault).toHaveBeenCalledOnce();
    expect(windowMocks.destroy).not.toHaveBeenCalled();
    releaseRecovery?.();
    await closeResult;

    expect(editorMocks.ensureRecoveryDurable).toHaveBeenCalledOnce();
    expect(windowMocks.destroy).toHaveBeenCalledOnce();
  });

  it("cancels closing when the active draft cannot reach recovery storage", async () => {
    editorMocks.ensureRecoveryDurable.mockRejectedValueOnce(
      new Error("Recovery storage is unavailable."),
    );
    render(
      <Workspace
        initialSnapshot={snapshot}
        initialTrackedFolders={[]}
        appearance="followWindows"
        onAppearanceChange={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("option", { name: /brief\.md/i }));
    expect(await screen.findByRole("heading", { name: "brief.md" })).toBeInTheDocument();
    await waitFor(() => expect(windowMocks.closeHandler).not.toBeNull());
    const preventDefault = vi.fn();

    await windowMocks.closeHandler?.({ preventDefault });

    expect(preventDefault).toHaveBeenCalledOnce();
    expect(windowMocks.destroy).not.toHaveBeenCalled();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Recovery storage is unavailable.",
    );
  });

  it("disables library mutations and editing when the library is read-only", async () => {
    render(
      <Workspace
        initialSnapshot={{
          ...snapshot,
          mode: "readOnly",
          readOnlyReason: "The folder is unavailable.",
        }}
        initialTrackedFolders={[]}
        appearance="dark"
        onAppearanceChange={vi.fn()}
      />,
    );

    expect(screen.getByRole("button", { name: "Create project" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Import files" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "New document" })).toBeDisabled();

    fireEvent.click(screen.getByRole("option", { name: /brief\.md/i }));
    expect(await screen.findByTestId("document-editor")).toHaveAttribute("data-read-only", "true");
  });

  it("reconnects a moved library only after showing the match summary", async () => {
    const readOnly = {
      ...snapshot,
      mode: "readOnly" as const,
      readOnlyReason: "root_unavailable",
    };
    const relinked = {
      ...snapshot,
      binding: { ...snapshot.binding!, rootPath: "D:\\Notes", generation: 2 },
    };
    libraryMocks.chooseLibraryRoot.mockResolvedValue("D:\\Notes");
    libraryMocks.previewLibraryRelink.mockResolvedValue({
      candidatePath: "D:\\Notes",
      libraryId: "library-1",
      expectedGeneration: 1,
      expectedStateToken: "state-token",
      rootIdentity: "root-2",
      matchedProjects: 1,
      matchedDocuments: 1,
      candidateManifest: "{}",
    });
    libraryMocks.confirmLibraryRelink.mockResolvedValue(relinked);
    vi.spyOn(globalThis, "confirm").mockReturnValue(true);

    render(
      <Workspace
        initialSnapshot={readOnly}
        initialTrackedFolders={[]}
        appearance="dark"
        onAppearanceChange={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Reconnect library" }));

    await waitFor(() => expect(libraryMocks.confirmLibraryRelink).toHaveBeenCalledOnce());
    expect(screen.queryByText(/library opened read-only/i)).not.toBeInTheDocument();
    expect(screen.getByText("D:\\Notes")).toBeInTheDocument();
  });
});
