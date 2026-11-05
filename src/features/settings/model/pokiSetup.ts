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
    hint: "Install it from here, or choose a folder where you cloned it.",
  },
  "no-bridge": {
    label: "Not a Poki folder",
    hint: "There is no src/stdio.mjs in it. Pick the folder you cloned.",
  },
  stopped: {
    label: "Poki is not running",
    hint: "Start it here; MonoBOT stops it when you turn this off or quit.",
  },
  ready: {
    label: "Ready",
    hint: "Give the connector to a Phone bot in its settings.",
  },
};

/** What the app's Rust side reports about this Mac. */
export type PokiMachine = {
  /** macOS on Apple Silicon, which the published package needs. */
  supported: boolean;
  /** Tools Poki needs that this Mac lacks, as the user would install them. */
  missing: string[];
  /** The folder MonoBOT installed Poki in, if it did. */
  installed: string | null;
  /** Whether the daemon MonoBOT started still runs. */
  running: boolean;
  version: string;
  /** Download size in bytes. */
  size: number;
};

export type PokiInstallStage =
  "download" | "verify" | "unpack" | "dependencies" | "done";

export const POKI_STAGE_TEXT: Record<PokiInstallStage, string> = {
  download: "Downloading Poki…",
  verify: "Checking it is the published release…",
  unpack: "Unpacking into MonoBOT's folder…",
  dependencies: "Installing its packages (no install scripts)…",
  done: "Installed",
};

/** What turning the switch on does next. */
export type PokiNext =
  /** Not an Apple Silicon Mac: only the manual tutorial. */
  | { kind: "unsupported" }
  /** Install these first; MonoBOT never installs them. */
  | { kind: "missing"; missing: string[] }
  /** Ask, then download and verify the pinned release. */
  | { kind: "install" }
  /** Use the copy MonoBOT already installed. */
  | { kind: "use"; folder: string }
  /** Start the daemon from the saved folder. */
  | { kind: "start"; folder: string }
  | { kind: "ready" };

export function pokiNext(
  folder: string,
  status: PokiStatus,
  machine: PokiMachine,
): PokiNext {
  if (status === "ready") return { kind: "ready" };
  if (folder.trim() && status === "stopped")
    return { kind: "start", folder: folder.trim() };
  if (machine.installed) return { kind: "use", folder: machine.installed };
  // A folder that is not Poki's is the user's to fix in the field.
  if (folder.trim()) return { kind: "ready" };
  if (!machine.supported) return { kind: "unsupported" };
  if (machine.missing.length)
    return { kind: "missing", missing: machine.missing };
  return { kind: "install" };
}

/** The question before downloading anything. */
export function pokiInstallQuestion(machine: PokiMachine): string {
  const megabytes = Math.round(machine.size / 1_000_000);
  return [
    `Download Poki ${machine.version} (${megabytes} MB, AGPL-3.0) from its GitHub release?`,
    "MonoBOT checks it against the SHA-256 GitHub published and refuses anything else, unpacks it only inside its own data folder, and runs no install scripts. It needs no admin rights and changes nothing else on your Mac.",
    "Poki's daemon runs while this connector is on and stops when you turn it off or quit.",
  ].join("\n\n");
}

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
