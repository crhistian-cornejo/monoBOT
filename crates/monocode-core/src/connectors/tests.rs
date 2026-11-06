//! The cases of src/features/settings/model/connectors.test.ts, plus checks
//! that the registry and its storage keys still match the TypeScript source.
use super::*;
use std::collections::HashMap;

const SOURCE: &str = include_str!("../../../../src/features/settings/model/connectors.ts");
const FLAGS: &str = include_str!("../../../../src/features/settings/model/storageFlags.ts");

/// Settings as the desktop's `localStorage` holds them.
fn saved(entries: &[(&str, &str)]) -> ConnectorSettings {
    let values: HashMap<String, String> = entries
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect();
    ConnectorSettings::read(|key| values.get(key).cloned())
}

fn by_id(ids: &[&str]) -> Vec<&'static Connector> {
    CONNECTORS
        .iter()
        .filter(|connector| ids.contains(&connector.id))
        .collect()
}

fn ids(connectors: &[&Connector]) -> Vec<&'static str> {
    connectors.iter().map(|connector| connector.id).collect()
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

/// A Rust connector in the shape of its TypeScript object literal.
fn as_ts(connector: &Connector) -> Value {
    let mut value = json!({
        "id": connector.id,
        "name": connector.name,
        "description": connector.description,
        "site": connector.site,
        "onByDefault": connector.on_by_default,
        "tools": connector.tools,
    });
    match connector.server {
        Server::Http { url } => value["url"] = json!(url),
        Server::Stdio {
            command,
            args,
            checkout,
        } => {
            value["command"] = json!(command);
            value["args"] = json!(args);
            if let Some(script) = checkout {
                value["checkout"] = json!({"script": script});
            }
        }
    }
    if let Some(platforms) = connector.platforms {
        let names: Vec<&str> = platforms.iter().map(|p| p.as_str()).collect();
        value["platforms"] = json!(names);
    }
    for (key, on) in [
        ("experimental", connector.experimental),
        ("userGranted", connector.user_granted),
    ] {
        if on {
            value[key] = json!(true);
        }
    }
    for (key, tools) in [("ask", connector.ask), ("denied", connector.denied)] {
        if !tools.is_empty() {
            value[key] = json!(tools);
        }
    }
    value
}

#[test]
fn registry_matches_the_typescript_connectors() {
    let source = crate::ts_literal::parse_const(SOURCE, "CONNECTORS");
    let source = source.as_array().unwrap();
    assert_eq!(
        source.iter().map(|c| c["id"].clone()).collect::<Vec<_>>(),
        CONNECTORS.iter().map(|c| json!(c.id)).collect::<Vec<_>>()
    );
    for (ts, rust) in source.iter().zip(&CONNECTORS) {
        assert_eq!(ts, &as_ts(rust), "{}", rust.id);
    }
}

