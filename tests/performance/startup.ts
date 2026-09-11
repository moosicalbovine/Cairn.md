import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { cpus, release as osRelease, tmpdir, totalmem, version as osVersion } from "node:os";
import { dirname, join, resolve } from "node:path";

type ProcessSample = Readonly<{
  mainWindowHandle: number;
  processCount: number;
  workingSetBytes: number;
  privateWorkingSetBytes: number;
}>;

type StartupSample = Readonly<{
  sample: number;
  startupMs: number;
  initialWorkingSetMb: number;
  initialPrivateWorkingSetMb: number;
}>;

type IdleMemorySample = Readonly<{
  processCount: number;
  aggregateWorkingSetMb: number;
  privateWorkingSetMb: number;
}>;

type BrowserReport = Readonly<{
  documentOpenMs: readonly number[];
  inputLatencyMs: readonly number[];
}>;

type BenchmarkOptions = Readonly<{
  binaryPath: string;
  samples: number;
  idleSeconds: number;
  outputPath: string | null;
}>;

type PerformanceProcess = Readonly<{
  child: ChildProcess;
  directory: string;
  reportPath: string;
  readyPath: string;
}>;

const pollIntervalMs = 50;
const startupTimeoutMs = 60_000;
const reportTimeoutMs = 120_000;
const startupLimitMs = 1_500;
const documentOpenLimitMs = 250;
const inputLatencyLimitMs = 50;
const idleMemoryLimitMb = 150;

function readProcessSample(processId: number, includeDescendants = false): ProcessSample | undefined {
  const script = [
    `$cairnRoot = Get-Process -Id ${processId} -ErrorAction SilentlyContinue`,
    "if ($null -eq $cairnRoot) { exit 2 }",
    `$cairnIds = @(${processId})`,
    includeDescendants
      ? "$cairnAll = @(Get-CimInstance Win32_Process); do { $cairnChildren = @($cairnAll | Where-Object { $cairnIds -contains [int]$_.ParentProcessId } | ForEach-Object { [int]$_.ProcessId } | Where-Object { $cairnIds -notcontains $_ }); $cairnIds += $cairnChildren } while ($cairnChildren.Count -gt 0)"
      : `$cairnIds = @(${processId})`,
    "$cairnProcesses = @($cairnIds | ForEach-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue } | Where-Object { $null -ne $_ })",
    "$cairnWorkingSet = ($cairnProcesses | Measure-Object -Property WorkingSet64 -Sum).Sum",
    "$cairnPerf = @(Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object { $cairnIds -contains [int]$_.IDProcess })",
    "$cairnPrivateWorkingSet = ($cairnPerf | Measure-Object -Property WorkingSetPrivate -Sum).Sum",
    "@{ mainWindowHandle = $cairnRoot.MainWindowHandle.ToInt64(); processCount = $cairnProcesses.Count; workingSetBytes = [double]$cairnWorkingSet; privateWorkingSetBytes = [double]$cairnPrivateWorkingSet } | ConvertTo-Json -Compress",
  ].join("; ");
  const result = spawnSync("powershell.exe", ["-NoProfile", "-Command", script], {
    encoding: "utf8",
    windowsHide: true,
  });
  if (result.status !== 0 || !result.stdout.trim()) return undefined;
  const parsed: unknown = JSON.parse(result.stdout);
  if (
    typeof parsed !== "object" ||
    parsed === null ||
    !("mainWindowHandle" in parsed) ||
    !("processCount" in parsed) ||
    !("workingSetBytes" in parsed) ||
    !("privateWorkingSetBytes" in parsed) ||
    typeof parsed.mainWindowHandle !== "number" ||
    typeof parsed.processCount !== "number" ||
    typeof parsed.workingSetBytes !== "number" ||
    typeof parsed.privateWorkingSetBytes !== "number"
  ) {
    throw new Error("Windows returned an invalid process sample");
  }
  return parsed as ProcessSample;
}

function startPerformanceProcess(
  binaryPath: string,
  scenario: "full" | "idle",
): PerformanceProcess {
  const directory = mkdtempSync(join(tmpdir(), "cairn-performance-"));
  const reportPath = join(directory, "browser-report.json");
  const child = spawn(binaryPath, [], {
    stdio: "ignore",
    windowsHide: false,
    env: {
      ...process.env,
      CAIRN_PERF_MODE: "1",
      CAIRN_PERF_SCENARIO: scenario,
      CAIRN_PERF_OUTPUT: reportPath,
      CAIRN_APP_DATA_DIR: join(directory, "app-data"),
    },
  });
  if (child.pid === undefined) {
    rmSync(directory, { recursive: true, force: true });
    throw new Error("Cairn.md process did not start");
  }
  return { child, directory, reportPath, readyPath: `${reportPath}.ready` };
}

