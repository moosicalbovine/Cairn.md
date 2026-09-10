import { invoke } from "@tauri-apps/api/core";

import { parseDocumentSnapshot, type DocumentSnapshot } from "./library";

export type ExternalImportSource = Readonly<{
  kind: "externalPath";
  absolutePath: string;
}>;

export type TrackedFileImportSource = Readonly<{
  kind: "trackedFile";
  trackedFolderId: string;
  relativePath: string;
}>;

export type ImportSource = ExternalImportSource | TrackedFileImportSource;

export type TrackedFolderSnapshot = Readonly<{
  id: string;
  absolutePath: string;
  displayName: string;
  available: boolean;
  lastScanAt: number | null;
}>;

export type TrackedFolderEntry = Readonly<{
  relativePath: string;
  displayName: string;
  isDirectory: boolean;
}>;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isNullableTimestamp(value: unknown): value is number | null {
  return value === null || (Number.isSafeInteger(value) && (value as number) >= 0);
}

function isSafeRelativePath(value: unknown): value is string {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.startsWith("/") ||
    value.includes("\\") ||
    /^[A-Za-z]:/.test(value)
  ) {
    return false;
  }
  return value
    .split("/")
    .every((part) => part.length > 0 && part !== "." && part !== "..");
}

export function parseTrackedFolderSnapshot(value: unknown): TrackedFolderSnapshot {
  if (
    !isRecord(value) ||
    typeof value.id !== "string" ||
    typeof value.absolutePath !== "string" ||
    typeof value.displayName !== "string" ||
    typeof value.available !== "boolean" ||
    !isNullableTimestamp(value.lastScanAt)
  ) {
    throw new Error("Invalid tracked folder snapshot");
  }
  return {
    id: value.id,
    absolutePath: value.absolutePath,
    displayName: value.displayName,
    available: value.available,
    lastScanAt: value.lastScanAt,
  };
}

export function parseTrackedFolderEntry(value: unknown): TrackedFolderEntry {
  if (
    !isRecord(value) ||
    !isSafeRelativePath(value.relativePath) ||
    typeof value.displayName !== "string" ||
    typeof value.isDirectory !== "boolean"
  ) {
    throw new Error("Invalid tracked folder entry");
  }
  return {
    relativePath: value.relativePath,
    displayName: value.displayName,
    isDirectory: value.isDirectory,
  };
}

export async function importMarkdown(
  projectId: string,
  source: ImportSource,
): Promise<DocumentSnapshot> {
  return parseDocumentSnapshot(
    await invoke<unknown>("import_markdown", { projectId, source }),
  );
}

export async function addTrackedFolder(path: string): Promise<TrackedFolderSnapshot> {
  return parseTrackedFolderSnapshot(
    await invoke<unknown>("add_tracked_folder", { path }),
  );
}

export async function listTrackedFolders(): Promise<readonly TrackedFolderSnapshot[]> {
  const value = await invoke<unknown>("list_tracked_folders");
  if (!Array.isArray(value)) {
    throw new Error("Invalid tracked folder list");
  }
  return value.map(parseTrackedFolderSnapshot);
}

export async function removeTrackedFolder(id: string): Promise<void> {
  await invoke("remove_tracked_folder", { id });
}

export async function listTrackedFolderEntries(
  folderId: string,
  relativeDirectory: string | null = null,
): Promise<readonly TrackedFolderEntry[]> {
  const value = await invoke<unknown>("list_tracked_folder_entries", {
    folderId,
    relativeDirectory,
  });
  if (!Array.isArray(value)) {
    throw new Error("Invalid tracked folder entry list");
  }
  return value.map(parseTrackedFolderEntry);
}
