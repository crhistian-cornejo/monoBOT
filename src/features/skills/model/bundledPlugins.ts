import { invoke } from "@tauri-apps/api/core";

/** Claude Code plugins MonoBOT ships in its bundle. Each one is written under
 * the app data folder once per launch and passed to Claude with
 * `--plugin-dir`, so its skills load in MonoBOT's sessions only and nothing
 * is installed into the user's own `~/.claude`. */
export type BundledPlugin = "monobot" | "monobot-finance";

type Loaders = Record<string, () => Promise<unknown>>;

// Dot folders are not matched by `**`, so each manifest is listed explicitly.
const SOURCES: Record<BundledPlugin, Loaders> = {
  monobot: {
    ...import.meta.glob("../bundled/monobot/.claude-plugin/plugin.json", {
      query: "?raw",
      import: "default",
    }),
    ...import.meta.glob("../bundled/monobot/skills/**/*.md", {
      query: "?raw",
      import: "default",
    }),
  },
  "monobot-finance": {
    ...import.meta.glob(
      "../bundled/monobot-finance/.claude-plugin/plugin.json",
      { query: "?raw", import: "default" },
    ),
    ...import.meta.glob("../bundled/monobot-finance/skills/**/*.md", {
      query: "?raw",
      import: "default",
    }),
  },
};

export async function bundledPluginFiles(
  name: BundledPlugin,
): Promise<{ path: string; content: string }[]> {
  const prefix = `../bundled/${name}/`;
  return Promise.all(
    Object.entries(SOURCES[name]).map(async ([path, load]) => ({
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