#[test]
fn storage_keys_and_flags_match_the_desktop() {
    assert!(SOURCE.contains("`monocode.connector.${id}.enabled`"));
    assert!(SOURCE.contains("`monocode.connector.${id}.path`"));
    assert!(SOURCE.contains("readFlag(enabledKey(connector.id)) ?? connector.onByDefault"));
    assert!(SOURCE.contains("localStorage.getItem(checkoutKey(id))?.trim()"));
    assert!(FLAGS.contains(r#"return raw === "1" || raw === "true";"#));
    assert_eq!(enabled_key("xmd"), "monocode.connector.xmd.enabled");
    assert_eq!(checkout_key("poki"), "monocode.connector.poki.path");

    let platform = Platform::Mac;
    let on = |raw: &str| {
        saved(&[("monocode.connector.exa.enabled", raw)])
            .enabled_connectors(platform)
            .iter()
            .any(|c| c.id == "exa")
    };
    assert!(on("1"));
    assert!(on("true"));
    assert!(!on("0"));
    assert!(!on("false"));
    assert!(!on("yes"));
    // A saved "off" beats the default.
    let off = saved(&[("monocode.connector.xmd.enabled", "0")]);
    assert_eq!(ids(&off.enabled_connectors(platform)), ["context7"]);
}

#[test]
fn reads_the_switches_from_desktop_preferences() {
    let preferences = DesktopPreferences::from_values(&[
        ("monocode.connector.deepwiki.enabled", json!(1)),
        ("monocode.connector.context7.enabled", json!(false)),
    ]);
    let settings = ConnectorSettings::from_preferences(&preferences);
    assert_eq!(
        ids(&settings.enabled_connectors(Platform::Linux)),
        ["deepwiki", "xmd"]
    );
}

#[test]
fn enables_the_on_by_default_connectors_when_nothing_is_saved() {
    let settings = ConnectorSettings::default();
    for platform in [Platform::Mac, Platform::Windows, Platform::Linux] {
        assert_eq!(
            ids(&settings.enabled_connectors(platform)),
            ["context7", "xmd"]
        );
    }
}

#[test]
fn builds_an_http_mcp_config_and_allows_only_the_reviewed_tools() {
    let connectors = by_id(&["context7", "deepwiki"]);
    let settings = ConnectorSettings::default();
    assert_eq!(
        settings.mcp_config(&connectors),
        Some(json!({
            "mcpServers": {
                "context7": {"type": "http", "url": "https://mcp.context7.com/mcp"},
                "deepwiki": {"type": "http", "url": "https://mcp.deepwiki.com/mcp"},
            }
        }))
    );
    assert_eq!(
        connector_allowed_tools(&connectors),
        [
            "mcp__context7__resolve-library-id",
            "mcp__context7__query-docs",
            "mcp__deepwiki__read_wiki_structure",
            "mcp__deepwiki__read_wiki_contents",
            "mcp__deepwiki__ask_question",
        ]
    );
}

#[test]
fn keeps_the_community_hosted_openalex_connector_off_until_the_user_opts_in() {
    let openalex = connector("openalex").unwrap();
    assert!(!openalex.on_by_default);
    assert!(matches!(openalex.server, Server::Http { url } if url.starts_with("https://")));
    let expected: Vec<String> = openalex
        .tools
        .iter()
        .map(|tool| format!("mcp__openalex__{tool}"))
        .collect();
    assert_eq!(connector_allowed_tools(&[openalex]), expected);
}

#[test]
fn adds_nothing_when_every_connector_is_off() {
    let settings = ConnectorSettings::default();
    assert_eq!(settings.mcp_config(&[]), None);
    assert!(connector_allowed_tools(&[]).is_empty());
    let mut off = settings.clone();
    off.set_enabled("context7", false);
    off.set_enabled("xmd", false);
    assert_eq!(mcp_servers(&off, Platform::Mac), json!({}));
}

#[test]
fn a_session_without_a_bot_gets_the_ready_servers() {
    let mut settings = saved(&[
        ("monocode.connector.playwright.enabled", "1"),
        ("monocode.connector.poki.enabled", "1"),
        ("monocode.connector.poki.path", "/Users/me/poki"),
    ]);
    assert_eq!(
        mcp_servers(&settings, Platform::Mac),
        json!({
            "context7": {"type": "http", "url": "https://mcp.context7.com/mcp"},
            "xmd": {"type": "http", "url": "https://mdfromx.com/mcp"},
            "playwright": {"type": "stdio", "command": "npx", "args": ["-y", "@playwright/mcp@0.0.84"]},
        })
    );
    settings.set_enabled("xmd", false);
    let servers = mcp_servers(&settings, Platform::Windows);
    let servers = servers.as_object().unwrap();
    assert_eq!(
        servers.keys().collect::<Vec<_>>(),
        ["context7", "playwright"]
    );
}

#[test]
fn adds_more_connectors_all_off() {
    for connector in by_id(&["cloudflare", "huggingface", "exa", "playwright", "mslearn"]) {
        assert!(!connector.on_by_default, "{}", connector.id);
    }
}

#[test]
fn runs_playwright_locally_at_a_pinned_version() {
    let playwright = by_id(&["playwright"]);
    assert_eq!(
        ConnectorSettings::default().mcp_config(&playwright),
        Some(json!({
            "mcpServers": {
                "playwright": {
                    "type": "stdio",
                    "command": "npx",
                    "args": ["-y", "@playwright/mcp@0.0.84"],
                }
            }
        }))
    );
    // Clicking and typing are not pre-approved.
    assert!(
        !connector_allowed_tools(&playwright).contains(&"mcp__playwright__browser_click".into())
    );
}

#[test]
fn hands_codex_the_same_servers_as_overrides_with_their_tools() {
    let args = ConnectorSettings::default().codex_args(&by_id(&["exa", "playwright"]));
    assert_eq!(
        args[..8],
        [
            "-c",
            r#"mcp_servers.exa.url="https://mcp.exa.ai/mcp""#,
            "-c",
            r#"mcp_servers.exa.enabled_tools=["web_search_exa","web_fetch_exa"]"#,
            "-c",
            r#"mcp_servers.playwright.command="npx""#,
            "-c",
            r#"mcp_servers.playwright.args=["-y","@playwright/mcp@0.0.84"]"#,
        ]
    );
    assert_eq!(args[8], "-c");
    assert!(args[9].starts_with(r#"mcp_servers.playwright.enabled_tools=["browser_navigate","#));
    assert_eq!(args.len(), 10);
    assert!(ConnectorSettings::default().codex_args(&[]).is_empty());
}

fn poki() -> &'static Connector {
    connector("poki").unwrap()
}

fn poki_on(path: Option<&str>) -> ConnectorSettings {
    let mut settings = ConnectorSettings::default();
    settings.set_enabled("poki", true);
    if let Some(path) = path {
        settings.set_checkout("poki", path);
    }
    settings
}

#[test]
fn poki_is_off_experimental_and_listed_only_on_a_mac() {
    assert!(!poki().on_by_default);
    assert!(poki().experimental);
    let listed = |platform| ids(&available_connectors(platform)).contains(&"poki");
    assert!(listed(Platform::Mac));
    assert!(!listed(Platform::Windows));
    assert!(!listed(Platform::Linux));
}

#[test]
fn poki_runs_only_once_switched_on_on_a_mac_with_its_folder_set() {
    let has = |settings: &ConnectorSettings, platform| {
        ids(&settings.enabled_connectors(platform)).contains(&"poki")
    };
    assert!(!has(&poki_on(None), Platform::Mac));
    assert!(!has(&poki_on(Some("   ")), Platform::Mac));
    let ready = poki_on(Some("/Users/me/poki"));
    assert!(has(&ready, Platform::Mac));
    assert!(!has(&ready, Platform::Windows));
    // The folder is saved even while the switch is off.
    let mut off = ready.clone();
    off.set_enabled("poki", false);
    assert!(!has(&off, Platform::Mac));
    assert_eq!(off.checkout("poki"), "/Users/me/poki");
    // As the desktop saved it, with whitespace around.
    let saved = saved(&[
        ("monocode.connector.poki.enabled", "true"),
        ("monocode.connector.poki.path", "  /Users/me/poki \n"),
    ]);
    assert_eq!(saved.checkout("poki"), "/Users/me/poki");
    assert!(has(&saved, Platform::Mac));
}

#[test]
fn poki_runs_node_on_the_checkouts_stdio_bridge_for_claude_and_codex() {
    let settings = poki_on(Some("/Users/me/poki/"));
    assert_eq!(
        settings.mcp_config(&[poki()]),
        Some(json!({
            "mcpServers": {
                "poki": {
                    "type": "stdio",
                    "command": "node",
                    "args": ["/Users/me/poki/src/stdio.mjs"],
                }
            }
        }))
    );
    assert_eq!(
        settings.codex_args(&[poki()])[..4],
        [
            "-c",
            r#"mcp_servers.poki.command="node""#,
            "-c",
            r#"mcp_servers.poki.args=["/Users/me/poki/src/stdio.mjs"]"#,
        ]
    );
    // Nothing passes an approval mode: Poki keeps asking on its side too.
    assert!(!settings
        .mcp_config(&[poki()])
        .unwrap()
        .to_string()
        .contains("APPROVAL"));
    assert_eq!(
        checkout_file("C:\\poki\\", "src/stdio.mjs"),
        "C:\\poki/src/stdio.mjs"
    );
}

#[test]
fn poki_allows_only_reading_the_screen_acting_asks_and_the_rest_is_denied() {
    let allowed = connector_allowed_tools(&[poki()]);
    let reading = [
        "poki_mcp_status",
        "list_devices",
        "select_device",
        "get_foreground_app",
        "list_elements",
        "take_screenshot",
        "list_apps",
        "wait_for",
    ];
    assert_eq!(
        allowed,
        reading
            .iter()
            .map(|tool| format!("mcp__poki__{tool}"))
            .collect::<Vec<_>>()
    );
    let ask = connector_ask_tools(&[poki()]);
    for tool in ["tap", "fill", "type_text"] {
        assert!(ask.contains(&format!("mcp__poki__{tool}")), "{tool}");
    }
    let denied = connector_denied_tools();
    for tool in [
        "batch_commands",
        "get_clipboard",
        "set_clipboard",
        "install_app",
        "uninstall_app",
        "download_app_container",
        "build_and_run",
    ] {
        assert!(denied.contains(&format!("mcp__poki__{tool}")), "{tool}");
    }
    for tool in ask.iter().chain(&denied) {
        assert!(!allowed.contains(tool), "{tool}");
    }
    assert!(!ask.iter().any(|tool| denied.contains(tool)));
}

#[test]
fn poki_reaches_only_a_bot_whose_list_names_it() {
    let settings = poki_on(Some("/Users/me/poki"));
    let session = |only: Option<&[String]>| ids(&settings.session_connectors(only, Platform::Mac));
    assert!(!session(None).contains(&"poki"));
    assert_eq!(session(Some(&strings(&["xmd"])[..])), ["xmd"]);
    assert_eq!(session(Some(&strings(&["poki"])[..])), ["poki"]);
    assert!(!ids(&settings.enabled_connectors(Platform::Mac)).is_empty());
}

#[test]
fn tells_when_a_list_would_add_poki() {
    let poki = strings(&["poki"]);
    let xmd = strings(&["xmd"]);
    assert_eq!(ids(&user_granted_added(Some(&poki[..]), None)), ["poki"]);
    assert_eq!(
        ids(&user_granted_added(Some(&poki[..]), Some(&xmd[..]))),
        ["poki"]
    );
    assert!(user_granted_added(Some(&poki[..]), Some(&poki[..])).is_empty());
    assert!(user_granted_added(Some(&[]), Some(&poki[..])).is_empty());
    assert!(user_granted_added(None, None).is_empty());
}

#[test]
fn takes_self_approving_modes_away_from_a_session_that_has_poki() {
    assert_eq!(asking_permission_mode("auto", &[poki()]), "default");
    assert_eq!(
        asking_permission_mode("bypassPermissions", &[poki()]),
        "default"
    );
    assert_eq!(asking_permission_mode("plan", &[poki()]), "plan");
    assert_eq!(
        asking_permission_mode("acceptEdits", &[poki()]),
        "acceptEdits"
    );
    assert_eq!(
        asking_permission_mode("auto", &by_id(&["xmd", "playwright"])),
        "auto"
    );
}
