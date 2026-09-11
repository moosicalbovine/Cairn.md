import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

if (process.platform !== "win32") {
  throw new Error("The forced-termination recovery test requires Windows");
}

const script = resolve("scripts/test/desktop-flow.ps1");
const harness = readFileSync(script, "utf8");
if (!harness.includes("[ValidateSet('workspace', 'recovery')]")) {
  throw new Error("The desktop harness does not implement the recovery scenario");
}
const result = spawnSync(
  "powershell.exe",
  [
    "-NoProfile",
    "-ExecutionPolicy",
    "Bypass",
    "-File",
    script,
    "-Scenario",
    "recovery",
  ],
  {
    stdio: "inherit",
    windowsHide: true,
  },
);

if (result.error) throw result.error;
if (result.status !== 0) process.exitCode = result.status ?? 1;
