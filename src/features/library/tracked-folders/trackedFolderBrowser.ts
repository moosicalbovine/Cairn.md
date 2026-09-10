import {
  addTrackedFolder,
  listTrackedFolderEntries,
  listTrackedFolders,
  removeTrackedFolder,
  type TrackedFolderEntry,
  type TrackedFolderSnapshot,
} from "../../../lib/tauri/import";
import { ImportQueue, type ImportSuccess } from "../import/ImportQueue";
import { chooseTrackedFolder } from "../import/importAdapters";

export async function addChosenTrackedFolder(): Promise<TrackedFolderSnapshot | null> {
  const selected = await chooseTrackedFolder();
  return selected === null ? null : addTrackedFolder(selected);
}

export function loadTrackedFolders(): Promise<readonly TrackedFolderSnapshot[]> {
  return listTrackedFolders();
}

export function browseTrackedFolder(
  folderId: string,
  relativeDirectory: string | null = null,
): Promise<readonly TrackedFolderEntry[]> {
  return listTrackedFolderEntries(folderId, relativeDirectory);
}

export function stopTrackingFolder(folderId: string): Promise<void> {
  return removeTrackedFolder(folderId);
}

export function importTrackedFile(
  projectId: string,
  trackedFolderId: string,
  entry: TrackedFolderEntry,
  queue: ImportQueue,
): Promise<ImportSuccess> {
  if (entry.isDirectory) {
    return Promise.reject(new Error("A tracked directory cannot be imported as a document"));
  }
  return queue.enqueue(projectId, {
    kind: "trackedFile",
    trackedFolderId,
    relativePath: entry.relativePath,
  });
}
