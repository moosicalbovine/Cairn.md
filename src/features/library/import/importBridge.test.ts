import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  addTrackedFolder,
  importMarkdown,
  listTrackedFolderEntries,
  listTrackedFolders,
  parseTrackedFolderEntry,
  parseTrackedFolderSnapshot,
  removeTrackedFolder,
} from "../../../lib/tauri/import";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const document = {
  id: "document-1",
  relativePath: "Alpha/note.md",
  sourcePath: "C:\\Inbox\\note.md",
  importedAt: 1_700_000_000_000,
  diskFingerprint: "sha256:abc",
};

describe("import command bridge", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it("passes the tagged source to the single import command", async () => {
    vi.mocked(invoke).mockResolvedValue(document);
    const source = {
      kind: "externalPath" as const,
      absolutePath: "C:\\Inbox\\note.md",
    };

    await expect(importMarkdown("project-1", source)).resolves.toEqual(document);
    expect(invoke).toHaveBeenCalledWith("import_markdown", {
      projectId: "project-1",
      source,
    });
  });

  it("wraps the narrow tracked-folder commands", async () => {
    const folder = {
      id: "folder-1",
      absolutePath: "C:\\Inbox",
      displayName: "Inbox",
      available: true,
      lastScanAt: null,
    };
    const entry = {
      relativePath: "Planning/note.md",
      displayName: "note.md",
      isDirectory: false,
    };
    vi.mocked(invoke)
      .mockResolvedValueOnce(folder)
      .mockResolvedValueOnce([folder])
      .mockResolvedValueOnce([entry])
      .mockResolvedValueOnce(undefined);

    await expect(addTrackedFolder("C:\\Inbox")).resolves.toEqual(folder);
    await expect(listTrackedFolders()).resolves.toEqual([folder]);
    await expect(listTrackedFolderEntries("folder-1", "Planning")).resolves.toEqual([
      entry,
    ]);
    await removeTrackedFolder("folder-1");

    expect(vi.mocked(invoke).mock.calls).toEqual([
      ["add_tracked_folder", { path: "C:\\Inbox" }],
      ["list_tracked_folders"],
      [
        "list_tracked_folder_entries",
        { folderId: "folder-1", relativeDirectory: "Planning" },
      ],
      ["remove_tracked_folder", { id: "folder-1" }],
    ]);
  });

  it("rejects malformed native payloads at the boundary", () => {
    expect(() =>
      parseTrackedFolderSnapshot({
        id: "folder-1",
        absolutePath: "C:\\Inbox",
        displayName: "Inbox",
        available: "yes",
        lastScanAt: null,
      }),
    ).toThrow(/tracked folder snapshot/i);
    expect(() =>
      parseTrackedFolderEntry({
        relativePath: "../note.md",
        displayName: "note.md",
        isDirectory: false,
      }),
    ).toThrow(/tracked folder entry/i);
  });
});
