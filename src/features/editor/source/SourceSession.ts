import { MarkdownSession } from "../session/MarkdownSession";

export class SourceSession {
  readonly #session: MarkdownSession;

  constructor(session: MarkdownSession) {
    this.#session = session;
  }

  get source(): string {
    return this.#session.source;
  }

  get revision(): number {
    return this.#session.revision;
  }

  get isReadOnly(): boolean {
    return this.#session.isReadOnly;
  }

  replaceSource(source: string, revision: number): void {
    this.#session.replaceSource(source, revision);
  }
}
