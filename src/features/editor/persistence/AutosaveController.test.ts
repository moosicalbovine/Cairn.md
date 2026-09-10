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

  it("stops and reports when protected save detects an external conflict", async () => {
    vi.useFakeTimers();
    const onConflict = vi.fn();
    const labels: string[] = [];
    const session = MarkdownSession.fromSource("");
    const controller = new AutosaveController({
      documentId: "document-1",
      session,
      baseFingerprint: "base-0",
      generation: "generation-1",
      onProgress: (value) => labels.push(value.label),
      onConflict,
      port: {
        storeRecoverySnapshot: async (request) => snapshotOf(request),
        saveDocument: async (request) => ({
          status: "conflict",
          revision: request.revision,
          diskFingerprint: "external-hash",
        }),
      },
    });

    session.replaceSource("local", 0);
    await vi.runAllTimersAsync();

    expect(labels.at(-1)).toBe("Save failed");
    expect(onConflict).toHaveBeenCalledOnce();
    await controller.dispose();
  });

  it("preserves typing that races with an external conflict before closing the session", async () => {
    vi.useFakeTimers();
    const save = deferred<SaveDocumentResult>();
    let conflicted = false;
    const storeRecoverySnapshot = vi.fn(async (request: RecoverySnapshotRequest) => ({
      ...snapshotOf(request),
      lifecycleState: conflicted ? "Conflict" as const : "Draft" as const,
    }));
    const onConflict = vi.fn();
    const session = MarkdownSession.fromSource("");
    const controller = new AutosaveController({
      documentId: "document-1",
      session,
      baseFingerprint: "base-0",
      generation: "generation-1",
      onProgress: vi.fn(),
      onConflict,
      port: {
        storeRecoverySnapshot,
        saveDocument: () => save.promise,
      },
    });

    session.replaceSource("first draft", 0);
    await vi.advanceTimersByTimeAsync(350);
    session.replaceSource("newest local draft", 1);
    conflicted = true;
    save.resolve({
      status: "conflict",
      revision: 1,
      diskFingerprint: "external-hash",
    });
    await vi.runAllTimersAsync();

    expect(onConflict).toHaveBeenCalledWith(
      expect.objectContaining({
        revision: 2,
        bytes: new TextEncoder().encode("newest local draft"),
        lifecycleState: "Conflict",
      }),
    );
    expect(storeRecoverySnapshot.mock.calls.at(-1)?.[0].revision).toBe(2);
    expect(() => session.replaceSource("too late", 2)).toThrow(/closed/);
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

  it("advances durable recovery on a bounded cadence during continuous typing", async () => {
    vi.useFakeTimers();
    const durableRevisions: number[] = [];
    const session = MarkdownSession.fromSource("");
    const controller = new AutosaveController({
      documentId: "document-1",
      session,
      baseFingerprint: "base-0",
      generation: "generation-1",
      onProgress: vi.fn(),
      port: {
        storeRecoverySnapshot: async (request) => {
          durableRevisions.push(request.revision);
          return snapshotOf(request);
        },
        saveDocument: vi.fn(),
      },
    });

    for (let revision = 0; revision < 20; revision += 1) {
      session.replaceSource(`continuous ${revision + 1}`, revision);
      await vi.advanceTimersByTimeAsync(100);
    }

    expect(durableRevisions.length).toBeGreaterThanOrEqual(4);
    expect(durableRevisions[0]).toBeLessThanOrEqual(4);
    expect(durableRevisions.at(-1)).toBeGreaterThanOrEqual(16);
    await controller.dispose();
    expect(durableRevisions.at(-1)).toBe(20);
  });

  it("serializes concurrent recovery flush requests", async () => {
    vi.useFakeTimers();
    const firstStore = deferred<RecoverySnapshot>();
    let calls = 0;
    let inFlight = 0;
    let maximumInFlight = 0;
    const storeRecoverySnapshot = vi.fn(async (request: RecoverySnapshotRequest) => {
      calls += 1;
      inFlight += 1;
      maximumInFlight = Math.max(maximumInFlight, inFlight);
      const snapshot = calls === 1
        ? await firstStore.promise
        : snapshotOf(request);
      inFlight -= 1;
      return snapshot;
    });
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

    session.replaceSource("one", 0);
    await vi.advanceTimersByTimeAsync(350);
    session.replaceSource("two", 1);
    await vi.advanceTimersByTimeAsync(350);
    session.replaceSource("three", 2);
    await vi.advanceTimersByTimeAsync(350);

    expect(storeRecoverySnapshot).toHaveBeenCalledTimes(1);
    const firstRequest = storeRecoverySnapshot.mock.calls[0]?.[0];
    if (!firstRequest) throw new Error("The first recovery write did not start.");
    firstStore.resolve(snapshotOf(firstRequest));
    await controller.dispose();

    expect(maximumInFlight).toBe(1);
    expect(storeRecoverySnapshot).toHaveBeenCalledTimes(2);
    expect(storeRecoverySnapshot.mock.calls[1]?.[0]).toEqual(
      expect.objectContaining({ revision: 3 }),
    );
  });
});
