import { invoke } from "@tauri-apps/api/core";

/** Claude Code plugins MonoBOT ships in its bundle. Each one is written under
 * the app data folder once per launch and passed to Claude with
 * `--plugin-dir`, so its skills load in MonoBOT's sessions only and nothing
 * is installed into the user's own `~/.claude`. */
export type BundledPlugin =
  | "monobot"
  | "monobot-finance"
  | SkillPackId;

/** Optional packs of third-party skills; each one is off until the user (or
 * a bot's tools) turns it on. See `skillPacks.ts`. */
export type SkillPackId =
  | "monobot-design"
  | "monobot-imagery"
  | "monobot-dev"
  | "monobot-audio"
  | "monobot-openai";

type Loaders = Record<string, () => Promise<unknown>>;

// The desktop app bundled these files with Vite's import.meta.glob. The host
// runs without a window, where bundledPluginDir never installs a plugin, so it
// ships none of them.
const ALL: Loaders = {};

function sources(name: BundledPlugin): Loaders {
  const prefix = `../bundled/${name}/`;
  return Object.fromEntries(
    Object.entries(ALL).filter(([path]) => path.startsWith(prefix)),
  );
}

export async function bundledPluginFiles(
  name: BundledPlugin,
): Promise<{ path: string; content: string }[]> {
  const prefix = `../bundled/${name}/`;
  return Promise.all(
    Object.entries(sources(name)).map(async ([path, load]) => ({
      path: path.slice(prefix.length),
      content: String(await load()),
    })),
  );
}

const installs = new Map<BundledPlugin, Promise<string | null>>();

/** The installed folder of a bundled plugin, or null when it could not be
 * written (a turn still starts, just without those skills). */
export function bundledPluginDir(name: BundledPlugin): Promise<string | null> {
  // Outside the desktop app (tests, the browser) there is nowhere to install.
  if (typeof window === "undefined") return Promise.resolve(null);
  let install = installs.get(name);
  if (!install) {
    install = bundledPluginFiles(name)
      .then((files) => invoke<unknown>("install_plugin", { name, files }))
      .then((dir) => (typeof dir === "string" && dir ? dir : null))
      .catch((error: unknown) => {
        console.warn(`Could not install the ${name} plugin`, error);
        installs.delete(name);
        return null;
      });
    installs.set(name, install);
  }
  return install;
}

export async function bundledPluginDirs(
  names: readonly BundledPlugin[],
): Promise<string[]> {
  const dirs = await Promise.all(names.map(bundledPluginDir));
  return dirs.filter((dir): dir is string => dir !== null);
}
