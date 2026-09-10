import type { MarkdownSession } from "../session/MarkdownSession";
import {
  saveDocument,
  storeRecoverySnapshot,
  type RecoverySnapshot,
  type RecoverySnapshotRequest,
  type SaveDocumentResult,
} from "../../../lib/tauri/persistence";

export type PersistenceLabel = "Saving…" | "Saved" | "Save failed" | "Recovered";

export type PersistenceProgress = Readonly<{
  label: PersistenceLabel;
  currentRevision: number;
  durableSnapshotRevision: number;
  diskRevision: number;
}>;

type PersistencePort = Readonly<{
  storeRecoverySnapshot(request: RecoverySnapshotRequest): Promise<RecoverySnapshot>;
  saveDocument(request: RecoverySnapshotRequest): Promise<SaveDocumentResult>;
}>;

type AutosaveOptions = Readonly<{
  documentId: string;
  session: MarkdownSession;
  baseFingerprint: string;
  onProgress(progress: PersistenceProgress): void;
  generation?: string;
  delayMs?: number;
  port?: PersistencePort;
}>;

const defaultPort: PersistencePort = { storeRecoverySnapshot, saveDocument };

export class AutosaveController {
  readonly #documentId: string;
  readonly #session: MarkdownSession;
  readonly #generation: string;
  readonly #delayMs: number;
  readonly #port: PersistencePort;
  readonly #onProgress: (progress: PersistenceProgress) => void;
  readonly #unsubscribe: () => void;

  #baseFingerprint: string;
  #durableSnapshot: RecoverySnapshot | null = null;
  #diskRevision = 0;
  #failed = false;
  #disposed = false;
  #snapshotTimer: ReturnType<typeof setTimeout> | null = null;
  #saveTimer: ReturnType<typeof setTimeout> | null = null;
  #snapshotRun: Promise<void> | null = null;
  #saveRun: Promise<void> | null = null;

  constructor(options: AutosaveOptions) {
    this.#documentId = options.documentId;
    this.#session = options.session;
    this.#generation = options.generation ?? globalThis.crypto.randomUUID();
    this.#baseFingerprint = options.baseFingerprint;
    this.#delayMs = options.delayMs ?? 350;
    this.#port = options.port ?? defaultPort;
    this.#onProgress = options.onProgress;
    this.#unsubscribe = this.#session.subscribe(() => this.#acknowledgeEdit());
    this.#emit("Saved");
  }

  get progress(): PersistenceProgress {
    return this.#progress(this.#label());
  }

  retry(): void {
    if (!this.#failed || this.#disposed) return;
    this.#failed = false;
    this.#emit("Saving…");
    this.#scheduleSave(0);
  }

  async dispose(): Promise<void> {
    if (this.#disposed) return;
    this.#disposed = true;
    this.#unsubscribe();
    this.#clearTimers();
    await this.#flushRecovery();
  }

  #acknowledgeEdit(): void {
    if (this.#disposed) return;
    this.#emit(this.#failed ? "Save failed" : "Saving…");
    this.#scheduleSnapshot();
    if (!this.#failed) this.#scheduleSave(this.#delayMs);
  }

  #scheduleSnapshot(): void {
    if (this.#snapshotTimer !== null) clearTimeout(this.#snapshotTimer);
    this.#snapshotTimer = setTimeout(() => {
      this.#snapshotTimer = null;
      void this.#flushRecovery().catch(() => {
        this.#failed = true;
        this.#emit("Save failed");
      });
    }, Math.min(this.#delayMs, 750));
  }

  #scheduleSave(delayMs: number): void {
    if (this.#saveTimer !== null) clearTimeout(this.#saveTimer);
    this.#saveTimer = setTimeout(() => {
      this.#saveTimer = null;
      void this.#flushSave();
    }, delayMs);
  }

  async #flushRecovery(): Promise<void> {
    if (this.#snapshotRun) {
      await this.#snapshotRun;
    }
    const revision = this.#session.revision;
    const durable = this.#durableSnapshot;
    if (
      revision === 0 ||
      (revision <= (durable?.revision ?? 0) &&
        durable?.baseFingerprint === this.#baseFingerprint)
    ) {
      return;
    }

    const request = this.#request(revision);
    const run = this.#port.storeRecoverySnapshot(request).then((snapshot) => {
      if (
        !this.#durableSnapshot ||
        snapshot.revision >= this.#durableSnapshot.revision
      ) {
        this.#durableSnapshot = snapshot;
      }
      this.#emit(this.#failed ? "Save failed" : "Saving…");
    });
    this.#snapshotRun = run;
    try {
      await run;
    } finally {
      if (this.#snapshotRun === run) this.#snapshotRun = null;
    }

    if (
      this.#session.revision > (this.#durableSnapshot?.revision ?? 0) ||
      this.#durableSnapshot?.baseFingerprint !== this.#baseFingerprint
    ) {
      await this.#flushRecovery();
    }
  }

  async #flushSave(): Promise<void> {
    if (this.#failed || this.#disposed || this.#saveRun) return;
    const run = (async () => {
      try {
        await this.#flushRecovery();
        const durable = this.#durableSnapshot;
        if (!durable || durable.revision <= this.#diskRevision) return;
        const result = await this.#port.saveDocument({
          documentId: durable.documentId,
          sessionGeneration: durable.sessionGeneration,
          revision: durable.revision,
          bytes: durable.bytes,
          baseFingerprint: durable.baseFingerprint,
        });
        if (result.status === "conflict") {
          this.#failed = true;
          this.#emit("Save failed");
          return;
        }
        if (result.revision >= this.#diskRevision) {
          this.#diskRevision = result.revision;
          this.#baseFingerprint = result.diskFingerprint;
        }
        if (this.#diskRevision === this.#session.revision) {
          this.#durableSnapshot = null;
          this.#emit("Saved");
        } else {
          this.#durableSnapshot = null;
          this.#emit("Saving…");
          await this.#flushRecovery();
          this.#scheduleSave(0);
        }
      } catch {
        this.#failed = true;
        this.#emit("Save failed");
      }
    })();
    this.#saveRun = run;
    try {
      await run;
    } finally {
      if (this.#saveRun === run) this.#saveRun = null;
    }
  }

  #request(revision: number): RecoverySnapshotRequest {
    return {
      documentId: this.#documentId,
      sessionGeneration: this.#generation,
      revision,
      bytes: this.#session.toBytes(),
      baseFingerprint: this.#baseFingerprint,
    };
  }

  #label(): PersistenceLabel {
    if (this.#failed) return "Save failed";
    return this.#diskRevision === this.#session.revision ? "Saved" : "Saving…";
  }

  #emit(label: PersistenceLabel): void {
    if (this.#disposed) return;
    this.#onProgress(this.#progress(label));
  }

  #progress(label: PersistenceLabel): PersistenceProgress {
    return {
      label,
      currentRevision: this.#session.revision,
      durableSnapshotRevision: this.#durableSnapshot?.revision ?? 0,
      diskRevision: this.#diskRevision,
    };
  }

  #clearTimers(): void {
    if (this.#snapshotTimer !== null) clearTimeout(this.#snapshotTimer);
    if (this.#saveTimer !== null) clearTimeout(this.#saveTimer);
    this.#snapshotTimer = null;
    this.#saveTimer = null;
  }
}
