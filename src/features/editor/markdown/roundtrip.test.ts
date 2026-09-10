// @vitest-environment node
import { readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

import { decodeMarkdownBytes } from "./documentFormat";
import { createMarkdownProjection } from "./projection";
import { MarkdownSession } from "../session/MarkdownSession";
import { VisualSession } from "../visual/VisualSession";

const fixtureDirectory = resolve(process.cwd(), "tests/fixtures/markdown");

function fixtureBytes(name: string): Uint8Array {
  const path = resolve(fixtureDirectory, name);

  if (name.endsWith(".hex")) {
    const hexadecimal = readFileSync(path, "utf8").replace(/\s/g, "");
    return Uint8Array.from(Buffer.from(hexadecimal, "hex"));
  }

  return Uint8Array.from(readFileSync(path));
}

const validFixtures = readdirSync(fixtureDirectory)
  .filter((name) => name.endsWith(".md") || name.endsWith(".md.hex"))
  .filter((name) => name !== "invalid-utf8.md.hex")
  .sort();

describe("Markdown byte round trips", () => {
  it.each(validFixtures)("preserves %s byte-for-byte without edits", (name) => {
    const original = fixtureBytes(name);
    const session = MarkdownSession.fromBytes(original);

    expect(session.isReadOnly).toBe(false);
    expect(Buffer.from(session.toBytes()).equals(Buffer.from(original))).toBe(true);
    expect(session.revision).toBe(0);
  });

  it("keeps invalid UTF-8 read-only and can only reproduce unchanged bytes", () => {
    const original = fixtureBytes("invalid-utf8.md.hex");
    const decoded = decodeMarkdownBytes(original);
    const session = MarkdownSession.fromDecoded(decoded);

    expect(decoded.isValidUtf8).toBe(false);
    expect(session.isReadOnly).toBe(true);
    expect(Buffer.from(session.toBytes()).equals(Buffer.from(original))).toBe(true);
    expect(() => session.replaceSource(`${session.source}changed`, 0)).toThrow(
      /invalid UTF-8/i,
    );
  });

  it("retains a BOM and the detected CRLF convention after a visual patch", () => {
    const original = Uint8Array.from(
      Buffer.concat([
        Buffer.from([0xef, 0xbb, 0xbf]),
        Buffer.from("# Old\r\n\r\nSecond line.\r\n", "utf8"),
      ]),
    );
    const session = MarkdownSession.fromBytes(original);

    session.applyPatch({
      from: 0,
      to: 5,
      replacement: "# New\ncontinued",
      revision: 0,
    });

    expect(session.hasUtf8Bom).toBe(true);
    expect(session.lineEnding).toBe("crlf");
    expect(Buffer.from(session.toBytes())).toEqual(
      Buffer.concat([
        Buffer.from([0xef, 0xbb, 0xbf]),
        Buffer.from("# New\r\ncontinued\r\n\r\nSecond line.\r\n", "utf8"),
      ]),
    );
  });
});

describe("source-ranged visual projection", () => {
  it("changes only the selected supported source range", () => {
    const source =
      "# Before\n\nParagraph **old** text.\n\n<!-- keep exactly -->\n\nAfter `$x$`.\n";
    const session = MarkdownSession.fromSource(source);
    const visual = new VisualSession(session);
    const segment = visual.projection.segments.find(
      (candidate) =>
        candidate.kind === "visual" && candidate.source.includes("Paragraph"),
    );

    expect(segment).toBeDefined();
    const before = source.slice(0, segment?.from);
    const after = source.slice(segment?.to);

    visual.replaceSegment(
      segment?.id ?? "missing",
      "Paragraph **new** text.",
      session.revision,
    );

    expect(session.source).toBe(
      `${before}Paragraph **new** text.${after}`,
    );
    expect(session.source).toContain("<!-- keep exactly -->");
    expect(session.source).toContain("After `$x$`.");
    expect(session.revision).toBe(1);
  });

  it("keeps unsupported blocks opaque when editing before and after them", () => {
    const opaque = "<!-- byte-sensitive -->\n\n<div data-x='1'>raw</div>\n\n$$x^2$$";
    const source = `Before.\n\n${opaque}\n\nAfter.\n`;
    const session = MarkdownSession.fromSource(source);
    const visual = new VisualSession(session);

    const first = visual.projection.segments.find(
      (segment) => segment.kind === "visual" && segment.source === "Before.",
    );
    expect(first).toBeDefined();
    visual.replaceSegment(first?.id ?? "missing", "Changed before.", 0);

    const last = visual.projection.segments.find(
      (segment) => segment.kind === "visual" && segment.source === "After.",
    );
    expect(last).toBeDefined();
    expect(last?.id).toBe("visual:2");
    visual.replaceSegment(last?.id ?? "missing", "Changed after.", 1);

    expect(session.source).toContain(opaque);
    expect(session.source).toBe(`Changed before.\n\n${opaque}\n\nChanged after.\n`);
  });

  it("groups adjacent supported blocks into one stable visual editing region", () => {
    const session = MarkdownSession.fromSource(
      "# Heading\n\nFirst paragraph.\n\n- one\n- two\n",
    );
    const visual = new VisualSession(session);

    expect(visual.projection.segments).toHaveLength(1);
    expect(visual.projection.segments[0]).toMatchObject({
      id: "visual:0",
      kind: "visual",
      source: session.source.trimEnd(),
    });

    visual.replaceSegment(
      "visual:0",
      "# Updated\n\nFirst paragraph.\n\n- one\n- two\n- three",
      0,
    );

    expect(visual.projection.segments[0]?.id).toBe("visual:0");
    expect(visual.projection.revision).toBe(1);
  });

  it.each(["", "  \n\t"])("keeps blank Markdown editable in visual mode", (source) => {
    const session = MarkdownSession.fromSource(source);
    const visual = new VisualSession(session);

    expect(visual.projection.segments).toEqual([
      expect.objectContaining({
        id: "visual:0",
        kind: "visual",
        from: 0,
        to: source.length,
        source,
      }),
    ]);

    visual.replaceSegment("visual:0", "First note", 0);
    expect(session.source).toBe("First note");
  });

  it("reparses source edits to unsupported content", () => {
    const session = MarkdownSession.fromSource("Before.\n\n$$old$$\n\nAfter.\n");
    const visual = new VisualSession(session);

    session.replaceSource(
      session.source.replace("$$old$$", () => "$$new$$"),
      0,
    );

    expect(visual.projection.revision).toBe(1);
    expect(
      visual.projection.segments.some(
        (segment) =>
          segment.kind === "source-backed" && segment.source.includes("$$new$$"),
      ),
    ).toBe(true);
  });

  it("rejects stale and out-of-range source patches", () => {
    const session = MarkdownSession.fromSource("alpha");

    session.applyPatch({ from: 0, to: 5, replacement: "beta", revision: 0 });

    expect(() =>
      session.applyPatch({ from: 0, to: 4, replacement: "gamma", revision: 0 }),
    ).toThrow(/revision/i);
    expect(() =>
      session.applyPatch({ from: -1, to: 2, replacement: "x", revision: 1 }),
    ).toThrow(/range/i);
  });

  it("records non-gating projection timing for normal and large fixtures", () => {
    const measurements = ["commonmark.md", "large.md"].map((name) => {
      const decoded = decodeMarkdownBytes(fixtureBytes(name));
      const started = performance.now();
      const projection = createMarkdownProjection(decoded.source, 0);
      return {
        fixture: name,
        milliseconds: Number((performance.now() - started).toFixed(3)),
        segments: projection.segments.length,
      };
    });

    console.info("Markdown projection timing (non-gating)", measurements);
    expect(measurements).toHaveLength(2);
  });
});
