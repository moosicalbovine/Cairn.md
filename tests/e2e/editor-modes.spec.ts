// @vitest-environment node
import { describe, expect, it } from "vitest";

import { EditorSession } from "../../src/features/editor/session/EditorSession";

// Deliberate U4 boundary: Tauri WebDriver infrastructure does not exist yet, so
// this deterministic integration test exercises the real session and parser chain.
describe("Visual and Source mode integration", () => {
  it("opens in Visual and repeated mode switches preserve the canonical bytes", () => {
    const original = new TextEncoder().encode(
      "# Portable\r\n\r\nText before.\r\n\r\n<!-- opaque -->\r\n",
    );
    const editor = EditorSession.open(original);

    expect(editor.mode).toBe("visual");

    for (let index = 0; index < 10; index += 1) {
      editor.switchMode("source");
      editor.switchMode("visual");
    }

    expect(editor.session.revision).toBe(0);
    expect(Buffer.from(editor.session.toBytes()).equals(Buffer.from(original))).toBe(
      true,
    );
  });

  it("uses one revisioned source for Visual edits and Source fallback edits", () => {
    const editor = EditorSession.fromSource(
      "Editable before.\n\n<!-- unsupported old -->\n\nEditable after.\n",
    );
    const visualSegment = editor.visual.projection.segments.find(
      (segment) =>
        segment.kind === "visual" && segment.source === "Editable before.",
    );

    expect(visualSegment).toBeDefined();
    editor.visual.replaceSegment(
      visualSegment?.id ?? "missing",
      "Edited visually.",
      0,
    );
    editor.switchMode("source");
    editor.replaceSource(
      editor.session.source.replace("unsupported old", "unsupported new"),
      1,
    );

    expect(editor.session.revision).toBe(2);
    expect(editor.session.source).toBe(
      "Edited visually.\n\n<!-- unsupported new -->\n\nEditable after.\n",
    );
    expect(editor.visual.projection.segments).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          kind: "source-backed",
          source: "<!-- unsupported new -->",
        }),
      ]),
    );
  });

  it("exposes only Visual and Source modes", () => {
    const editor = EditorSession.fromSource("");

    expect(editor.availableModes).toEqual(["visual", "source"]);
    expect(() => editor.switchMode("preview" as never)).toThrow(/mode/i);
  });
});
