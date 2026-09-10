import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  confirmLibraryRelink,
  createProject,
  loadLibraryIndex,
  parseDocumentContent,
  parseLibrarySnapshot,
  previewLibraryRelink,
  readDocument,
  watchLibraryReconciliation,
  type LibrarySnapshot,
  type RelinkPreview,
} from "../../lib/tauri/library";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const snapshot: LibrarySnapshot = {
  mode: "writable",
  readOnlyReason: null,
  binding: {
    libraryId: "library-1",
    rootPath: "C:\\Library",
    rootIdentity: "root-1",
    generation: 1,
  },
  projects: [],
};

describe("library command bridge", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it("renders the cached index before requesting authoritative reconciliation", async () => {
    const reconciled = { ...snapshot, projects: [] };
    vi.mocked(invoke)
      .mockResolvedValueOnce(snapshot)
      .mockResolvedValueOnce(reconciled);
    const rendered: LibrarySnapshot[] = [];

    await expect(loadLibraryIndex((value) => rendered.push(value))).resolves.toEqual(
      reconciled,
    );

    expect(rendered).toEqual([snapshot, reconciled]);
    expect(vi.mocked(invoke).mock.calls.map(([command]) => command)).toEqual([
      "library_snapshot",
      "reconcile_library",
    ]);
  });

  it("renders and returns an unbound cached index without reconciling", async () => {
    const unbound = { ...snapshot, binding: null };
    vi.mocked(invoke).mockResolvedValue(unbound);
    const rendered: LibrarySnapshot[] = [];

    await expect(loadLibraryIndex((value) => rendered.push(value))).resolves.toEqual(unbound);

    expect(rendered).toEqual([unbound]);
    expect(invoke).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith("library_snapshot");
  });

  it("renders and returns a read-only cached index without reconciling", async () => {
    const readOnly = {
      ...snapshot,
      mode: "readOnly" as const,
      readOnlyReason: "Library root is unavailable",
    };
    vi.mocked(invoke).mockResolvedValue(readOnly);
    const rendered: LibrarySnapshot[] = [];

    await expect(loadLibraryIndex((value) => rendered.push(value))).resolves.toEqual(readOnly);

    expect(rendered).toEqual([readOnly]);
    expect(invoke).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith("library_snapshot");
  });

  it("preserves candidateManifest across the relink preview contract", async () => {
    const preview: RelinkPreview = {
      candidatePath: "D:\\Library",
      libraryId: "library-1",
      expectedGeneration: 1,
      expectedStateToken: "state-token",
      rootIdentity: "root-2",
      matchedProjects: 2,
      matchedDocuments: 3,
      candidateManifest: '{"projects":[]}',
    };
    vi.mocked(invoke)
      .mockResolvedValueOnce(preview)
      .mockResolvedValueOnce(snapshot);

    const parsed = await previewLibraryRelink(preview.candidatePath);
    await expect(confirmLibraryRelink(parsed)).resolves.toEqual(snapshot);

    expect(parsed).toEqual(preview);
    expect(invoke).toHaveBeenNthCalledWith(1, "preview_library_relink", {
      candidatePath: preview.candidatePath,
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "confirm_library_relink", { preview });
  });

  it("rejects a relink preview without candidateManifest", async () => {
    vi.mocked(invoke).mockResolvedValue({
      candidatePath: "D:\\Library",
      libraryId: "library-1",
      expectedGeneration: 1,
      expectedStateToken: "state-token",
      rootIdentity: "root-2",
      matchedProjects: 2,
      matchedDocuments: 3,
    });

    await expect(previewLibraryRelink("D:\\Library")).rejects.toThrow(
      "Invalid relink preview",
    );
  });

  it("passes only a project name across the create-project boundary", async () => {
    vi.mocked(invoke).mockResolvedValue({
      id: "project-1",
      relativePath: "Alpha",
      documents: [],
    });

    await createProject("Alpha");

    expect(invoke).toHaveBeenCalledWith("create_project", { name: "Alpha" });
  });

  it("opens verified Markdown bytes through the document boundary", async () => {
    vi.mocked(invoke).mockResolvedValue({
      document: {
        id: "document-1",
        relativePath: "Alpha/note.md",
        sourcePath: null,
        importedAt: null,
        diskFingerprint: "sha256:abc",
      },
      bytes: [35, 32, 78, 111, 116, 101],
      baseFingerprint: "sha256:abc",
    });

    const content = await readDocument("document-1");

    expect([...content.bytes]).toEqual([35, 32, 78, 111, 116, 101]);
    expect(invoke).toHaveBeenCalledWith("read_document", {
      documentId: "document-1",
    });
  });

  it("rejects invalid byte payloads from the native boundary", () => {
    expect(() =>
      parseDocumentContent({
        document: {
          id: "document-1",
          relativePath: "Alpha/note.md",
          sourcePath: null,
          importedAt: null,
          diskFingerprint: "sha256:abc",
        },
        bytes: [256],
        baseFingerprint: "sha256:abc",
      }),
    ).toThrow("Invalid document content");
  });

  it("rejects malformed snapshots instead of trusting IPC data", () => {
    expect(() =>
      parseLibrarySnapshot({
        ...snapshot,
        projects: [{ id: "p", relativePath: "C:\\absolute", documents: [] }],
      }),
    ).toThrow("Invalid library snapshot");
  });

  it("consumes watcher hints without overlapping reconciliation calls", async () => {
    vi.useFakeTimers();
    const snapshots: LibrarySnapshot[] = [];
    let releaseFirst: ((value: unknown) => void) | undefined;
    vi.mocked(invoke)
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            releaseFirst = resolve;
          }),
      )
      .mockResolvedValueOnce(snapshot);

    const stop = watchLibraryReconciliation((value) => snapshots.push(value), vi.fn(), 100);
    await vi.advanceTimersByTimeAsync(300);
    expect(invoke).toHaveBeenCalledTimes(1);

    releaseFirst?.(null);
    await Promise.resolve();
    await vi.advanceTimersByTimeAsync(100);
    expect(invoke).toHaveBeenCalledTimes(2);
    await Promise.resolve();
    expect(snapshots).toEqual([snapshot]);

    stop();
    await vi.advanceTimersByTimeAsync(200);
    expect(invoke).toHaveBeenCalledTimes(2);
    vi.useRealTimers();
  });
});
