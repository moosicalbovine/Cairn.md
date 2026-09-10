import { invoke } from "@tauri-apps/api/core";

export type LibraryMode = "writable" | "readOnly";

export type LibraryBinding = Readonly<{
  libraryId: string;
  rootPath: string;
  rootIdentity: string;
  generation: number;
}>;

export type DocumentSnapshot = Readonly<{
  id: string;
  relativePath: string;
  sourcePath: string | null;
  importedAt: number | null;
  diskFingerprint: string;
}>;

export type ProjectSnapshot = Readonly<{
  id: string;
  relativePath: string;
  documents: readonly DocumentSnapshot[];
}>;

export type LibrarySnapshot = Readonly<{
  mode: LibraryMode;
  readOnlyReason: string | null;
  binding: LibraryBinding | null;
  projects: readonly ProjectSnapshot[];
}>;

export type CandidateRootProbe = Readonly<{
  candidatePath: string;
  rootIdentity: string | null;
  exists: boolean;
  isDirectory: boolean;
  canCreate: boolean;
  canFlush: boolean;
  canRenameWithoutOverwrite: boolean;
  recoverableDelete: boolean;
  canBind: boolean;
  reason: string | null;
}>;

export type RelinkPreview = Readonly<{
  candidatePath: string;
  libraryId: string;
  expectedGeneration: number;
  expectedStateToken: string;
  rootIdentity: string;
  matchedProjects: number;
  matchedDocuments: number;
}>;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isStringOrNull(value: unknown): value is string | null {
  return typeof value === "string" || value === null;
}

function isRelativePath(value: unknown, expectedParts: number): value is string {
  if (typeof value !== "string" || value.length === 0 || /^[\\/]|^[A-Za-z]:/.test(value)) {
    return false;
  }
  const parts = value.split("/");
  return (
    parts.length === expectedParts &&
    parts.every((part) => part.length > 0 && part !== "." && part !== ".." && !part.includes("\\"))
  );
}

function parseDocument(value: unknown): DocumentSnapshot {
  if (
    !isRecord(value) ||
    typeof value.id !== "string" ||
    !isRelativePath(value.relativePath, 2) ||
    !isStringOrNull(value.sourcePath) ||
    !(typeof value.importedAt === "number" || value.importedAt === null) ||
    typeof value.diskFingerprint !== "string"
  ) {
    throw new Error("Invalid library snapshot");
  }
  return {
    id: value.id,
    relativePath: value.relativePath,
    sourcePath: value.sourcePath,
    importedAt: value.importedAt,
    diskFingerprint: value.diskFingerprint,
  };
}

function parseProject(value: unknown): ProjectSnapshot {
  if (
    !isRecord(value) ||
    typeof value.id !== "string" ||
    !isRelativePath(value.relativePath, 1) ||
    !Array.isArray(value.documents)
  ) {
    throw new Error("Invalid library snapshot");
  }
  return {
    id: value.id,
    relativePath: value.relativePath,
    documents: value.documents.map(parseDocument),
  };
}

export function parseLibrarySnapshot(value: unknown): LibrarySnapshot {
  if (
    !isRecord(value) ||
    (value.mode !== "writable" && value.mode !== "readOnly") ||
    !isStringOrNull(value.readOnlyReason) ||
    !Array.isArray(value.projects)
  ) {
    throw new Error("Invalid library snapshot");
  }
  if (value.binding !== null) {
    const binding = value.binding;
    if (
      !isRecord(binding) ||
      typeof binding.libraryId !== "string" ||
      typeof binding.rootPath !== "string" ||
      typeof binding.rootIdentity !== "string" ||
      !Number.isSafeInteger(binding.generation) ||
      (binding.generation as number) < 1
    ) {
      throw new Error("Invalid library snapshot");
    }
  }
  return {
    mode: value.mode,
    readOnlyReason: value.readOnlyReason,
    binding: value.binding as LibraryBinding | null,
    projects: value.projects.map(parseProject),
  };
}

export async function loadLibraryIndex(
  renderSnapshot: (snapshot: LibrarySnapshot) => void,
): Promise<LibrarySnapshot> {
  const cached = parseLibrarySnapshot(await invoke<unknown>("library_snapshot"));
  renderSnapshot(cached);
  const reconciled = parseLibrarySnapshot(await invoke<unknown>("reconcile_library"));
  renderSnapshot(reconciled);
  return reconciled;
}

export function watchLibraryReconciliation(
  onSnapshot: (snapshot: LibrarySnapshot) => void,
  onError: (error: unknown) => void = () => undefined,
  intervalMs = 300,
): () => void {
  let stopped = false;
  let running = false;

  const poll = async () => {
    if (stopped || running) {
      return;
    }
    running = true;
    try {
      const value = await invoke<unknown>("reconcile_library_if_requested");
      if (value !== null) {
        onSnapshot(parseLibrarySnapshot(value));
      }
    } catch (error) {
      onError(error);
    } finally {
      running = false;
    }
  };

  const timer = globalThis.setInterval(() => void poll(), intervalMs);
  return () => {
    stopped = true;
    globalThis.clearInterval(timer);
  };
}

export async function createProject(name: string): Promise<ProjectSnapshot> {
  return parseProject(await invoke<unknown>("create_project", { name }));
}

export async function probeLibraryRoot(candidatePath: string): Promise<CandidateRootProbe> {
  return invoke<CandidateRootProbe>("probe_library_root", { candidatePath });
}

export async function bindLibraryRoot(rootPath: string): Promise<LibrarySnapshot> {
  return parseLibrarySnapshot(await invoke<unknown>("bind_library_root", { rootPath }));
}

export async function previewLibraryRelink(candidatePath: string): Promise<RelinkPreview> {
  return invoke<RelinkPreview>("preview_library_relink", { candidatePath });
}

export async function confirmLibraryRelink(preview: RelinkPreview): Promise<LibrarySnapshot> {
  return parseLibrarySnapshot(await invoke<unknown>("confirm_library_relink", { preview }));
}
