import { IS_MAC, IS_WIN } from "../../../platform/tauri/platform";
import { readFlag, writeFlag } from "./storageFlags";

/** Built-in MCP servers that need no account: remote ones (Streamable HTTP)
 * and a few that run locally through `npx`. monoBOT passes the enabled ones
 * to each Claude turn with `--mcp-config` and to Codex as `-c` overrides, so
 * neither CLI's own configuration is touched. What they return is data,
 * never instructions. */
export type ConnectorId =
  | "context7"
  | "mslearn"
  | "deepwiki"
  | "xmd"
  | "openalex"
  | "cloudflare"
  | "huggingface"
  | "exa"
  | "playwright"
  | "poki";

export type ConnectorPlatform = "mac" | "windows" | "linux";

export type Connector = {
  id: ConnectorId;
  name: string;
  description: string;
  site: string;
  onByDefault: boolean;
  /** Reviewed read-only tools. Never allow a whole server: it may also write. */
  tools: readonly string[];
  /** Where it can run at all; unset, everywhere. Elsewhere it is not shown. */
  platforms?: readonly ConnectorPlatform[];
  /** Still changing from one version to the next; Settings marks it. */
  experimental?: boolean;
  /**
   * It acts on something real (a phone), so it reaches only a bot whose list
   * names it, and only the user puts it there: never a session without a
   * bot, nor a bot that has no list.
   */
  userGranted?: boolean;
  /**
   * Tools that act. They ask the user every time, whatever mode the session
   * runs in and whatever was allowed before.
   */
  ask?: readonly string[];
  /** Tools no bot may use at all. */
  denied?: readonly string[];
} & (
  | { url: string; command?: never; checkout?: never }
  /** Runs on this machine; the version is pinned so a turn never upgrades it. */
  | {
      url?: never;
      command: string;
      args: readonly string[];
      /**
       * A file in a checkout the user keeps on this computer, run after
       * `args`. The folder is a setting; without it the connector stays off.
       */
      checkout?: { script: string };
    }
);

