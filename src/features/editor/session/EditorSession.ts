import { MarkdownSession } from "./MarkdownSession";
import { SourceSession } from "../source/SourceSession";
import { VisualSession } from "../visual/VisualSession";

export type EditorMode = "visual" | "source";

const availableModes = ["visual", "source"] as const;

export class EditorSession {
  readonly session: MarkdownSession;
  readonly visual: VisualSession;
  readonly source: SourceSession;
  readonly availableModes = availableModes;

  #mode: EditorMode;

  private constructor(session: MarkdownSession) {
    this.session = session;
    this.visual = new VisualSession(session);
    this.source = new SourceSession(session);
    this.#mode = session.isReadOnly ? "source" : "visual";
  }

  static open(bytes: Uint8Array): EditorSession {
    return new EditorSession(MarkdownSession.fromBytes(bytes));
  }

  static fromSource(source: string): EditorSession {
    return new EditorSession(MarkdownSession.fromSource(source));
  }

  static openRecovered(bytes: Uint8Array, revision: number): EditorSession {
    return new EditorSession(MarkdownSession.fromRecoveredBytes(bytes, revision));
  }

  get mode(): EditorMode {
    return this.#mode;
  }

  switchMode(mode: EditorMode): void {
    if (!availableModes.includes(mode)) {
      throw new Error(`Unsupported editor mode: ${String(mode)}.`);
    }
    if (this.session.isReadOnly && mode === "visual") {
      throw new Error("Invalid UTF-8 Markdown is available in Source mode only.");
    }
    this.#mode = mode;
  }

  replaceSource(source: string, revision: number): void {
    this.source.replaceSource(source, revision);
  }

  dispose(): void {
    this.visual.dispose();
  }
}
