import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";

type ProcessSample = Readonly<{
  mainWindowHandle: number;
  workingSetBytes: number;
}>;

type StartupSample = Readonly<{
  sample: number;
  startupMs: number;
  initialWorkingSetMb: number;
}>;

type BenchmarkOptions = Readonly<{
  binaryPath: string;
  samples: number;
  idleSeconds: number;
  outputPath: string | null;
}>;

const pollIntervalMs = 50;
const startupTimeoutMs = 10_000;
const startupLimitMs = 1_500;
const idleMemoryLimitMb = 150;

function readProcessSample(processId: number, includeDescendants = false): ProcessSample | undefined {
  const script = [
    `$cairnRoot = Get-Process -Id ${processId} -ErrorAction SilentlyContinue`,
    "if ($null -eq $cairnRoot) { exit 2 }",
    `$cairnIds = @(${processId})`,
    includeDescendants
      ? "$cairnAll = @(Get-CimInstance Win32_Process); do { $cairnChildren = @($cairnAll | Where-Object { $cairnIds -contains [int]$_.ParentProcessId } | ForEach-Object { [int]$_.ProcessId } | Where-Object { $cairnIds -notcontains $_ }); $cairnIds += $cairnChildren } while ($cairnChildren.Count -gt 0)"
      : `$cairnIds = @(${processId})`,
    "$cairnWorkingSet = ($cairnIds | ForEach-Object { (Get-Process -Id $_ -ErrorAction SilentlyContinue).WorkingSet64 } | Measure-Object -Sum).Sum",
    "@{ mainWindowHandle = $cairnRoot.MainWindowHandle.ToInt64(); workingSetBytes = [double]$cairnWorkingSet } | ConvertTo-Json -Compress",
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
    !("workingSetBytes" in parsed) ||
    typeof parsed.mainWindowHandle !== "number" ||
    typeof parsed.workingSetBytes !== "number"
  ) {
    throw new Error("Windows returned an invalid process sample");
  }
  return parsed as ProcessSample;
}

function stopProcessTree(processId: number): void {
  spawnSync("taskkill.exe", ["/PID", String(processId), "/T", "/F"], {
    stdio: "ignore",
    windowsHide: true,
  });
}

async function delay(milliseconds: number): Promise<void> {
  await new Promise((resolveDelay) => setTimeout(resolveDelay, milliseconds));
}

async function waitForWindow(processId: number): Promise<ProcessSample> {
  const startedAt = performance.now();
  while (performance.now() - startedAt < startupTimeoutMs) {
    const sample = readProcessSample(processId);
    if (sample?.mainWindowHandle) return sample;
    await delay(pollIntervalMs);
  }
  throw new Error("Cairn.md did not create its main window within 10 seconds");
}

async function measureStartup(binaryPath: string, sample: number): Promise<StartupSample> {
  const startedAt = performance.now();
  const child = spawn(binaryPath, [], { stdio: "ignore", windowsHide: false });
  if (child.pid === undefined) throw new Error("Cairn.md process did not start");
  try {
    const processSample = await waitForWindow(child.pid);
    return {
      sample,
      startupMs: Number((performance.now() - startedAt).toFixed(1)),
      initialWorkingSetMb: Number((processSample.workingSetBytes / 1024 / 1024).toFixed(1)),
    };
  } finally {
    stopProcessTree(child.pid);
  }
}

async function measureIdleMemory(binaryPath: string, idleSeconds: number): Promise<number> {
  const child = spawn(binaryPath, [], { stdio: "ignore", windowsHide: false });
  if (child.pid === undefined) throw new Error("Cairn.md process did not start");
  try {
    await waitForWindow(child.pid);
    await delay(idleSeconds * 1_000);
    const sample = readProcessSample(child.pid, true);
    if (!sample) throw new Error("Cairn.md exited before the idle-memory sample");
    return Number((sample.workingSetBytes / 1024 / 1024).toFixed(1));
  } finally {
    stopProcessTree(child.pid);
  }
}

function percentile(values: readonly number[], percentileValue: number): number {
  const sorted = [...values].sort((left, right) => left - right);
  const index = Math.max(0, Math.ceil((percentileValue / 100) * sorted.length) - 1);
  return sorted[index] ?? Number.NaN;
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
      "Usage: npm run perf:startup -- <cairn-md.exe> [--samples 20] [--idle-seconds 60] [--output path]",
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
  const startup: StartupSample[] = [];
  for (let sample = 1; sample <= options.samples; sample += 1) {
    startup.push(await measureStartup(options.binaryPath, sample));
  }
  const idleWorkingSetMb = await measureIdleMemory(options.binaryPath, options.idleSeconds);
  const startupValues = startup.map((sample) => sample.startupMs);
  const startupP95 = percentile(startupValues, 95);
  const summary = {
    benchmark: "cairn-startup-and-idle-memory",
    measuredAt: new Date().toISOString(),
    binaryPath: options.binaryPath,
    samples: startup,
    startupMs: {
      p50: percentile(startupValues, 50),
      p95: startupP95,
      maximum: Math.max(...startupValues),
      limitP95: startupLimitMs,
      passed: startupP95 <= startupLimitMs,
    },
    idleWorkingSetMb: {
      value: idleWorkingSetMb,
      idleSeconds: options.idleSeconds,
      limit: idleMemoryLimitMb,
      passed: idleWorkingSetMb < idleMemoryLimitMb,
    },
  };

  const serialized = `${JSON.stringify(summary, null, 2)}\n`;
  process.stdout.write(serialized);
  if (options.outputPath) {
    mkdirSync(dirname(options.outputPath), { recursive: true });
    writeFileSync(options.outputPath, serialized, "utf8");
  }
  if (!summary.startupMs.passed || !summary.idleWorkingSetMb.passed) {
    process.exitCode = 1;
  }
}

await run(parseOptions(process.argv.slice(2)));
