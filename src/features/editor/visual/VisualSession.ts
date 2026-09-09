import {
  createMarkdownProjection,
  type MarkdownProjection,
} from "../markdown/projection";
import { MarkdownSession } from "../session/MarkdownSession";

export class VisualSession {
  #projection: MarkdownProjection;
  readonly #session: MarkdownSession;
  readonly #unsubscribe: () => void;

  constructor(session: MarkdownSession) {
    this.#session = session;
    this.#projection = createMarkdownProjection(session.source, session.revision);
    this.#unsubscribe = session.subscribe(({ source, revision }) => {
      this.#projection = createMarkdownProjection(source, revision);
    });
  }

  get projection(): MarkdownProjection {
    return this.#projection;
  }

  replaceSegment(
    segmentId: string,
    replacement: string,
    revision: number,
  ): void {
    if (revision !== this.#projection.revision) {
      throw new Error("Visual edit targets a stale Markdown revision.");
    }

    const segment = this.#projection.segments.find(
      (candidate) => candidate.id === segmentId,
    );
    if (!segment) {
      throw new Error(`Unknown visual segment: ${segmentId}.`);
    }
    if (segment.kind !== "visual") {
      throw new Error("Source-backed Markdown must be edited in Source mode.");
    }

    this.#session.applyPatch({
      from: segment.from,
      to: segment.to,
      replacement,
      revision,
    });
  }

  dispose(): void {
    this.#unsubscribe();
  }
}
