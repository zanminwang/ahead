import * as FileSystem from "expo-file-system/legacy";

/** Demo identity and backend selection. Development configuration, not authentication. */
export interface LaunchConfig {
  user: "alice" | "bob";
  url: string;
  /** Test-only: when set, the diagnostic runner drives this phase instead of the screen. */
  phase?: string;
  expectedClientId?: string;
}

const defaults: LaunchConfig = { user: "alice", url: "http://127.0.0.1:4242" };
export const configPath = `${FileSystem.documentDirectory}config.json`;

/** Reads Documents/config.json written by launch tooling; falls back to Alice on the local backend. */
export async function loadConfig(): Promise<LaunchConfig> {
  let raw: string;
  try {
    raw = await FileSystem.readAsStringAsync(configPath);
  } catch {
    return defaults;
  }
  const parsed = JSON.parse(raw) as Partial<LaunchConfig>;
  if (parsed.user !== "alice" && parsed.user !== "bob")
    throw Error(`config.json user must be alice or bob, got ${String(parsed.user)}`);
  if (typeof parsed.url !== "string" || !/^https?:\/\//.test(parsed.url))
    throw Error("config.json url must be an http(s) URL");
  return {
    user: parsed.user,
    url: parsed.url,
    ...(typeof parsed.phase === "string" ? { phase: parsed.phase } : {}),
    ...(typeof parsed.expectedClientId === "string"
      ? { expectedClientId: parsed.expectedClientId }
      : {}),
  };
}
