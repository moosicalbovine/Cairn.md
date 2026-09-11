import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  getPerformanceFixturePaths,
  getPerformanceScenario,
  writeInstalledSmokeReport,
} from "./performance";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("performance command boundary", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it.each(["full", "idle", "installed"] as const)("accepts the %s scenario", async (scenario) => {
    vi.mocked(invoke).mockResolvedValueOnce(scenario);

    await expect(getPerformanceScenario()).resolves.toBe(scenario);
    expect(invoke).toHaveBeenCalledWith("performance_scenario");
  });

  it("rejects unknown scenarios", async () => {
    vi.mocked(invoke).mockResolvedValueOnce("memory-only");

    await expect(getPerformanceScenario()).rejects.toThrow("Invalid performance scenario");
  });

  it("validates isolated installed-workflow fixture paths", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      libraryRoot: "C:\\temp\\library",
      trackedRoot: "C:\\temp\\tracked",
    });

    await expect(getPerformanceFixturePaths()).resolves.toEqual({
      libraryRoot: "C:\\temp\\library",
      trackedRoot: "C:\\temp\\tracked",
    });
    expect(invoke).toHaveBeenCalledWith("performance_fixture_paths");
  });

  it("rejects missing installed-workflow fixture paths", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ libraryRoot: "", trackedRoot: null });

    await expect(getPerformanceFixturePaths()).rejects.toThrow(
      "Invalid performance fixture paths",
    );
  });

  it("sends the installed workflow proof through a narrow command", async () => {
    const report = {
      projectCreated: true,
      trackedImport: true,
      visualEdit: true,
      sourceMode: true,
      autosave: true,
      message: "Installed workflow passed.",
    };

    await writeInstalledSmokeReport(report);

    expect(invoke).toHaveBeenCalledWith("write_installed_smoke_report", { report });
  });
});
