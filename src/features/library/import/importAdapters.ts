import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";

import type { ImportSource } from "../../../lib/tauri/import";
import { ImportQueue, type ImportSuccess } from "./ImportQueue";

export type ImportFailure = Readonly<{
  absolutePath: string;
  message: string;
}>;

export type ImportBatchResult = Readonly<{
  imported: readonly ImportSuccess[];
  failures: readonly ImportFailure[];
}>;

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
): Promise<ImportBatchResult> {
  const selected = await chooseMarkdownFiles();
  return importFileBatch(projectId, selected, queue);
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
): Promise<ImportBatchResult> {
  return importFileBatch(projectId, paths, queue);
}

async function importFileBatch(
  projectId: string,
  paths: readonly string[],
  queue: ImportQueue,
): Promise<ImportBatchResult> {
  const settled = await Promise.allSettled(
    paths.map((absolutePath) =>
      queue.enqueue(projectId, externalFileSource(absolutePath)),
    ),
  );
  const imported: ImportSuccess[] = [];
  const failures: ImportFailure[] = [];
  settled.forEach((result, index) => {
    if (result.status === "fulfilled") {
      imported.push(result.value);
      return;
    }
    const absolutePath = paths[index];
    if (absolutePath === undefined) return;
    failures.push({
      absolutePath,
      message:
        result.reason instanceof Error
          ? result.reason.message
          : "The Markdown file could not be imported.",
    });
  });
  return { imported, failures };
}
