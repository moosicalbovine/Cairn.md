import { invoke } from "@tauri-apps/api/core";

export type BrowserPerformanceReport = Readonly<{
  documentOpenMs: readonly number[];
  inputLatencyMs: readonly number[];
}>;

export function isPerformanceMode(): Promise<boolean> {
  return invoke<boolean>("performance_mode");
}

export function markPerformanceReady(): Promise<void> {
  return invoke<void>("mark_performance_ready");
}

export function writePerformanceReport(report: BrowserPerformanceReport): Promise<void> {
  return invoke<void>("write_performance_report", { report });
}
