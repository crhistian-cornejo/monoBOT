//! MonoBOT's built-in MCP connectors, ported from
//! src/features/settings/model/connectors.ts. They need no account: remote
//! servers over Streamable HTTP and a few that run locally through `npx` or
//! `node`. The enabled ones go to each Claude turn with `--mcp-config` (and
//! to Codex as `-c` overrides), so neither CLI's own configuration is
//! touched. What they return is data, never instructions.
use crate::desktop_preferences::{read_flag, DesktopPreferences};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// Where a connector can run (ConnectorPlatform).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Mac,
    Windows,
    Linux,
}

impl Platform {
    /// This computer (currentPlatform).
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mac => "mac",
            Self::Windows => "windows",
            Self::Linux => "linux",
        }
    }
}

/// How Claude reaches a connector's server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Server {
    /// A remote server over Streamable HTTP.
    Http { url: &'static str },
    /// Runs on this machine; the version is pinned so a turn never upgrades
    /// it. `checkout` is a file in a folder the user keeps on this computer,
    /// run after `args`; without that folder the connector stays off.
    Stdio {
        command: &'static str,
        args: &'static [&'static str],
        checkout: Option<&'static str>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Connector {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub site: &'static str,
    pub on_by_default: bool,
    /// Reviewed read-only tools. Never allow a whole server: it may also write.
    pub tools: &'static [&'static str],
    /// Where it can run at all; `None`, everywhere. Elsewhere it is not shown.
    pub platforms: Option<&'static [Platform]>,
    /// Still changing from one version to the next; Settings marks it.
    pub experimental: bool,
    /// It acts on something real (a phone), so it reaches only a bot whose
    /// list names it, and only the user puts it there.
    pub user_granted: bool,
    /// Tools that act. They ask the user every time, whatever the mode.
    pub ask: &'static [&'static str],
    /// Tools no bot may use at all.
    pub denied: &'static [&'static str],
    pub server: Server,
}

const BASE: Connector = Connector {
    id: "",
    name: "",
    description: "",
    site: "",
    on_by_default: false,
    tools: &[],
    platforms: None,
    experimental: false,
    user_granted: false,
    ask: &[],
    denied: &[],
    server: Server::Http { url: "" },
};