export const CONNECTORS: readonly Connector[] = [
  {
    id: "context7",
    name: "Context7",
    description: "Up-to-date documentation for libraries and APIs.",
    url: "https://mcp.context7.com/mcp",
    site: "https://context7.com",
    onByDefault: true,
    tools: ["resolve-library-id", "query-docs"],
  },
  {
    id: "mslearn",
    name: "Microsoft Learn",
    description:
      "Official Microsoft docs: Windows, Office, Azure, .NET and PowerShell.",
    url: "https://learn.microsoft.com/api/mcp",
    site: "https://learn.microsoft.com/training/support/mcp",
    onByDefault: false,
    tools: [
      "microsoft_docs_search",
      "microsoft_docs_fetch",
      "microsoft_code_sample_search",
    ],
  },
  {
    id: "deepwiki",
    name: "DeepWiki",
    description: "Explains any public GitHub repository and answers questions.",
    url: "https://mcp.deepwiki.com/mcp",
    site: "https://deepwiki.com",
    onByDefault: false,
    tools: ["read_wiki_structure", "read_wiki_contents", "ask_question"],
  },
  {
    id: "xmd",
    name: "x.md",
    description: "Public X/Twitter posts, threads, profiles and searches.",
    url: "https://mdfromx.com/mcp",
    site: "https://mdfromx.com",
    onByDefault: true,
    tools: [
      "x_md_get_post",
      "x_md_get_profile",
      "x_md_search_posts",
      "x_md_get_followers",
      "x_md_get_following",
    ],
  },
  {
    id: "openalex",
    name: "OpenAlex",
    description:
      "Scholarly papers, authors, citations and trends. Community-hosted: your queries reach that server.",
    url: "https://openalex.caseyjhand.com/mcp",
    site: "https://github.com/cyanheads/openalex-mcp-server",
    onByDefault: false,
    tools: [
      "openalex_resolve_name",
      "openalex_search_entities",
      "openalex_analyze_trends",
      "openalex_get_citation_graph",
      "openalex_describe_fields",
    ],
  },
  {
    id: "cloudflare",
    name: "Cloudflare Docs",
    description:
      "Search Cloudflare's documentation: Workers, Pages, R2, D1 and more.",
    url: "https://docs.mcp.cloudflare.com/mcp",
    site: "https://github.com/cloudflare/mcp-server-cloudflare",
    onByDefault: false,
    tools: [
      "search_cloudflare_documentation",
      "migrate_pages_to_workers_guide",
    ],
  },
  {
    id: "huggingface",
    name: "Hugging Face",
    description: "Search models, datasets and Spaces on the Hugging Face Hub.",
    url: "https://huggingface.co/mcp",
    site: "https://huggingface.co/settings/mcp",
    onByDefault: false,
    tools: ["hub_repo_search", "hub_repo_details", "hf_fs"],
  },
  {
    id: "exa",
    name: "Exa",
    description:
      "Web search and page reading built for agents. Your queries reach Exa; it limits use without a key.",
    url: "https://mcp.exa.ai/mcp",
    site: "https://exa.ai",
    onByDefault: false,
    tools: ["web_search_exa", "web_fetch_exa"],
  },
  {
    id: "playwright",
    name: "Playwright",
    description:
      "A real browser on this computer to open, read and screenshot pages. Runs with npx; clicking and typing still ask.",
    command: "npx",
    args: ["-y", "@playwright/mcp@0.0.84"],
    site: "https://github.com/microsoft/playwright-mcp",
    onByDefault: false,
    tools: [
      "browser_navigate",
      "browser_navigate_back",
      "browser_snapshot",
      "browser_take_screenshot",
      "browser_console_messages",
      "browser_network_requests",
      "browser_resize",
      "browser_tabs",
      "browser_wait_for",
      "browser_close",
    ],
  },
  {
    id: "poki",
    name: "iPhone (Poki)",
    description:
      "Your iPhone or the iOS Simulator, through Poki on this Mac. Reading the screen runs on its own; every tap and keystroke asks you first.",
    command: "node",
    args: [],
    checkout: { script: "src/stdio.mjs" },
    site: "https://github.com/JosssphZhou/poki",
    onByDefault: false,
    platforms: ["mac"],
    experimental: true,
    userGranted: true,
    tools: [
      "poki_mcp_status",
      "list_devices",
      "select_device",
      "get_foreground_app",
      "list_elements",
      "take_screenshot",
      "list_apps",
      "wait_for",
    ],
    ask: [
      "tap",
      "tap_text",
      "double_tap",
      "long_press",
      "swipe",
      "scroll",
      "scroll_to",
      "drag",
      "pinch",
      "type_text",
      "fill",
      "press_key",
      "hide_keyboard",
      "press_button",
      "system_gesture",
      "go_back",
      "rotate",
      "launch_app",
      "terminate_app",
      "open_url",
      "open_settings",
      "handle_alert",
      "set_appearance",
      "set_accessibility",
      "set_location",
      "clear_location",
      "start_simulator",
      "shut_down_simulator",
      "request_human_action",
      "acquire_device",
      "renew_device",
      "release_device",
      "cancel_device",
    ],
    // Many steps under one approval, the clipboard (where codes land),
    // installing apps and pulling an app's data never reach a bot.
    denied: [
      "batch_commands",
      "get_clipboard",
      "set_clipboard",
      "install_app",
      "uninstall_app",
      "download_app_container",
      "build_and_run",
    ],
  },
];

const enabledKey = (id: ConnectorId) => `monocode.connector.${id}.enabled`;
const checkoutKey = (id: ConnectorId) => `monocode.connector.${id}.path`;

export function currentPlatform(): ConnectorPlatform {
  return IS_MAC ? "mac" : IS_WIN ? "windows" : "linux";
}

/** Whether it can run on this computer at all. */
export function connectorAvailable(
  connector: Connector,
  platform: ConnectorPlatform = currentPlatform(),
): boolean {
  return !connector.platforms || connector.platforms.includes(platform);
}

/** The connectors this computer can run, as Settings lists them. */
export function availableConnectors(
  platform: ConnectorPlatform = currentPlatform(),
): Connector[] {
  return CONNECTORS.filter((connector) =>
    connectorAvailable(connector, platform),
  );
}

/** The folder of a connector's local checkout, or "" when none is set. */
export function connectorCheckout(id: ConnectorId): string {
  try {
    return localStorage.getItem(checkoutKey(id))?.trim() ?? "";
  } catch {
    return "";
  }
}

export function setConnectorCheckout(id: ConnectorId, path: string) {
  try {
    const value = path.trim();
    if (value) localStorage.setItem(checkoutKey(id), value);
    else localStorage.removeItem(checkoutKey(id));
  } catch {
    // private mode / quota
  }
}

/** A file inside a checkout folder, whichever slash the folder ends with. */
export function checkoutFile(folder: string, script: string): string {
  return `${folder.trim().replace(/[\\/]+$/, "")}/${script}`;
}

/** The switch in Settings: on or off, whether or not it can run yet. */
export function isConnectorEnabled(connector: Connector): boolean {
  return readFlag(enabledKey(connector.id)) ?? connector.onByDefault;
}

export function setConnectorEnabled(id: ConnectorId, enabled: boolean) {
  writeFlag(enabledKey(id), enabled);
}

