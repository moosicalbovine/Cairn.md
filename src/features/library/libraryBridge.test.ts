import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  createProject,
  loadLibraryIndex,
  parseLibrarySnapshot,
  watchLibraryReconciliation,
  type LibrarySnapshot,
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

  it("passes only a project name across the create-project boundary", async () => {
    vi.mocked(invoke).mockResolvedValue({
      id: "project-1",
      relativePath: "Alpha",
      documents: [],
    });

    await createProject("Alpha");

    expect(invoke).toHaveBeenCalledWith("create_project", { name: "Alpha" });
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
