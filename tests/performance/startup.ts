import { spawn, spawnSync } from "node:child_process";
import { resolve } from "node:path";

type ProcessSample = Readonly<{
  mainWindowHandle: number;
  workingSetBytes: number;
}>;

const pollIntervalMs = 50;
const startupTimeoutMs = 10_000;

function readProcessSample(processId: number): ProcessSample | undefined {
  const script = [
    `$notemdProcess = Get-Process -Id ${processId} -ErrorAction SilentlyContinue`,
    "if ($null -eq $notemdProcess) { exit 2 }",
    "@{ mainWindowHandle = $notemdProcess.MainWindowHandle.ToInt64(); workingSetBytes = $notemdProcess.WorkingSet64 } | ConvertTo-Json -Compress",
  ].join("; ");

  const result = spawnSync("powershell.exe", ["-NoProfile", "-Command", script], {
    encoding: "utf8",
    windowsHide: true,
  });

  if (result.status !== 0 || !result.stdout.trim()) {
    return undefined;
  }

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

async function delay(milliseconds: number): Promise<void> {
  await new Promise((resolveDelay) => setTimeout(resolveDelay, milliseconds));
}

async function measure(binaryPath: string): Promise<void> {
  const startedAt = performance.now();
  const child = spawn(resolve(binaryPath), [], {
    stdio: "ignore",
    windowsHide: false,
  });

  if (child.pid === undefined) {
    throw new Error("NoteMD process did not start");
  }

  try {
    while (performance.now() - startedAt < startupTimeoutMs) {
      const sample = readProcessSample(child.pid);
      if (sample?.mainWindowHandle) {
        const startupMs = performance.now() - startedAt;
        console.log(
          JSON.stringify({
            metric: "u1-process-window-baseline",
            startupMs: Math.round(startupMs),
            workingSetMb: Number(
              (sample.workingSetBytes / 1024 / 1024).toFixed(1),
            ),
          }),
        );
        return;
      }
      await delay(pollIntervalMs);
    }

    throw new Error("NoteMD did not create its main window within 10 seconds");
  } finally {
    child.kill();
  }
}

const binaryPath = process.argv[2];
if (!binaryPath) {
  throw new Error("Usage: node tests/performance/startup.ts <notemd.exe>");
}

await measure(binaryPath);
