//! Claude Code plugins MonoBOT ships (src/features/skills/bundled), ported
//! from skillPacks.ts, bundledPlugins.ts and src-tauri/src/plugins.rs. They
//! are embedded at build time, written under the data folder once per
//! bundle and passed to Claude with `--plugin-dir`, so their skills load in
//! MonoBOT's sessions only and nothing is installed into `~/.claude`.
use crate::desktop_preferences::{read_flag, DesktopPreferences};
use std::{
    path::{Component, Path, PathBuf},
    sync::{Mutex, OnceLock},
};

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/bundled_plugins.rs"));
}

/// Who gets a bundled plugin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginUse {
    /// Every Claude session loads it.
    Always,
    /// Only a bot given it in its tools (BOT_SKILLS).
    Bot,
    /// A skill pack: off until the user turns it on in Settings, or a bot
    /// is given it.
    Optional,
}

/// Every plugin in the bundle (BundledPlugin), in the desktop's order.
pub const BUNDLED_PLUGINS: [(&str, PluginUse); 7] = [
    ("monobot", PluginUse::Always),
    ("monobot-finance", PluginUse::Bot),
    ("monobot-design", PluginUse::Optional),
    ("monobot-imagery", PluginUse::Optional),
    ("monobot-dev", PluginUse::Optional),
    ("monobot-audio", PluginUse::Optional),
    ("monobot-openai", PluginUse::Optional),
];

/// The plugin every Claude session gets.
pub const ALWAYS_ON: &str = "monobot";

/// Third-party skills gathered from Claude Code and Codex setups, in packs
/// that stay off until turned on (SkillPack).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkillPack {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub skills: &'static [&'static str],
    /// A key the skills' scripts read from the environment, when they need one.
    pub needs: Option<&'static str>,
}

pub const SKILL_PACKS: [SkillPack; 5] = [
    SkillPack {
        id: "monobot-design",
        name: "Design and taste",
        description: "Impeccable, the taste skills, motion principles and Vercel's React and web interface guidelines.",
        skills: &[
            "impeccable",
            "design-taste-frontend",
            "gpt-taste",
            "high-end-visual-design",
            "minimalist-ui",
            "industrial-brutalist-ui",
            "stitch-design-taste",
            "design-motion-principles",
            "vercel-react-best-practices",
            "web-design-guidelines",
        ],
        needs: None,
    },
    SkillPack {
        id: "monobot-imagery",
        name: "Image direction",
        description: "Design comps for web and mobile, image-to-code and brand kits, generated before building.",
        skills: &[
            "imagegen-frontend-web",
            "imagegen-frontend-mobile",
            "image-to-code",
            "brandkit",
        ],
        needs: None,
    },
    SkillPack {
        id: "monobot-dev",
        name: "Developer",
        description: "AI SDK, Context7, Postgres and Python performance, GitHub review comments and CI, PDFs and Vercel deploys.",
        skills: &[
            "ai-sdk",
            "context7",
            "supabase-postgres-best-practices",
            "python-performance-optimization",
            "find-skills",
            "gh-address-comments",
            "gh-fix-ci",
            "pdf",
            "vercel-deploy",
        ],
        needs: None,
    },
    SkillPack {
        id: "monobot-audio",
        name: "ElevenLabs audio",
        description: "Text to speech, transcription, voices, dubbing, sound effects and music.",
        skills: &[
            "text-to-speech",
            "speech-to-text",
            "sound-effects",
            "music",
            "dubbing",
            "voice-changer",
            "voice-isolator",
            "agents",
            "speech-engine",
            "setup-api-key",
        ],
        needs: Some("ELEVENLABS_API_KEY"),
    },
    SkillPack {
        id: "monobot-openai",
        name: "OpenAI media",
        description: "Images and Sora video with OpenAI's API.",
        skills: &["imagegen", "sora"],
        needs: Some("OPENAI_API_KEY"),
    },
];

/// The desktop's switch for a pack (`monocode.skillPack.<id>.enabled`).
pub fn skill_pack_key(id: &str) -> String {
    format!("monocode.skillPack.{id}.enabled")
}

/// The packs turned on in Settings, in SKILL_PACKS order (enabledSkillPacks).
pub fn enabled_skill_packs(preferences: &DesktopPreferences) -> Vec<String> {
    enabled_skill_packs_with(|key| preferences.get(key).map(str::to_owned))
}