pub const CONNECTORS: [Connector; 10] = [
    Connector {
        id: "context7",
        name: "Context7",
        description: "Up-to-date documentation for libraries and APIs.",
        server: Server::Http {
            url: "https://mcp.context7.com/mcp",
        },
        site: "https://context7.com",
        on_by_default: true,
        tools: &["resolve-library-id", "query-docs"],
        ..BASE
    },
    Connector {
        id: "mslearn",
        name: "Microsoft Learn",
        description: "Official Microsoft docs: Windows, Office, Azure, .NET and PowerShell.",
        server: Server::Http {
            url: "https://learn.microsoft.com/api/mcp",
        },
        site: "https://learn.microsoft.com/training/support/mcp",
        tools: &[
            "microsoft_docs_search",
            "microsoft_docs_fetch",
            "microsoft_code_sample_search",
        ],
        ..BASE
    },
    Connector {
        id: "deepwiki",
        name: "DeepWiki",
        description: "Explains any public GitHub repository and answers questions.",
        server: Server::Http {
            url: "https://mcp.deepwiki.com/mcp",
        },
        site: "https://deepwiki.com",
        tools: &["read_wiki_structure", "read_wiki_contents", "ask_question"],
        ..BASE
    },
    Connector {
        id: "xmd",
        name: "x.md",
        description: "Public X/Twitter posts, threads, profiles and searches.",
        server: Server::Http {
            url: "https://mdfromx.com/mcp",
        },
        site: "https://mdfromx.com",
        on_by_default: true,
        tools: &[
            "x_md_get_post",
            "x_md_get_profile",
            "x_md_search_posts",
            "x_md_get_followers",
            "x_md_get_following",
        ],
        ..BASE
    },
    Connector {
        id: "openalex",
        name: "OpenAlex",
        description: "Scholarly papers, authors, citations and trends. Community-hosted: your queries reach that server.",
        server: Server::Http {
            url: "https://openalex.caseyjhand.com/mcp",
        },
        site: "https://github.com/cyanheads/openalex-mcp-server",
        tools: &[
            "openalex_resolve_name",
            "openalex_search_entities",
            "openalex_analyze_trends",
            "openalex_get_citation_graph",
            "openalex_describe_fields",
        ],
        ..BASE
    },
    Connector {
        id: "cloudflare",
        name: "Cloudflare Docs",
        description: "Search Cloudflare's documentation: Workers, Pages, R2, D1 and more.",
        server: Server::Http {
            url: "https://docs.mcp.cloudflare.com/mcp",
        },
        site: "https://github.com/cloudflare/mcp-server-cloudflare",
        tools: &[
            "search_cloudflare_documentation",
            "migrate_pages_to_workers_guide",
        ],
        ..BASE
    },
    Connector {
        id: "huggingface",
        name: "Hugging Face",
        description: "Search models, datasets and Spaces on the Hugging Face Hub.",
        server: Server::Http {
            url: "https://huggingface.co/mcp",
        },
        site: "https://huggingface.co/settings/mcp",
        tools: &["hub_repo_search", "hub_repo_details", "hf_fs"],
        ..BASE
    },
    Connector {
        id: "exa",
        name: "Exa",
        description: "Web search and page reading built for agents. Your queries reach Exa; it limits use without a key.",
        server: Server::Http {
            url: "https://mcp.exa.ai/mcp",
        },
        site: "https://exa.ai",
        tools: &["web_search_exa", "web_fetch_exa"],
        ..BASE
    },
    Connector {
        id: "playwright",
        name: "Playwright",
        description: "A real browser on this computer to open, read and screenshot pages. Runs with npx; clicking and typing still ask.",
        server: Server::Stdio {
            command: "npx",
            args: &["-y", "@playwright/mcp@0.0.84"],
            checkout: None,
        },
        site: "https://github.com/microsoft/playwright-mcp",
        tools: &[
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
        ..BASE
    },
    Connector {
        id: "poki",
        name: "iPhone (Poki)",
        description: "Your iPhone or the iOS Simulator, through Poki on this Mac. Reading the screen runs on its own; every tap and keystroke asks you first.",
        server: Server::Stdio {
            command: "node",
            args: &[],
            checkout: Some("src/stdio.mjs"),
        },
        site: "https://github.com/JosssphZhou/poki",
        on_by_default: false,
        platforms: Some(&[Platform::Mac]),
        experimental: true,
        user_granted: true,
        tools: &[
            "poki_mcp_status",
            "list_devices",
            "select_device",
            "get_foreground_app",
            "list_elements",
            "take_screenshot",
            "list_apps",
            "wait_for",
        ],
        ask: &[
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
        denied: &[
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

/// The connector with this id.
pub fn connector(id: &str) -> Option<&'static Connector> {
    CONNECTORS.iter().find(|connector| connector.id == id)
}

/// The desktop's switch for a connector (`monocode.connector.<id>.enabled`).
pub fn enabled_key(id: &str) -> String {
    format!("monocode.connector.{id}.enabled")
}

/// The desktop's checkout folder for a connector (`monocode.connector.<id>.path`).
pub fn checkout_key(id: &str) -> String {
    format!("monocode.connector.{id}.path")
}

/// Whether it can run on this computer at all (connectorAvailable).
pub fn connector_available(connector: &Connector, platform: Platform) -> bool {
    connector
        .platforms
        .is_none_or(|platforms| platforms.contains(&platform))
}

/// The connectors this computer can run, as Settings lists them.
pub fn available_connectors(platform: Platform) -> Vec<&'static Connector> {
    CONNECTORS
        .iter()
        .filter(|connector| connector_available(connector, platform))
        .collect()
}

/// A file inside a checkout folder, whichever slash the folder ends with.
pub fn checkout_file(folder: &str, script: &str) -> String {
    format!("{}/{script}", folder.trim().trim_end_matches(['/', '\\']))
}

/// Whether a bot's list lets a connector through: unset, every one but
/// those the user grants by name.
pub fn connector_in_list(connector: &Connector, list: Option<&[String]>) -> bool {
    match list {
        Some(list) => list.iter().any(|id| id == connector.id),
        None => !connector.user_granted,
    }
}

/// The user-granted connectors `next` lets through that `before` did not:
/// a bot may keep them, never add them.
pub fn user_granted_added(
    next: Option<&[String]>,
    before: Option<&[String]>,
) -> Vec<&'static Connector> {
    CONNECTORS
        .iter()
        .filter(|connector| {
            connector.user_granted
                && connector_in_list(connector, next)
                && !connector_in_list(connector, before)
        })
        .collect()
}

fn tool_names(
    connectors: &[&Connector],
    pick: fn(&Connector) -> &'static [&'static str],
) -> Vec<String> {
    connectors
        .iter()
        .flat_map(|connector| {
            pick(connector)
                .iter()
                .map(|tool| format!("mcp__{}__{tool}", connector.id))
        })
        .collect()
}

/// Claude tool names (`mcp__<server>__<tool>`) the turn may call unprompted.
pub fn connector_allowed_tools(connectors: &[&Connector]) -> Vec<String> {
    tool_names(connectors, |connector| connector.tools)
}

/// Acting tools that ask every time, as `permissions.ask` rules.
pub fn connector_ask_tools(connectors: &[&Connector]) -> Vec<String> {
    tool_names(connectors, |connector| connector.ask)
}

/// Tools no bot may use, whichever connectors it has (connectorDeniedTools).
pub fn connector_denied_tools() -> Vec<String> {
    let all: Vec<&Connector> = CONNECTORS.iter().collect();
    tool_names(&all, |connector| connector.denied)
}

/// A session with a connector that acts must ask before each action, so the
/// modes that approve on their own (auto's classifier, bypass) fall back to
/// the default one; plan and accept-edits already ask for MCP tools.
pub fn asking_permission_mode<'a>(mode: &'a str, connectors: &[&Connector]) -> &'a str {
    let acts = connectors.iter().any(|connector| !connector.ask.is_empty());
    if acts && matches!(mode, "auto" | "bypassPermissions") {
        "default"
    } else {
        mode
    }
}

