// @vitest-environment node
import { describe, expect, it } from "vitest";

import { normalPerformanceDocument } from "../../src/features/performance/normalDocument";

describe("release performance fixture", () => {
  it("represents a normal 250 KB Markdown document", () => {
    const source = normalPerformanceDocument();
    const bytes = new TextEncoder().encode(source).byteLength;
    const lines = source.split("\n").length;

    expect(bytes).toBeGreaterThanOrEqual(240 * 1024);
    expect(bytes).toBeLessThanOrEqual(275 * 1024);
    expect(lines).toBeGreaterThanOrEqual(1_000);
    expect(source).toContain("- [ ] Review section");
    expect(source).toContain("| Topic | Status |");
    expect(source).toContain("```text");
  });
});