/// Like [`enabled_skill_packs`], reading raw `localStorage` values through `get`.
pub fn enabled_skill_packs_with(get: impl Fn(&str) -> Option<String>) -> Vec<String> {
    SKILL_PACKS
        .iter()
        .filter(|pack| read_flag(get(&skill_pack_key(pack.id)).as_deref()).unwrap_or(false))
        .map(|pack| pack.id.to_owned())
        .collect()
}

/// The plugins a Claude session loads: MonoBOT's own, then a bot's packs,
/// or for any other session the ones turned on in Settings.
pub fn session_plugins(bot_plugins: Option<&[String]>, enabled_packs: &[String]) -> Vec<String> {
    let mut out = vec![ALWAYS_ON.to_owned()];
    for plugin in bot_plugins.unwrap_or(enabled_packs) {
        if !out.contains(plugin) {
            out.push(plugin.clone());
        }
    }
    out
}

/// A name for this bundle's contents; a new bundle extracts to a new folder.
pub fn version() -> &'static str {
    embedded::VERSION
}

/// The embedded files of a plugin, relative to its root with `/` separators.
pub fn plugin_files(plugin: &str) -> impl Iterator<Item = (&'static str, &'static [u8])> + '_ {
    embedded::FILES
        .iter()
        .filter(move |(name, ..)| *name == plugin)
        .map(|(_, path, content)| (*path, *content))
}

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Where the app keeps its data; the plugins go in its `native-plugins`
/// folder. The first call wins.
pub fn set_data_dir(dir: PathBuf) {
    let _ = DATA_DIR.set(dir);
}

/// The folder given to [`set_data_dir`], else `MONOCODE_NATIVE_DATA_DIR`,
/// else `~/.cache/monocode-native` (as Devin's supervised config does).
pub fn data_dir() -> Option<PathBuf> {
    DATA_DIR
        .get()
        .cloned()
        .or_else(|| std::env::var_os("MONOCODE_NATIVE_DATA_DIR").map(PathBuf::from))
        .or_else(|| crate::accounts::home().map(|home| home.join(".cache/monocode-native")))
}

/// The installed folders of these plugins, for `--plugin-dir`, in order.
/// A plugin that is not in the bundle or cannot be written is left out: the
/// turn still starts, just without those skills (bundledPluginDirs).
pub fn bundled_plugin_dirs(plugins: &[String]) -> Vec<PathBuf> {
    match data_dir() {
        Some(dir) => bundled_plugin_dirs_in(&dir, plugins),
        None => Vec::new(),
    }
}

/// [`bundled_plugin_dirs`] under a given data folder.
pub fn bundled_plugin_dirs_in(data_dir: &Path, plugins: &[String]) -> Vec<PathBuf> {
    plugins
        .iter()
        .filter_map(|plugin| match install(data_dir, plugin) {
            Ok(dir) => Some(dir),
            Err(error) => {
                eprintln!("Could not install the {plugin} plugin: {error}");
                None
            }
        })
        .collect()
}

/// `<data>/native-plugins/<version>/<plugin>`, written on first use. Older
/// bundles' folders are removed when a new one is written.
pub fn install(data_dir: &Path, plugin: &str) -> Result<PathBuf, String> {
    static WRITING: Mutex<()> = Mutex::new(());
    if plugin_files(plugin).next().is_none() {
        return Err("not in the bundle".into());
    }
    let root = data_dir.join("native-plugins");
    let version = root.join(embedded::VERSION);
    let target = version.join(plugin);
    let _writing = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    // A folder is renamed into place only once complete.
    if target.is_dir() {
        return Ok(target);
    }
    std::fs::create_dir_all(&version).map_err(|e| format!("{}: {e}", version.display()))?;
    remove_old_versions(&root);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let staging = version.join(format!(".{plugin}-{}-{stamp}", std::process::id()));
    let written = write_plugin(&staging, plugin).and_then(|()| {
        std::fs::rename(&staging, &target).or_else(|error| {
            // Another MonoBOT wrote the same bundle first.
            if target.is_dir() {
                Ok(())
            } else {
                Err(format!("{}: {error}", target.display()))
            }
        })
    });
    if staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    written.map(|()| target)
}

fn remove_old_versions(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name() != embedded::VERSION && entry.path().is_dir() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn write_plugin(dir: &Path, plugin: &str) -> Result<(), String> {
    for (relative, content) in plugin_files(plugin) {
        let relative = Path::new(relative);
        if !relative
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        {
            return Err(format!("Invalid plugin file path: {}", relative.display()));
        }
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&path, content).map_err(|e| format!("{}: {e}", path.display()))?;
        // A skill's launcher (`scripts/impeccable`) is run by path.
        #[cfg(unix)]
        if content.starts_with(b"#!") {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