/** Switched on, on a computer that runs it, with its checkout set. */
export function isConnectorReady(
  connector: Connector,
  platform: ConnectorPlatform = currentPlatform(),
): boolean {
  return (
    isConnectorEnabled(connector) &&
    connectorAvailable(connector, platform) &&
    (!connector.checkout || connectorCheckout(connector.id) !== "")
  );
}

export function enabledConnectors(
  platform: ConnectorPlatform = currentPlatform(),
): Connector[] {
  return CONNECTORS.filter((connector) => isConnectorReady(connector, platform));
}

/**
 * Whether a bot's list lets a connector through: unset, every one but those
 * the user grants by name.
 */
export function connectorInList(
  connector: Connector,
  list: readonly string[] | undefined,
): boolean {
  return list ? list.includes(connector.id) : !connector.userGranted;
}

/**
 * The enabled connectors a session gets. `only` is a bot's list; a session
 * without a bot passes nothing and never gets a user-granted connector.
 */
export function sessionConnectors(
  only?: readonly string[],
  platform: ConnectorPlatform = currentPlatform(),
): Connector[] {
  return enabledConnectors(platform).filter((connector) =>
    connectorInList(connector, only),
  );
}

/**
 * The user-granted connectors `next` lets through that `before` did not: a
 * bot may keep them, never add them.
 */
export function userGrantedAdded(
  next: readonly string[] | undefined,
  before: readonly string[] | undefined,
): Connector[] {
  return CONNECTORS.filter(
    (connector) =>
      connector.userGranted &&
      connectorInList(connector, next) &&
      !connectorInList(connector, before),
  );
}

/** The command's arguments, with the checkout's file when it has one. */
export function connectorArgs(connector: Connector): string[] {
  if (!connector.command) return [];
  const args = [...connector.args];
  if (connector.checkout)
    args.push(
      checkoutFile(connectorCheckout(connector.id), connector.checkout.script),
    );
  return args;
}

/** The `--mcp-config` value for these connectors, or null when there are none. */
export function connectorMcpConfig(connectors: readonly Connector[]) {
  if (connectors.length === 0) return null;
  return {
    mcpServers: Object.fromEntries(
      connectors.map((connector) => [
        connector.id,
        connector.command
          ? {
              type: "stdio",
              command: connector.command,
              args: connectorArgs(connector),
            }
          : { type: "http", url: connector.url },
      ]),
    ),
  };
}

/** Claude tool names (`mcp__<server>__<tool>`) the turn may call unprompted. */
export function connectorAllowedTools(connectors: readonly Connector[]) {
  return connectors.flatMap((connector) =>
    connector.tools.map((tool) => `mcp__${connector.id}__${tool}`),
  );
}

/** Acting tools that ask every time, as `permissions.ask` rules. */
export function connectorAskTools(connectors: readonly Connector[]) {
  return connectors.flatMap((connector) =>
    (connector.ask ?? []).map((tool) => `mcp__${connector.id}__${tool}`),
  );
}

/** Tools no bot may use, whichever connectors it has. */
export function connectorDeniedTools(
  connectors: readonly Connector[] = CONNECTORS,
) {
  return connectors.flatMap((connector) =>
    (connector.denied ?? []).map((tool) => `mcp__${connector.id}__${tool}`),
  );
}

/**
 * A session with a connector that acts must ask before each action, so the
 * modes that approve on their own (auto's classifier, bypass) fall back to
 * the default one; plan and accept-edits already ask for MCP tools.
 */
export function askingPermissionMode<Mode extends string>(
  mode: Mode,
  connectors: readonly Connector[],
): Mode | "default" {
  const acts = connectors.some((connector) => connector.ask?.length);
  return acts && (mode === "auto" || mode === "bypassPermissions")
    ? "default"
    : mode;
}

/**
 * The same servers as Codex `-c` overrides (`mcp_servers.<id>.url=...`),
 * each limited to its reviewed tools. Values are TOML, so strings are quoted.
 */
export function connectorCodexArgs(connectors: readonly Connector[]): string[] {
  const toml = (value: string) => JSON.stringify(value);
  const list = (values: readonly string[]) => `[${values.map(toml).join(",")}]`;
  return connectors.flatMap((connector) => {
    const key = `mcp_servers.${connector.id}`;
    const where = connector.command
      ? [
          "-c",
          `${key}.command=${toml(connector.command)}`,
          "-c",
          `${key}.args=${list(connectorArgs(connector))}`,
        ]
      : ["-c", `${key}.url=${toml(connector.url ?? "")}`];
    return [...where, "-c", `${key}.enabled_tools=${list(connector.tools)}`];
  });
}
