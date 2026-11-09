//! Which projects may run their own Claude Code configuration. A cloned
//! repository can bring `.claude/settings.json` (hooks, a status line, key
//! helpers) and `.mcp.json` (servers), and a turn in print mode would run
//! them without asking. Until the user trusts the project, MonoBOT starts
//! Claude Code with the user's own settings only (`--setting-sources user`).
//! Trust is kept per repository root, in a file of MonoBOT's own data.
use std::path::{Path, PathBuf};

/// The project files that can make Claude Code run something.
const CONFIG: [&str; 3] = [
    ".claude/settings.json",
    ".claude/settings.local.json",
    ".mcp.json",
];

/// The repository root `cwd` is in (where its `.git` is), else `cwd`.
pub fn root(cwd: &Path) -> PathBuf {
    let cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    cwd.ancestors()
        .find(|folder| folder.join(".git").exists())
        .map(Path::to_path_buf)
        .unwrap_or(cwd)
}

/// The project's own Claude Code configuration, from `cwd` up to its root.
pub fn config_files(cwd: &Path) -> Vec<PathBuf> {
    let root = root(cwd);
    let cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let mut found = Vec::new();
    for folder in cwd.ancestors() {
        for file in CONFIG {
            let path = folder.join(file);
            if path.is_file() {
                found.push(path);
            }
        }
        if folder == root {
            break;
        }
    }
    found
}

fn list_path() -> PathBuf {
    crate::store::monocode_data_dir().join("trusted-projects.json")
}

fn read_list(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn trusted_in(list: &Path, cwd: &Path) -> bool {
    let root = root(cwd).to_string_lossy().into_owned();
    read_list(list).contains(&root)
}

fn trust_in(list: &Path, cwd: &Path) -> Result<(), String> {
    let root = root(cwd).to_string_lossy().into_owned();
    let mut trusted = read_list(list);
    if !trusted.contains(&root) {
        trusted.push(root);
    }
    if let Some(parent) = list.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&trusted).map_err(|e| e.to_string())?;
    let temporary = list.with_extension("json.tmp");
    std::fs::write(&temporary, text).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, list).map_err(|e| e.to_string())
}

/// Whether the user trusts the project `cwd` is in.
pub fn trusted(cwd: &Path) -> bool {
    trusted_in(&list_path(), cwd)
}

/// Trusts the project `cwd` is in: its Claude Code configuration runs from
/// the next turn.
pub fn trust(cwd: &Path) -> Result<(), String> {
    trust_in(&list_path(), cwd)
}

/// Whether the project brings Claude Code configuration the user has not
/// trusted yet. Reads the disk: never on the interface thread.
pub fn needs_trust(cwd: &Path) -> bool {
    !config_files(cwd).is_empty() && !trusted(cwd)
}

/// `--setting-sources` for a Claude Code turn in `cwd`.
pub fn setting_sources(cwd: &Path) -> &'static str {
    if needs_trust(cwd) {
        "--setting-sources=user"
    } else {
        "--setting-sources=user,project,local"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_projects_own_configuration_waits_for_trust() {
        let home = tempfile::tempdir().unwrap();
        let repo = home.path().join("cloned");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("src/deep")).unwrap();
        let list = home.path().join("trusted.json");
        // Nothing of its own: nothing to trust.
        assert!(config_files(&repo.join("src/deep")).is_empty());
        std::fs::create_dir_all(repo.join(".claude")).unwrap();
        std::fs::write(repo.join(".claude/settings.json"), "{\"hooks\":{}}").unwrap();
        std::fs::write(repo.join(".mcp.json"), "{}").unwrap();
        // Found from a subfolder too.
        assert_eq!(config_files(&repo.join("src/deep")).len(), 2);
        assert!(!trusted_in(&list, &repo.join("src")));
        trust_in(&list, &repo.join("src/deep")).unwrap();
        assert!(trusted_in(&list, &repo), "trust is per repository root");
        trust_in(&list, &repo).unwrap();
        assert_eq!(read_list(&list).len(), 1, "once");
    }

    #[test]
    fn configuration_above_the_repository_is_not_its_own() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join(".mcp.json"), "{}").unwrap();
        let repo = home.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        assert!(config_files(&repo).is_empty());
    }
}
