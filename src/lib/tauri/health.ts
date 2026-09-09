import { invoke } from "@tauri-apps/api/core";

export type HealthResponse = Readonly<{
  app: "NoteMD";
  version: string;
  status: "ok";
}>;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function parseHealth(value: unknown): HealthResponse {
  if (!isRecord(value)) {
    throw new Error("Invalid health response");
  }

  const keys = Object.keys(value).sort();
  const hasExpectedShape =
    keys.join(",") === "app,status,version" &&
    value.app === "NoteMD" &&
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
