// @vitest-environment node
import { afterEach, describe, expect, it, vi } from "vitest";

import type {
  RecoverySnapshot,
  RecoverySnapshotRequest,
  SaveDocumentResult,
} from "../../../lib/tauri/persistence";
import { MarkdownSession } from "../session/MarkdownSession";
import { AutosaveController, type PersistenceProgress } from "./AutosaveController";

function snapshotOf(request: RecoverySnapshotRequest): RecoverySnapshot {
  return {
    ...request,
    contentHash: `hash-${request.revision}`,
    intendedDiskHash: `hash-${request.revision}`,
    operationId: null,
    lifecycleState: "Draft",
    durableAt: request.revision,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((complete) => {
    resolve = complete;
  });
  return { promise, resolve };
}

afterEach(() => vi.useRealTimers());

describe("AutosaveController", () => {
  it("stores recovery before saving and reports only the approved labels", async () => {
    vi.useFakeTimers();
    const order: string[] = [];
    const progress: PersistenceProgress[] = [];
    const session = MarkdownSession.fromSource("");
    const controller = new AutosaveController({
      documentId: "document-1",
      session,
      baseFingerprint: "base-0",
      generation: "generation-1",
      onProgress: (value) => progress.push(value),
      port: {
        storeRecoverySnapshot: vi.fn(async (request) => {
          order.push(`snapshot-${request.revision}`);
          return snapshotOf(request);
        }),
        saveDocument: vi.fn(async (request): Promise<SaveDocumentResult> => {
          order.push(`save-${request.revision}`);
          return {
            status: "saved",
            revision: request.revision,
            diskFingerprint: `disk-${request.revision}`,
          };
        }),
      },
    });

    session.replaceSource("draft", 0);
    await vi.runAllTimersAsync();

    expect(order).toEqual(["snapshot-1", "save-1"]);
    expect(progress.at(-1)).toEqual({
      label: "Saved",
      currentRevision: 1,
      durableSnapshotRevision: 0,
      diskRevision: 1,
    });
    expect(new Set(progress.map((value) => value.label))).toEqual(
      new Set(["Saved", "Saving…"]),
    );
    await controller.dispose();
  });

  it("cannot let an older completed write mark a newer edit saved", async () => {
    vi.useFakeTimers();
    const firstSave = deferred<SaveDocumentResult>();
    const progress: PersistenceProgress[] = [];
    const saveDocument = vi
      .fn<(request: RecoverySnapshotRequest) => Promise<SaveDocumentResult>>()
      .mockImplementationOnce(() => firstSave.promise)
      .mockImplementation(async (request) => ({
        status: "saved",
        revision: request.revision,
        diskFingerprint: `disk-${request.revision}`,
      }));
    const session = MarkdownSession.fromSource("");
    const controller = new AutosaveController({
      documentId: "document-1",
      session,
      baseFingerprint: "base-0",
      generation: "generation-1",
      onProgress: (value) => progress.push(value),
      port: {
        storeRecoverySnapshot: async (request) => snapshotOf(request),
        saveDocument,
      },
    });

    session.replaceSource("one", 0);
    await vi.runAllTimersAsync();
    expect(saveDocument).toHaveBeenCalledTimes(1);

    session.replaceSource("two", 1);
    await vi.advanceTimersByTimeAsync(350);
    firstSave.resolve({ status: "saved", revision: 1, diskFingerprint: "disk-1" });
    await Promise.resolve();
    await vi.runAllTimersAsync();

    expect(saveDocument).toHaveBeenCalledTimes(2);
    expect(progress.at(-1)?.currentRevision).toBe(2);
    expect(progress.at(-1)?.diskRevision).toBe(2);
    expect(progress.at(-1)?.label).toBe("Saved");
    await controller.dispose();
  });

  it("keeps recovery after failure and retries explicitly", async () => {
    vi.useFakeTimers();
    const labels: string[] = [];
    const saveDocument = vi
      .fn<(request: RecoverySnapshotRequest) => Promise<SaveDocumentResult>>()
      .mockRejectedValueOnce(new Error("disk denied"))
      .mockImplementation(async (request) => ({
        status: "saved",
        revision: request.revision,
        diskFingerprint: "disk-retry",
      }));
    const session = MarkdownSession.fromSource("");
    const controller = new AutosaveController({
      documentId: "document-1",
      session,
      baseFingerprint: "base-0",
      generation: "generation-1",
      onProgress: (value) => labels.push(value.label),
      port: {
        storeRecoverySnapshot: async (request) => snapshotOf(request),
        saveDocument,
      },
    });

    session.replaceSource("draft", 0);
    await vi.runAllTimersAsync();
    expect(labels.at(-1)).toBe("Save failed");

    controller.retry();
    await vi.runAllTimersAsync();
    expect(labels.at(-1)).toBe("Saved");
    expect(saveDocument).toHaveBeenCalledTimes(2);
    await controller.dispose();
  });

  it("flushes acknowledged text to recovery before disposal", async () => {
    vi.useFakeTimers();
    const storeRecoverySnapshot = vi.fn(async (request) => snapshotOf(request));
    const session = MarkdownSession.fromSource("");
    const controller = new AutosaveController({
      documentId: "document-1",
      session,
      baseFingerprint: "base-0",
      generation: "generation-1",
      onProgress: vi.fn(),
      port: {
        storeRecoverySnapshot,
        saveDocument: vi.fn(),
      },
    });

    session.replaceSource("close safely", 0);
    await controller.dispose();

    expect(storeRecoverySnapshot).toHaveBeenCalledWith(
      expect.objectContaining({ revision: 1 }),
    );
  });
});
