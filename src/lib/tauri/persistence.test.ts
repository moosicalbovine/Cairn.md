import { describe, expect, it } from "vitest";

import { parseRecoverySnapshot } from "./persistence";

describe("recovery command boundary", () => {
  it("accepts a complete snapshot and restores its bytes", () => {
    const snapshot = parseRecoverySnapshot({
      documentId: "document-1",
      sessionGeneration: "session-1",
      revision: 3,
      bytes: [35, 32, 68, 114, 97, 102, 116],
      contentHash: `sha256:${"a".repeat(64)}`,
      baseFingerprint: `sha256:${"b".repeat(64)}`,
      intendedDiskHash: `sha256:${"a".repeat(64)}`,
      operationId: null,
      lifecycleState: "Draft",
      durableAt: 42,
    });

    expect(new TextDecoder().decode(snapshot.bytes)).toBe("# Draft");
    expect(snapshot.revision).toBe(3);
  });

  it("rejects malformed bytes and lifecycle state", () => {
    expect(() =>
      parseRecoverySnapshot({
        documentId: "document-1",
        sessionGeneration: "session-1",
        revision: 1,
        bytes: [300],
        contentHash: "hash",
        baseFingerprint: "base",
        intendedDiskHash: "hash",
        operationId: null,
        lifecycleState: "Unknown",
        durableAt: 42,
      }),
    ).toThrow(/recovery snapshot/i);
  });
});
