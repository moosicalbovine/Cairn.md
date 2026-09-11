import { invoke } from "@tauri-apps/api/core";

import { isRecord } from "../validation";

export type HealthResponse = Readonly<{
  app: "Cairn.md";
  version: string;
  status: "ok";
}>;

export function parseHealth(value: unknown): HealthResponse {
  if (!isRecord(value)) {
    throw new Error("Invalid health response");
  }

  const keys = Object.keys(value).sort();
  const hasExpectedShape =
    keys.join(",") === "app,status,version" &&
    value.app === "Cairn.md" &&
    typeof value.version === "string" &&
    value.version.length > 0 &&
    value.status === "ok";

  if (!hasExpectedShape) {
    throw new Error("Invalid health response");
  }

  return value as HealthResponse;
}

export async function getHealth(): Promise<HealthResponse> {
  return parseHealth(await invoke<unknown>("health"));
}