function stopPerformanceProcess(run: PerformanceProcess): void {
  if (run.child.pid !== undefined) {
    spawnSync("taskkill.exe", ["/PID", String(run.child.pid), "/T", "/F"], {
      stdio: "ignore",
      windowsHide: true,
    });
  }
  try {
    rmSync(run.directory, {
      recursive: true,
      force: true,
      maxRetries: 20,
      retryDelay: 100,
    });
  } catch (reason) {
    const code =
      typeof reason === "object" && reason !== null && "code" in reason
        ? reason.code
        : undefined;
    if (code === "EPERM" || code === "EBUSY" || code === "ENOTEMPTY") {
      process.stderr.write("Warning: Windows deferred cleanup of isolated benchmark data.\n");
      return;
    }
    throw reason;
  }
}

async function delay(milliseconds: number): Promise<void> {
  await new Promise((resolveDelay) => setTimeout(resolveDelay, milliseconds));
}

async function waitForFile(
  run: PerformanceProcess,
  path: string,
  timeoutMs: number,
  description: string,
): Promise<void> {
  const startedAt = performance.now();
  while (performance.now() - startedAt < timeoutMs) {
    if (existsSync(path)) return;
    if (run.child.exitCode !== null || run.child.signalCode !== null) {
      throw new Error(`Cairn.md exited before ${description}`);
    }
    await delay(pollIntervalMs);
  }
  throw new Error(`Cairn.md did not produce ${description} within ${timeoutMs / 1_000} seconds`);
}

async function measureStartup(binaryPath: string, sample: number): Promise<StartupSample> {
  const startedAt = performance.now();
  const run = startPerformanceProcess(binaryPath, "idle");
  try {
    await waitForFile(run, run.readyPath, startupTimeoutMs, "the interactive-ready signal");
    const readyAt = performance.now();
    const processSample = readProcessSample(run.child.pid ?? -1);
    if (!processSample?.mainWindowHandle) throw new Error("Cairn.md reported ready without a window");
    return {
      sample,
      startupMs: Number((readyAt - startedAt).toFixed(1)),
      initialWorkingSetMb: Number((processSample.workingSetBytes / 1024 / 1024).toFixed(1)),
      initialPrivateWorkingSetMb: Number(
        (processSample.privateWorkingSetBytes / 1024 / 1024).toFixed(1),
      ),
    };
  } finally {
    stopPerformanceProcess(run);
  }
}

function parseBrowserReport(path: string): BrowserReport {
  const parsed: unknown = JSON.parse(readFileSync(path, "utf8"));
  if (
    typeof parsed !== "object" ||
    parsed === null ||
    !("documentOpenMs" in parsed) ||
    !("inputLatencyMs" in parsed) ||
    !Array.isArray(parsed.documentOpenMs) ||
    !Array.isArray(parsed.inputLatencyMs) ||
    parsed.documentOpenMs.length < 20 ||
    parsed.inputLatencyMs.length < 20 ||
    parsed.documentOpenMs.some((value) => typeof value !== "number" || !Number.isFinite(value)) ||
    parsed.inputLatencyMs.some((value) => typeof value !== "number" || !Number.isFinite(value))
  ) {
    throw new Error("Cairn.md returned an invalid browser performance report");
  }
  return parsed as BrowserReport;
}

async function measureBrowser(binaryPath: string): Promise<BrowserReport> {
  const run = startPerformanceProcess(binaryPath, "full");
  try {
    await waitForFile(run, run.readyPath, startupTimeoutMs, "the interactive-ready signal");
    await waitForFile(run, run.reportPath, reportTimeoutMs, "the browser measurements");
    return parseBrowserReport(run.reportPath);
  } finally {
    stopPerformanceProcess(run);
  }
}

async function measureIdle(
  binaryPath: string,
  idleSeconds: number,
): Promise<IdleMemorySample> {
  const run = startPerformanceProcess(binaryPath, "idle");
  try {
    await waitForFile(run, run.readyPath, startupTimeoutMs, "the interactive-ready signal");
    await delay(idleSeconds * 1_000);
    const sample = readProcessSample(run.child.pid ?? -1, true);
    if (!sample) throw new Error("Cairn.md exited before the idle-memory sample");
    return {
      processCount: sample.processCount,
      aggregateWorkingSetMb: Number((sample.workingSetBytes / 1024 / 1024).toFixed(1)),
      privateWorkingSetMb: Number(
        (sample.privateWorkingSetBytes / 1024 / 1024).toFixed(1),
      ),
    };
  } finally {
    stopPerformanceProcess(run);
  }
}

