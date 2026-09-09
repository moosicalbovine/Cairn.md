import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { getHealth, parseHealth } from "./health";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("health bridge", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it("accepts the narrow Rust health payload", () => {
    expect(
      parseHealth({ app: "NoteMD", version: "0.1.0", status: "ok" }),
    ).toEqual({ app: "NoteMD", version: "0.1.0", status: "ok" });
  });

  it("rejects malformed command responses", () => {
    expect(() => parseHealth({ status: "ok", content: "private" })).toThrow(
      "Invalid health response",
    );
  });

  it("invokes only the registered health command", async () => {
    vi.mocked(invoke).mockResolvedValue({
      app: "NoteMD",
      version: "0.1.0",
      status: "ok",
    });

    await expect(getHealth()).resolves.toMatchObject({ status: "ok" });
    expect(invoke).toHaveBeenCalledWith("health");
  });
});
