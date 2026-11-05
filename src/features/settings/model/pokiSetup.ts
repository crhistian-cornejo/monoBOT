import { checkoutFile } from "./connectors";

/**
 * Where Poki's daemon (`npm start`) writes its address and token while it
 * runs, and removes them when it stops. Its stdio bridge reads the same file,
 * so without it every phone tool fails.
 */
export const POKI_ENDPOINT_FILE =
  "~/Library/Application Support/PokiMCP/mcp.json";

export const POKI_BRIDGE = "src/stdio.mjs";

export type PokiStatus =
  /** No folder saved yet. */
  | "no-folder"
  /** The folder has no `src/stdio.mjs`: not a Poki checkout. */
  | "no-bridge"
  /** The daemon is not running (`npm start`). */
  | "stopped"
  | "ready";

export type PokiIO = {
  /** Whether a file exists. */
  exists(path: string): Promise<boolean>;
  read(path: string): Promise<string>;
};

/**
 * Claude runs `node <folder>/src/stdio.mjs` without a shell, so `~` is not
 * expanded there: the folder is saved with the home folder spelled out.
 */
export function expandHome(path: string, home: string): string {
  const trimmed = path.trim();
  if (trimmed === "~") return home;
  return trimmed.startsWith("~/")
    ? `${home.replace(/\/+$/, "")}/${trimmed.slice(2)}`
    : trimmed;
}

/** Why a folder cannot be Poki's, if it cannot. */
export function pokiFolderProblem(path: string): string | undefined {
  const trimmed = path.trim();
  if (!trimmed) return undefined;
  if (!trimmed.startsWith("/") && !trimmed.startsWith("~/"))
    return "Use the folder's full path, such as ~/poki";
  return undefined;
}

/** What each step of the setup tells the user. */
export const POKI_STATUS_TEXT: Record<
  PokiStatus,
  { label: string; hint: string }
> = {
  "no-folder": {
    label: "Not set up",
    hint: "Choose the folder where you cloned Poki.",
  },
  "no-bridge": {
    label: "Not a Poki folder",
    hint: "There is no src/stdio.mjs in it. Pick the folder you cloned.",
  },
  stopped: {
    label: "Poki is not running",
    hint: "Run npm start in the Poki folder and keep that terminal open.",
  },
  ready: {
    label: "Ready",
    hint: "Give the connector to a Phone bot in its settings.",
  },
};

/** Whether the daemon's file names a local endpoint, as the bridge checks. */
export function pokiEndpointLive(raw: string): boolean {
  try {
    const entry = JSON.parse(raw) as { enabled?: unknown; url?: unknown };
    if (entry.enabled !== true || typeof entry.url !== "string") return false;
    const url = new URL(entry.url);
    return (
      url.protocol === "http:" &&
      url.hostname === "127.0.0.1" &&
      url.pathname === "/mcp"
    );
  } catch {
    return false;
  }
}

/**
 * How far the setup got. A daemon that crashed can leave its file behind;
 * the bridge then says it is not running, and so does the bot.
 */
export async function pokiStatus(
  folder: string,
  io: PokiIO,
): Promise<PokiStatus> {
  if (!folder.trim()) return "no-folder";
  const bridge = await io
    .exists(checkoutFile(folder, POKI_BRIDGE))
    .catch(() => false);
  if (!bridge) return "no-bridge";
  const raw = await io.read(POKI_ENDPOINT_FILE).catch(() => "");
  return pokiEndpointLive(raw) ? "ready" : "stopped";
}
