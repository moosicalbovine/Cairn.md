import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { getPerformanceScenario } from "./performance";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("performance command boundary", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it.each(["full", "idle"] as const)("accepts the %s scenario", async (scenario) => {
    vi.mocked(invoke).mockResolvedValueOnce(scenario);

    await expect(getPerformanceScenario()).resolves.toBe(scenario);
    expect(invoke).toHaveBeenCalledWith("performance_scenario");
  });

  it("rejects unknown scenarios", async () => {
    vi.mocked(invoke).mockResolvedValueOnce("memory-only");

    await expect(getPerformanceScenario()).rejects.toThrow("Invalid performance scenario");
  });
});
