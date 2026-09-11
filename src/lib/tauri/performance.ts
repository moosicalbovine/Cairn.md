import { invoke } from "@tauri-apps/api/core";

export type BrowserPerformanceReport = Readonly<{
  documentOpenMs: readonly number[];
  inputLatencyMs: readonly number[];
}>;

export type PerformanceFixturePaths = Readonly<{
  libraryRoot: string;
  trackedRoot: string;
}>;

export type InstalledSmokeReport = Readonly<{
  projectCreated: boolean;
  trackedImport: boolean;
  visualEdit: boolean;
  sourceMode: boolean;
  autosave: boolean;
  message: string;
}>;

export function isPerformanceMode(): Promise<boolean> {
  return invoke<boolean>("performance_mode");
}

export async function getPerformanceScenario(): Promise<"full" | "idle" | "installed"> {
  const value = await invoke<unknown>("performance_scenario");
  if (value !== "full" && value !== "idle" && value !== "installed") {
    throw new Error("Invalid performance scenario");
  }
  return value;
}

export async function getPerformanceFixturePaths(): Promise<PerformanceFixturePaths> {
  const value = await invoke<unknown>("performance_fixture_paths");
  if (
    typeof value !== "object" ||
    value === null ||
    !("libraryRoot" in value) ||
    !("trackedRoot" in value) ||
    typeof value.libraryRoot !== "string" ||
    value.libraryRoot.length === 0 ||
    typeof value.trackedRoot !== "string" ||
    value.trackedRoot.length === 0
  ) {
    throw new Error("Invalid performance fixture paths");
  }
  return { libraryRoot: value.libraryRoot, trackedRoot: value.trackedRoot };
}

export function markPerformanceReady(): Promise<void> {
  return invoke<void>("mark_performance_ready");
}

export function writePerformanceReport(report: BrowserPerformanceReport): Promise<void> {
  return invoke<void>("write_performance_report", { report });
}

export function writeInstalledSmokeReport(report: InstalledSmokeReport): Promise<void> {
  return invoke<void>("write_installed_smoke_report", { report });
}