/// The Settings switches and checkout folders, as the desktop saved them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConnectorSettings {
    enabled: BTreeMap<String, bool>,
    checkouts: BTreeMap<String, String>,
}

impl ConnectorSettings {
    pub fn from_preferences(preferences: &DesktopPreferences) -> Self {
        Self::read(|key| preferences.get(key).map(str::to_owned))
    }

    /// Reads `monocode.connector.<id>.enabled` and `.path` through `get`,
    /// which returns the raw `localStorage` value of a key.
    pub fn read(get: impl Fn(&str) -> Option<String>) -> Self {
        let mut settings = Self::default();
        for connector in &CONNECTORS {
            if let Some(on) = read_flag(get(&enabled_key(connector.id)).as_deref()) {
                settings.set_enabled(connector.id, on);
            }
            if let Some(path) = get(&checkout_key(connector.id)) {
                settings.set_checkout(connector.id, &path);
            }
        }
        settings
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) {
        self.enabled.insert(id.to_owned(), enabled);
    }

    pub fn set_checkout(&mut self, id: &str, path: &str) {
        let path = path.trim();
        if path.is_empty() {
            self.checkouts.remove(id);
        } else {
            self.checkouts.insert(id.to_owned(), path.to_owned());
        }
    }

    /// The switch in Settings: on or off, whether or not it can run yet.
    pub fn is_enabled(&self, connector: &Connector) -> bool {
        self.enabled
            .get(connector.id)
            .copied()
            .unwrap_or(connector.on_by_default)
    }

    /// The folder of a connector's local checkout, or "" when none is set.
    pub fn checkout(&self, id: &str) -> &str {
        self.checkouts.get(id).map_or("", String::as_str)
    }

