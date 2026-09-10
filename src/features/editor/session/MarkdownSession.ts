import {
  decodeMarkdownBytes,
  encodeMarkdownSource,
  type DecodedMarkdown,
  type MarkdownLineEnding,
} from "../markdown/documentFormat";

export interface MarkdownPatch {
  readonly from: number;
  readonly to: number;
  readonly replacement: string;
  readonly revision: number;
}

export interface MarkdownSessionChange {
  readonly source: string;
  readonly revision: number;
}

type MarkdownSessionListener = (change: MarkdownSessionChange) => void;

export class MarkdownSession {
  readonly isReadOnly: boolean;
  readonly hasUtf8Bom: boolean;
  readonly lineEnding: MarkdownLineEnding;

  #source: string;
  #revision = 0;
  #closed = false;
  #originalBytes: Uint8Array;
  #listeners = new Set<MarkdownSessionListener>();

  private constructor(decoded: DecodedMarkdown, initialRevision = 0) {
    this.#source = decoded.source;
    this.#originalBytes = Uint8Array.from(decoded.originalBytes);
    this.#revision = initialRevision;
    this.isReadOnly = !decoded.isValidUtf8;
    this.hasUtf8Bom = decoded.hasUtf8Bom;
    this.lineEnding = decoded.lineEnding;
  }

  static fromBytes(bytes: Uint8Array): MarkdownSession {
    return MarkdownSession.fromDecoded(decodeMarkdownBytes(bytes));
  }

  static fromDecoded(decoded: DecodedMarkdown): MarkdownSession {
    return new MarkdownSession(decoded);
  }

  static fromRecoveredBytes(bytes: Uint8Array, revision: number): MarkdownSession {
    if (!Number.isSafeInteger(revision) || revision <= 0) {
      throw new Error("Recovered Markdown revision must be positive.");
    }
    const session = new MarkdownSession(decodeMarkdownBytes(bytes), revision);
    if (session.isReadOnly) {
      throw new Error("Recovered Markdown must be valid UTF-8.");
    }
    return session;
  }

  static fromSource(source: string): MarkdownSession {
    return MarkdownSession.fromBytes(new TextEncoder().encode(source));
  }

  get source(): string {
    return this.#source;
  }

  get revision(): number {
    return this.#revision;
  }

  toBytes(): Uint8Array {
    if (this.#revision === 0 || this.isReadOnly) {
      return Uint8Array.from(this.#originalBytes);
    }

    return encodeMarkdownSource(this.#source, this.hasUtf8Bom);
  }

  replaceSource(source: string, revision: number): void {
    this.#assertEditable();
    this.#assertRevision(revision);
    if (source === this.#source) {
      return;
    }

    this.#commit(source);
  }

  applyPatch(patch: MarkdownPatch): void {
    this.#assertEditable();
    this.#assertRevision(patch.revision);
    this.#assertRange(patch.from, patch.to);

    const replacement = this.#normalizeLineEndings(patch.replacement);
    const source =
      this.#source.slice(0, patch.from) +
      replacement +
      this.#source.slice(patch.to);
    if (source === this.#source) {
      return;
    }

    this.#commit(source);
  }

  subscribe(listener: MarkdownSessionListener): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  close(): void {
    this.#closed = true;
    this.#listeners.clear();
  }

  #assertEditable(): void {
    if (this.#closed) {
      throw new Error("This Markdown editing session is closed.");
    }
    if (this.isReadOnly) {
      throw new Error("Markdown with invalid UTF-8 is read-only.");
    }
  }

  #assertRevision(revision: number): void {
    if (revision !== this.#revision) {
      throw new Error(
        `Stale Markdown revision: expected ${this.#revision}, received ${revision}.`,
      );
    }
  }

  #assertRange(from: number, to: number): void {
    if (
      !Number.isInteger(from) ||
      !Number.isInteger(to) ||
      from < 0 ||
      to < from ||
      to > this.#source.length
    ) {
      throw new Error(`Invalid Markdown source range: ${from}..${to}.`);
    }
  }

  #commit(source: string): void {
    this.#source = source;
    this.#revision += 1;
    const change = { source: this.#source, revision: this.#revision };
    for (const listener of this.#listeners) {
      listener(change);
    }
  }

  #normalizeLineEndings(source: string): string {
    const lfSource = source.replace(/\r\n|\r/g, "\n");
    return this.lineEnding === "crlf"
      ? lfSource.replace(/\n/g, "\r\n")
      : lfSource;
  }
}
