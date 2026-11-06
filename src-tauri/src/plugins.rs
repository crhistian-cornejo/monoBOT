//! Plugins MonoBOT ships inside its frontend bundle (skills, mostly). Each one
//! is written under the app data directory and handed to Claude Code with
//! `--plugin-dir`, so it loads for MonoBOT's sessions only and nothing is
//! installed into the user's own `~/.claude`.
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use tauri::{AppHandle, Manager};

/// Same budget as a large skill pack: protects the disk from a runaway caller.
const MAX_PLUGIN_BYTES: usize = 8 * 1024 * 1024;

#[derive(Deserialize)]
pub struct PluginFile {
    /// Relative to the plugin root, with `/` separators.
    path: String,
    content: String,
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A relative path that stays inside the plugin root.
fn relative_path(raw: &str) -> Option<PathBuf> {
    let path = Path::new(raw);
    if raw.is_empty() || path.is_absolute() {
        return None;
    }
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            _ => return None,
        }
    }
    Some(clean)
}

fn write_plugin(root: &Path, files: &[PluginFile]) -> Result<(), String> {
    let total: usize = files.iter().map(|file| file.content.len()).sum();
    if total > MAX_PLUGIN_BYTES {
        return Err("Plugin is too large to install.".into());
    }
    let mut targets = Vec::with_capacity(files.len());
    for file in files {
        let relative = relative_path(&file.path)
            .ok_or_else(|| format!("Invalid plugin file path: {}", file.path))?;
        targets.push((root.join(relative), &file.content));
    }
    // Replace the whole folder so files dropped from a newer bundle disappear.
    if root.exists() {
        std::fs::remove_dir_all(root).map_err(|e| format!("{}: {e}", root.display()))?;
    }
    for (target, content) in targets {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&target, content).map_err(|e| format!("{}: {e}", target.display()))?;
        // A skill's launcher (`scripts/impeccable`) is run by path.
        #[cfg(unix)]
        if content.starts_with("#!") {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("{}: {e}", target.display()))?;
        }
    }
    Ok(())
}

/// Writes the plugin to `<app data>/plugins/<name>` and returns that folder.
#[tauri::command]
pub async fn install_plugin(
    app: AppHandle,
    name: String,
    files: Vec<PluginFile>,
) -> Result<String, String> {
    if !valid_name(&name) {
        return Err("Invalid plugin name.".into());
    }
    let root = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("plugins")
        .join(&name);
    let written = root.clone();
    tauri::async_runtime::spawn_blocking(move || write_plugin(&written, &files))
        .await
        .map_err(|e| e.to_string())??;
    Ok(root.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_paths_that_leave_the_plugin() {
        for raw in ["", "../x", "skills/../../x", "./x", "/etc/passwd"] {
            assert!(relative_path(raw).is_none(), "{raw}");
        }
        assert_eq!(
            relative_path("skills/a/SKILL.md"),
            Some(PathBuf::from("skills/a/SKILL.md"))
        );
    }

    #[test]
    fn plugin_names_are_plain_slugs() {
        assert!(valid_name("monobot-skills"));
        assert!(!valid_name("../evil"));
        assert!(!valid_name("Mono"));
        assert!(!valid_name(""));
    }

    #[cfg(unix)]
    #[test]
    fn makes_scripts_with_a_shebang_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("monobot-plugin-exec-{}", std::process::id()));
        let files = [
            PluginFile {
                path: "s/run".into(),
                content: "#!/bin/sh\necho hi\n".into(),
            },
            PluginFile {
                path: "s/SKILL.md".into(),
                content: "# x".into(),
            },
        ];
        write_plugin(&dir, &files).unwrap();
        let mode = |p: &str| std::fs::metadata(dir.join(p)).unwrap().permissions().mode() & 0o111;
        assert_ne!(mode("s/run"), 0);
        assert_eq!(mode("s/SKILL.md"), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rewrites_the_folder_with_only_the_new_files() {
        let dir = std::env::temp_dir().join(format!("monobot-plugin-test-{}", std::process::id()));
        let file = |path: &str| PluginFile {
            path: path.into(),
            content: "x".into(),
        };
        write_plugin(&dir, &[file("a/SKILL.md"), file("old.md")]).unwrap();
        write_plugin(&dir, &[file("a/SKILL.md")]).unwrap();
        assert!(dir.join("a/SKILL.md").exists());
        assert!(!dir.join("old.md").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