function percentile(values: readonly number[], percentileValue: number): number {
  const sorted = [...values].sort((left, right) => left - right);
  const index = Math.max(0, Math.ceil((percentileValue / 100) * sorted.length) - 1);
  return sorted[index] ?? Number.NaN;
}

function metricSummary(values: readonly number[], limitP95: number) {
  const p95 = percentile(values, 95);
  return {
    samples: values,
    p50: percentile(values, 50),
    p95,
    maximum: Math.max(...values),
    limitP95,
    passed: p95 <= limitP95,
  };
}

function parsePositiveInteger(value: string | undefined, option: string): number {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) {
    throw new Error(`${option} must be a positive integer`);
  }
  return parsed;
}

function parseOptions(args: readonly string[]): BenchmarkOptions {
  const binary = args[0];
  if (!binary) {
    throw new Error(
      "Usage: npm run perf -- <cairn-md.exe> [--samples 20] [--idle-seconds 60] [--output path]",
    );
  }
  let samples = 20;
  let idleSeconds = 60;
  let outputPath: string | null = null;
  for (let index = 1; index < args.length; index += 2) {
    const option = args[index];
    const value = args[index + 1];
    if (option === "--samples") samples = parsePositiveInteger(value, option);
    else if (option === "--idle-seconds") idleSeconds = parsePositiveInteger(value, option);
    else if (option === "--output" && value) outputPath = resolve(value);
    else throw new Error(`Unknown or incomplete option: ${option ?? ""}`);
  }
  return { binaryPath: resolve(binary), samples, idleSeconds, outputPath };
}

async function run(options: BenchmarkOptions): Promise<void> {
  if (!existsSync(options.binaryPath)) {
    throw new Error(`Release binary not found: ${options.binaryPath}`);
  }
  const startup: StartupSample[] = [];
  for (let sample = 1; sample <= options.samples; sample += 1) {
    startup.push(await measureStartup(options.binaryPath, sample));
  }
  const browser = await measureBrowser(options.binaryPath);
  const idleMemorySample = await measureIdle(options.binaryPath, options.idleSeconds);
  const startupSummary = metricSummary(
    startup.map((sample) => sample.startupMs),
    startupLimitMs,
  );
  const documentOpen = metricSummary(browser.documentOpenMs, documentOpenLimitMs);
  const inputLatency = metricSummary(browser.inputLatencyMs, inputLatencyLimitMs);
  const idleMemory = {
    value: idleMemorySample.privateWorkingSetMb,
    aggregateWorkingSetMb: idleMemorySample.aggregateWorkingSetMb,
    processCount: idleMemorySample.processCount,
    idleSeconds: options.idleSeconds,
    limit: idleMemoryLimitMb,
    measurement: "sum of per-process private working sets",
    passed: idleMemorySample.privateWorkingSetMb < idleMemoryLimitMb,
  };
  const summary = {
    benchmark: "cairn-release-performance",
    measuredAt: new Date().toISOString(),
    commit: process.env.GITHUB_SHA ?? "local",
    binaryPath: options.binaryPath,
    environment: {
      platform: process.platform,
      architecture: process.arch,
      osVersion: osVersion(),
      osRelease: osRelease(),
      cpu: cpus()[0]?.model ?? "unknown",
      logicalProcessors: cpus().length,
      installedMemoryGb: Number((totalmem() / 1024 / 1024 / 1024).toFixed(1)),
    },
    startupRuns: startup,
    startupMs: startupSummary,
    documentOpenMs: documentOpen,
    inputLatencyMs: inputLatency,
    idleWorkingSetMb: idleMemory,
    passed: startupSummary.passed && documentOpen.passed && inputLatency.passed && idleMemory.passed,
  };

  const serialized = `${JSON.stringify(summary, null, 2)}\n`;
  process.stdout.write(serialized);
  if (options.outputPath) {
    mkdirSync(dirname(options.outputPath), { recursive: true });
    writeFileSync(options.outputPath, serialized, "utf8");
  }
  if (!summary.passed) process.exitCode = 1;
}

await run(parseOptions(process.argv.slice(2)));