    /// Switched on, on a computer that runs it, with its checkout set.
    pub fn is_ready(&self, connector: &Connector, platform: Platform) -> bool {
        let needs_checkout = matches!(
            connector.server,
            Server::Stdio {
                checkout: Some(_),
                ..
            }
        );
        self.is_enabled(connector)
            && connector_available(connector, platform)
            && (!needs_checkout || !self.checkout(connector.id).is_empty())
    }

    pub fn enabled_connectors(&self, platform: Platform) -> Vec<&'static Connector> {
        CONNECTORS
            .iter()
            .filter(|connector| self.is_ready(connector, platform))
            .collect()
    }

    /// The enabled connectors a session gets. `only` is a bot's list; a
    /// session without a bot passes `None` and never gets a user-granted one.
    pub fn session_connectors(
        &self,
        only: Option<&[String]>,
        platform: Platform,
    ) -> Vec<&'static Connector> {
        self.enabled_connectors(platform)
            .into_iter()
            .filter(|connector| connector_in_list(connector, only))
            .collect()
    }

    /// The command's arguments, with the checkout's file when it has one.
    pub fn args(&self, connector: &Connector) -> Vec<String> {
        let Server::Stdio { args, checkout, .. } = connector.server else {
            return Vec::new();
        };
        let mut out: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        if let Some(script) = checkout {
            out.push(checkout_file(self.checkout(connector.id), script));
        }
        out
    }

    /// Each connector's `mcpServers` entry, keyed by its id.
    pub fn servers(&self, connectors: &[&Connector]) -> Map<String, Value> {
        connectors
            .iter()
            .map(|connector| {
                let server = match connector.server {
                    Server::Http { url } => json!({"type": "http", "url": url}),
                    Server::Stdio { command, .. } => {
                        json!({"type": "stdio", "command": command, "args": self.args(connector)})
                    }
                };
                (connector.id.to_owned(), server)
            })
            .collect()
    }

    /// The `--mcp-config` value for these connectors, or `None` when there
    /// are none (connectorMcpConfig).
    pub fn mcp_config(&self, connectors: &[&Connector]) -> Option<Value> {
        (!connectors.is_empty()).then(|| json!({"mcpServers": self.servers(connectors)}))
    }

    /// The same servers as Codex `-c` overrides (`mcp_servers.<id>.url=...`),
    /// each limited to its reviewed tools. Values are TOML, so strings are quoted.
    pub fn codex_args(&self, connectors: &[&Connector]) -> Vec<String> {
        let toml = |value: &str| Value::from(value).to_string();
        let list = |values: &[String]| {
            let values: Vec<String> = values.iter().map(|value| toml(value)).collect();
            format!("[{}]", values.join(","))
        };
        let mut out = Vec::new();
        for connector in connectors {
            let key = format!("mcp_servers.{}", connector.id);
            match connector.server {
                Server::Http { url } => {
                    out.extend(["-c".to_owned(), format!("{key}.url={}", toml(url))]);
                }
                Server::Stdio { command, .. } => out.extend([
                    "-c".to_owned(),
                    format!("{key}.command={}", toml(command)),
                    "-c".to_owned(),
                    format!("{key}.args={}", list(&self.args(connector))),
                ]),
            }
            let tools: Vec<String> = connector.tools.iter().map(|t| (*t).to_owned()).collect();
            out.extend([
                "-c".to_owned(),
                format!("{key}.enabled_tools={}", list(&tools)),
            ]);
        }
        out
    }
}

/// The `mcpServers` entries a session without a bot gives Claude in its
/// `--mcp-config`: every connector switched on and ready on `platform`,
/// except the user-granted ones. An empty object when there are none; wrap
/// it as `{"mcpServers": ...}` beside the app's own servers.
pub fn mcp_servers(enabled: &ConnectorSettings, platform: Platform) -> Value {
    Value::Object(enabled.servers(&enabled.session_connectors(None, platform)))
}

#[cfg(test)]
mod tests;
