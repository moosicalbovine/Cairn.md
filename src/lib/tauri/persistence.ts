import { invoke } from "@tauri-apps/api/core";

export type RecoveryLifecycle = "Draft" | "Saving" | "Recovered" | "Conflict";

export type RecoverySnapshotRequest = Readonly<{
  documentId: string;
  sessionGeneration: string;
  revision: number;
  bytes: Uint8Array;
  baseFingerprint: string;
}>;

export type RecoverySnapshot = Readonly<{
  documentId: string;
  sessionGeneration: string;
  revision: number;
  bytes: Uint8Array;
  contentHash: string;
  baseFingerprint: string;
  intendedDiskHash: string;
  operationId: string | null;
  lifecycleState: RecoveryLifecycle;
  durableAt: number;
}>;

export type SaveDocumentResult = Readonly<{
  status: "saved" | "conflict";
  revision: number;
  diskFingerprint: string;
}>;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isByteArray(value: unknown): value is number[] {
  return (
    Array.isArray(value) &&
    value.every(
      (byte) => Number.isSafeInteger(byte) && byte >= 0 && byte <= 255,
    )
  );
}

function isLifecycle(value: unknown): value is RecoveryLifecycle {
  return ["Draft", "Saving", "Recovered", "Conflict"].includes(
    value as RecoveryLifecycle,
  );
}

export function parseRecoverySnapshot(value: unknown): RecoverySnapshot {
  if (
    !isRecord(value) ||
    typeof value.documentId !== "string" ||
    typeof value.sessionGeneration !== "string" ||
    !Number.isSafeInteger(value.revision) ||
    (value.revision as number) <= 0 ||
    !isByteArray(value.bytes) ||
    typeof value.contentHash !== "string" ||
    typeof value.baseFingerprint !== "string" ||
    typeof value.intendedDiskHash !== "string" ||
    !(typeof value.operationId === "string" || value.operationId === null) ||
    !isLifecycle(value.lifecycleState) ||
    !Number.isSafeInteger(value.durableAt)
  ) {
    throw new Error("Invalid recovery snapshot");
  }

  return {
    documentId: value.documentId,
    sessionGeneration: value.sessionGeneration,
    revision: value.revision as number,
    bytes: Uint8Array.from(value.bytes),
    contentHash: value.contentHash,
    baseFingerprint: value.baseFingerprint,
    intendedDiskHash: value.intendedDiskHash,
    operationId: value.operationId,
    lifecycleState: value.lifecycleState,
    durableAt: value.durableAt as number,
  };
}

export function parseSaveDocumentResult(value: unknown): SaveDocumentResult {
  if (
    !isRecord(value) ||
    (value.status !== "saved" && value.status !== "conflict") ||
    !Number.isSafeInteger(value.revision) ||
    (value.revision as number) <= 0 ||
    typeof value.diskFingerprint !== "string"
  ) {
    throw new Error("Invalid save result");
  }
  return {
    status: value.status,
    revision: value.revision as number,
    diskFingerprint: value.diskFingerprint,
  };
}

export async function storeRecoverySnapshot(
  request: RecoverySnapshotRequest,
): Promise<RecoverySnapshot> {
  return parseRecoverySnapshot(
    await invoke<unknown>("store_recovery_snapshot", {
      request: { ...request, bytes: Array.from(request.bytes) },
    }),
  );
}

export async function loadRecoverySnapshot(
  documentId: string,
): Promise<RecoverySnapshot | null> {
  const value = await invoke<unknown>("load_recovery_snapshot", { documentId });
  return value === null ? null : parseRecoverySnapshot(value);
}

export function discardRecoverySnapshot(
  documentId: string,
  sessionGeneration: string,
): Promise<boolean> {
  return invoke<boolean>("discard_recovery_snapshot", {
    documentId,
    sessionGeneration,
  });
}

export async function saveDocument(
  request: RecoverySnapshotRequest,
): Promise<SaveDocumentResult> {
  return parseSaveDocumentResult(
    await invoke<unknown>("save_document", {
      request: { ...request, bytes: Array.from(request.bytes) },
    }),
  );
}
