// @vitest-environment node
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ImportQueue, type Importer } from "../../src/features/library/import/ImportQueue";
import {
  importChosenMarkdownFiles,
  importDroppedFiles,
  listenForDroppedFiles,
} from "../../src/features/library/import/importAdapters";
import { importTrackedFile } from "../../src/features/library/tracked-folders/trackedFolderBrowser";
import type { ImportSource } from "../../src/lib/tauri/import";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: vi.fn() }));

function document(id: string) {
  return {
    id,
    relativePath: `Alpha/${id}.md`,
    sourcePath: `C:\\Inbox\\${id}.md`,
    importedAt: 1_700_000_000_000,
    diskFingerprint: `sha256:${id}`,
  };
}

describe("the three import entry points", () => {
  beforeEach(() => {
    vi.mocked(open).mockReset();
    vi.mocked(getCurrentWebview).mockReset();
  });

  it("adapts picker, drop, and tracked selections into one queue", async () => {
    const received: ImportSource[] = [];
    const importer: Importer = vi.fn(async (_projectId, source) => {
      received.push(source);
      return document(`document-${received.length}`);
    });
    const queue = new ImportQueue(importer);
    vi.mocked(open).mockResolvedValue(["C:\\Inbox\\picked.md"]);

    const picked = await importChosenMarkdownFiles("project-1", queue);
    const dropped = await importDroppedFiles(
      "project-1",
      ["C:\\Inbox\\dropped.md"],
      queue,
    );
    const tracked = await importTrackedFile(
      "project-1",
      "folder-1",
      {
        relativePath: "Planning/tracked.md",
        displayName: "tracked.md",
        isDirectory: false,
      },
      queue,
    );

    expect(received).toEqual([
      { kind: "externalPath", absolutePath: "C:\\Inbox\\picked.md" },
      { kind: "externalPath", absolutePath: "C:\\Inbox\\dropped.md" },
      {
        kind: "trackedFile",
        trackedFolderId: "folder-1",
        relativePath: "Planning/tracked.md",
      },
    ]);
    expect([
      picked.imported[0]?.navigation,
      dropped.imported[0]?.navigation,
      tracked.navigation,
    ]).toEqual([
      { projectId: "project-1", documentId: "document-1", editorMode: "visual" },
      { projectId: "project-1", documentId: "document-2", editorMode: "visual" },
      { projectId: "project-1", documentId: "document-3", editorMode: "visual" },
    ]);
  });

  it("treats a cancelled picker as a no-op", async () => {
    const importer: Importer = vi.fn();
    vi.mocked(open).mockResolvedValue(null);

    await expect(
      importChosenMarkdownFiles("project-1", new ImportQueue(importer)),
    ).resolves.toEqual({ imported: [], failures: [] });
    expect(importer).not.toHaveBeenCalled();
  });

  it("waits for a whole batch and reports successes beside individual failures", async () => {
    const importer: Importer = vi.fn(async (_projectId, source) => {
      const path = source.kind === "externalPath" ? source.absolutePath : source.relativePath;
      if (path.endsWith("bad.md")) throw new Error("The file is not readable");
      return document(path.endsWith("last.md") ? "last" : "first");
    });

    const result = await importDroppedFiles(
      "project-1",
      ["first.md", "bad.md", "last.md"],
      new ImportQueue(importer),
    );

    expect(result.imported.map(({ document: item }) => item.id)).toEqual([
      "first",
      "last",
    ]);
    expect(result.failures).toEqual([
      { absolutePath: "bad.md", message: "The file is not readable" },
    ]);
    expect(importer).toHaveBeenCalledTimes(3);
  });

  it("serializes imports and continues after an individual failure", async () => {
    const order: string[] = [];
    const importer: Importer = vi.fn(async (_projectId, source) => {
      const path = source.kind === "externalPath" ? source.absolutePath : source.relativePath;
      order.push(`start:${path}`);
      await Promise.resolve();
      order.push(`finish:${path}`);
      if (path.endsWith("bad.md")) {
        throw new Error("rejected");
      }
      return document(path.endsWith("last.md") ? "last" : "first");
    });
    const queue = new ImportQueue(importer);
    const first = queue.enqueue("project-1", {
      kind: "externalPath",
      absolutePath: "first.md",
    });
    const failed = queue.enqueue("project-1", {
      kind: "externalPath",
      absolutePath: "bad.md",
    });
    const last = queue.enqueue("project-1", {
      kind: "externalPath",
      absolutePath: "last.md",
    });

    await expect(first).resolves.toMatchObject({ document: { id: "first" } });
    await expect(failed).rejects.toThrow("rejected");
    await expect(last).resolves.toMatchObject({ document: { id: "last" } });
    expect(order).toEqual([
      "start:first.md",
      "finish:first.md",
      "start:bad.md",
      "finish:bad.md",
      "start:last.md",
      "finish:last.md",
    ]);
  });

  it("forwards only native drop events and exposes the unlisten function", async () => {
    const unlisten = vi.fn();
    let handler: ((event: { payload: { type: string; paths?: string[] } }) => void) | undefined;
    vi.mocked(getCurrentWebview).mockReturnValue({
      onDragDropEvent: vi.fn(async (value) => {
        handler = value as typeof handler;
        return unlisten;
      }),
    } as never);
    const onPaths = vi.fn();

    const stop = await listenForDroppedFiles(onPaths);
    handler?.({ payload: { type: "over" } });
    handler?.({ payload: { type: "drop", paths: ["C:\\Inbox\\note.md"] } });
    stop();

    expect(onPaths).toHaveBeenCalledOnce();
    expect(onPaths).toHaveBeenCalledWith(["C:\\Inbox\\note.md"]);
    expect(unlisten).toHaveBeenCalledOnce();
  });
});
