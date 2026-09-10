// @vitest-environment node
import { describe, expect, it } from "vitest";

import { EditorSession } from "../../src/features/editor/session/EditorSession";

describe("editor stress", () => {
  it("preserves opaque Markdown through sustained cross-region editing", () => {
    const opaque = "<!-- preserve exactly -->\n\n<div data-owner='external'>raw</div>";
    const editor = EditorSession.fromSource(`Before 0.\n\n${opaque}\n\nAfter 0.\n`);

    for (let iteration = 1; iteration <= 500; iteration += 1) {
      const first = editor.visual.projection.segments.find(
        (segment) => segment.kind === "visual" && segment.source.startsWith("Before"),
      );
      const last = editor.visual.projection.segments.find(
        (segment) => segment.kind === "visual" && segment.source.startsWith("After"),
      );
      expect(first).toBeDefined();
      expect(last).toBeDefined();
      editor.visual.replaceSegment(
        first?.id ?? "missing",
        `Before ${iteration}.`,
        editor.session.revision,
      );
      const refreshedLast = editor.visual.projection.segments.find(
        (segment) => segment.id === last?.id,
      );
      editor.visual.replaceSegment(
        refreshedLast?.id ?? "missing",
        `After ${iteration}.`,
        editor.session.revision,
      );
      editor.switchMode("source");
      editor.switchMode("visual");
    }

    expect(editor.session.revision).toBe(1_000);
    expect(editor.session.source).toBe(`Before 500.\n\n${opaque}\n\nAfter 500.\n`);
    expect(editor.session.toBytes()).toEqual(
      new TextEncoder().encode(`Before 500.\n\n${opaque}\n\nAfter 500.\n`),
    );
  });

  it("coalesces a long source-edit sequence into the exact final bytes", () => {
    const editor = EditorSession.fromSource("# Stress\n\n0\n");
    for (let revision = 0; revision < 10_000; revision += 1) {
      editor.replaceSource(`# Stress\n\n${revision + 1}\n`, revision);
    }

    expect(editor.session.revision).toBe(10_000);
    expect(editor.session.toBytes()).toEqual(
      new TextEncoder().encode("# Stress\n\n10000\n"),
    );
  });
});
