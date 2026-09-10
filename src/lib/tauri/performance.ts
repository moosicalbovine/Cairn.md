import { invoke } from "@tauri-apps/api/core";

export type BrowserPerformanceReport = Readonly<{
  documentOpenMs: readonly number[];
  inputLatencyMs: readonly number[];
}>;

export function isPerformanceMode(): Promise<boolean> {
  return invoke<boolean>("performance_mode");
}

export async function getPerformanceScenario(): Promise<"full" | "idle"> {
  const value = await invoke<unknown>("performance_scenario");
  if (value !== "full" && value !== "idle") {
    throw new Error("Invalid performance scenario");
  }
  return value;
}

export function markPerformanceReady(): Promise<void> {
  return invoke<void>("mark_performance_ready");
}

export function writePerformanceReport(report: BrowserPerformanceReport): Promise<void> {
  return invoke<void>("write_performance_report", { report });
}
