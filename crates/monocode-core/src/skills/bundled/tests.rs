//! The cases of skillPacks.test.ts and bundledPlugins.test.ts, plus checks
//! that the pack table still matches the TypeScript source.
use super::*;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};

const PACKS: &str = include_str!("../../../tests/fixtures/parity/skillPacks.ts");
const PLUGINS: &str = include_str!("../../../tests/fixtures/parity/bundledPlugins.ts");
const CLAUDE: &str = include_str!("../../../tests/fixtures/parity/claude.ts");

fn ids(uses: PluginUse) -> Vec<&'static str> {
    BUNDLED_PLUGINS
        .iter()
        .filter(|(_, u)| *u == uses)
        .map(|(id, _)| *id)
        .collect()
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

/// The string literals of `export type <name> = "a" | "b" | ...;`.
fn union(source: &str, name: &str) -> Vec<String> {
    let start = source.find(&format!("export type {name} =")).unwrap();
    let body = &source[start..];
    let body = &body[..body.find(';').unwrap()];
    regex::Regex::new(r#""([^"]+)""#)
        .unwrap()
        .captures_iter(body)
        .map(|capture| capture[1].to_owned())
        .collect()
}

fn text(plugin: &str, path: &str) -> String {
    let (_, content) = plugin_files(plugin).find(|(p, _)| *p == path).unwrap();
    String::from_utf8(content.to_vec()).unwrap()
}

#[test]
fn skill_packs_match_the_typescript_source() {
    let source = crate::ts_literal::parse_const(PACKS, "SKILL_PACKS");
    let rust: Vec<Value> = SKILL_PACKS
        .iter()
        .map(|pack| {
            let mut value = json!({
                "id": pack.id,
                "name": pack.name,
                "description": pack.description,
                "skills": pack.skills,
            });
            if let Some(needs) = pack.needs {
                value["needs"] = json!(needs);
            }
            value
        })
        .collect();
    assert_eq!(source, Value::Array(rust));
    assert_eq!(
        SKILL_PACKS.iter().map(|p| p.id).collect::<Vec<_>>(),
        ids(PluginUse::Optional)
    );
}

#[test]
fn plugin_names_and_uses_match_the_desktop() {
    assert_eq!(union(PLUGINS, "SkillPackId"), ids(PluginUse::Optional));
    let mut fixed = ids(PluginUse::Always);
    fixed.extend(ids(PluginUse::Bot));
    assert_eq!(union(PLUGINS, "BundledPlugin"), fixed);
    assert_eq!(ids(PluginUse::Always), [ALWAYS_ON]);
    // Every Claude session takes monoBOT's own plugin, then a bot's packs or
    // the ones turned on in Settings.
    let launch = CLAUDE
        .find("pluginDirs: await bundledPluginDirs([")
        .unwrap();
    let launch: String = CLAUDE[launch..launch + 160].split_whitespace().collect();
    assert!(
        launch.contains(r#"(["monobot",...(bot?bot.plugins:enabledSkillPacks()),]"#),
        "{launch}"
    );
    assert!(PACKS.contains("`monocode.skillPack.${id}.enabled`"));
    assert!(PACKS.contains("return readFlag(flagKey(id)) ?? false;"));
    assert_eq!(
        skill_pack_key("monobot-dev"),
        "monocode.skillPack.monobot-dev.enabled"
    );
}

#[test]
fn packs_stay_off_until_the_user_turns_one_on() {
    let saved = |entries: &[(&str, &str)]| {
        let values: HashMap<String, String> = entries
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        enabled_skill_packs_with(|key| values.get(key).cloned())
    };
    assert!(saved(&[]).is_empty());
    assert_eq!(
        saved(&[
            ("monocode.skillPack.monobot-openai.enabled", "1"),
            ("monocode.skillPack.monobot-design.enabled", "true"),
            ("monocode.skillPack.monobot-dev.enabled", "0"),
            // Not a pack: monoBOT's own plugin has no switch.
            ("monocode.skillPack.monobot.enabled", "1"),
        ]),
        ["monobot-design", "monobot-openai"]
    );
    let preferences = crate::desktop_preferences::DesktopPreferences::from_values(&[(
        "monocode.skillPack.monobot-audio.enabled",
        json!(1),
    )]);
    assert_eq!(enabled_skill_packs(&preferences), ["monobot-audio"]);
}

#[test]
fn sessions_load_monobot_then_a_bots_packs_or_the_enabled_ones() {
    let enabled = strings(&["monobot-design"]);
    assert_eq!(
        session_plugins(None, &enabled),
        ["monobot", "monobot-design"]
    );
    assert_eq!(session_plugins(None, &[]), ["monobot"]);
    let bot = strings(&["monobot-finance", "monobot", "monobot-finance"]);
    assert_eq!(
        session_plugins(Some(&bot), &enabled),
        ["monobot", "monobot-finance"]
    );
}

#[test]
fn embeds_every_bundled_plugin_with_its_manifest() {
    let embedded: BTreeSet<&str> = embedded::FILES.iter().map(|(name, ..)| *name).collect();
    let listed: BTreeSet<&str> = BUNDLED_PLUGINS.iter().map(|(id, _)| *id).collect();
    assert_eq!(embedded, listed);
    for (id, _) in BUNDLED_PLUGINS {
        let manifest: Value =
            serde_json::from_str(&text(id, ".claude-plugin/plugin.json")).unwrap();
        assert_eq!(manifest["name"], id);
    }
    assert_eq!(version().len(), 16);
}

#[test]
fn each_pack_ships_every_skill_it_lists_with_its_license_under_the_size_limit() {
    let name = regex::Regex::new(r#"^---\r?\nname: "?([a-z0-9-]+)"?\r?\n"#).unwrap();
    for pack in &SKILL_PACKS {
        let files: Vec<_> = plugin_files(pack.id).collect();
        let mut skills: Vec<&str> = files
            .iter()
            .filter_map(|(path, _)| {
                let rest = path.strip_prefix("skills/")?.strip_suffix("/SKILL.md")?;
                (!rest.contains('/')).then_some(rest)
            })
            .collect();
        skills.sort_unstable();
        let mut listed = pack.skills.to_vec();
        listed.sort_unstable();
        assert_eq!(skills, listed, "{}", pack.id);
        for skill in pack.skills {
            let body = text(pack.id, &format!("skills/{skill}/SKILL.md"));
            assert!(name.is_match(&body), "{skill}");
            let notice = text(pack.id, &format!("skills/{skill}/NOTICE.md"));
            assert!(notice.contains("License"), "{skill}");
        }
        // No platform binaries or images: everything is text.
        for (path, _) in &files {
            assert!(!path.contains("/bin/"), "{path}");
            assert!(
                ![".png", ".svg", ".zip"]
                    .iter()
                    .any(|ext| path.ends_with(ext)),
                "{path}"
            );
        }
        let bytes: usize = files.iter().map(|(_, content)| content.len()).sum();
        assert!(bytes < 8 * 1024 * 1024, "{}", pack.id);
    }
}

#[test]
fn ships_the_monobot_plugin_and_keeps_finance_in_its_own() {
    let name = regex::Regex::new(r"^---\r?\nname: ([a-z0-9-]+)\r?\n").unwrap();
    let mut skills: Vec<String> = plugin_files("monobot")
        .filter(|(path, _)| {
            path.strip_prefix("skills/")
                .and_then(|rest| rest.strip_suffix("/SKILL.md"))
                .is_some_and(|rest| !rest.contains('/'))
        })
        .map(|(path, content)| {
            let body = String::from_utf8(content.to_vec()).unwrap();
            name.captures(&body).unwrap_or_else(|| panic!("{path}"))[1].to_owned()
        })
        .collect();
    skills.sort();
    assert_eq!(
        skills,
        [
            "animate",
            "animate-expo",
            "animation-performance",
            "animation-vocabulary",
            "apple-design",
            "ask-sonner",
            "break-ui",
            "choreograph",
            "emil-design-eng",
            "files",
            "find-animation-opportunities",
            "improve-animations",
            "mobile-native",
            "pick-ui-library",
            "prototype",
            "remove-animations",
            "research",
            "review-animations",
            "threejs-graphics",
            "visualize",
            "write-swift",
            "writing",
        ]
    );
    let references = plugin_files("monobot")
        .filter(|(path, _)| path.starts_with("skills/threejs-graphics/references/"))
        .count();
    assert_eq!(references, 72);
    let mut finance: Vec<&str> = plugin_files("monobot-finance").map(|(p, _)| p).collect();
    finance.sort_unstable();
    assert_eq!(
        finance,
        [".claude-plugin/plugin.json", "skills/finance/SKILL.md"]
    );
}

#[test]
fn installs_each_plugin_once_per_bundle_and_drops_older_bundles() {
    let data = tempfile::tempdir().unwrap();
    let old = data.path().join("native-plugins/0000000000000000/monobot");
    std::fs::create_dir_all(&old).unwrap();
    let dirs = bundled_plugin_dirs_in(
        data.path(),
        &strings(&["monobot", "not-a-plugin", "monobot-finance"]),
    );
    let root = data.path().join("native-plugins").join(version());
    assert_eq!(dirs, [root.join("monobot"), root.join("monobot-finance")]);
    assert!(!old.exists());
    for (path, content) in plugin_files("monobot-finance") {
        assert_eq!(std::fs::read(dirs[1].join(path)).unwrap(), content);
    }
    // Nothing half-written is left beside the plugins.
    let mut entries: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    entries.sort();
    assert_eq!(entries, ["monobot", "monobot-finance"]);
    // A written plugin is reused as it is.
    let marker = dirs[1].join("marker");
    std::fs::write(&marker, "kept").unwrap();
    assert_eq!(
        bundled_plugin_dirs_in(data.path(), &strings(&["monobot-finance"])),
        [dirs[1].clone()]
    );
    assert!(marker.exists());
    assert!(install(data.path(), "../monobot").is_err());
}

#[cfg(unix)]
#[test]
fn makes_scripts_with_a_shebang_executable() {
    use std::os::unix::fs::PermissionsExt;
    let data = tempfile::tempdir().unwrap();
    let dir = install(data.path(), "monobot-design").unwrap();
    let mode = |path: &str| {
        std::fs::metadata(dir.join(path))
            .unwrap()
            .permissions()
            .mode()
            & 0o111
    };
    let (script, _) = plugin_files("monobot-design")
        .find(|(_, content)| content.starts_with(b"#!"))
        .unwrap();
    assert_ne!(mode(script), 0, "{script}");
    assert_eq!(mode(".claude-plugin/plugin.json"), 0);
}
