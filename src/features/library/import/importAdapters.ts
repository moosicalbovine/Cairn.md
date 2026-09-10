import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";

import type { ImportSource } from "../../../lib/tauri/import";
import { ImportQueue, type ImportSuccess } from "./ImportQueue";

export function externalFileSource(absolutePath: string): ImportSource {
  return { kind: "externalPath", absolutePath };
}

export async function chooseMarkdownFiles(): Promise<readonly string[]> {
  const selection = await open({
    title: "Import Markdown files",
    directory: false,
    multiple: true,
    filters: [{ name: "Markdown", extensions: ["md"] }],
  });
  return selection ?? [];
}

export async function chooseTrackedFolder(): Promise<string | null> {
  return open({
    title: "Add a tracked PC folder",
    directory: true,
    multiple: false,
  });
}

export async function importChosenMarkdownFiles(
  projectId: string,
  queue: ImportQueue,
): Promise<readonly ImportSuccess[]> {
  const selected = await chooseMarkdownFiles();
  return Promise.all(
    selected.map((absolutePath) => queue.enqueue(projectId, externalFileSource(absolutePath))),
  );
}

export async function listenForDroppedFiles(
  onPaths: (paths: readonly string[]) => void,
): Promise<() => void> {
  return getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type === "drop") {
      onPaths(event.payload.paths);
    }
  });
}

export function importDroppedFiles(
  projectId: string,
  paths: readonly string[],
  queue: ImportQueue,
): Promise<readonly ImportSuccess[]> {
  return Promise.all(
    paths.map((absolutePath) => queue.enqueue(projectId, externalFileSource(absolutePath))),
  );
}
