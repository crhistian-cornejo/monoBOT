//! `cargo xtask bump-version <x.y.z>`: one version for the app, its crates
//! and the remote host.
use crate::util::Result;
use std::{fs, path::Path};

fn valid(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Replaces the first match of `prefix` + old value + `"` in `text`, where the
/// match starts a line (`at_line_start`) or anywhere.
fn replace_value(text: &str, prefix: &str, value: &str, at_line_start: bool) -> Option<String> {
    let mut search = 0;
    while let Some(found) = text[search..].find(prefix) {
        let start = search + found;
        search = start + prefix.len();
        if at_line_start && start > 0 && !text[..start].ends_with('\n') {
            continue;
        }
        let rest = start + prefix.len();
        let end = rest + text[rest..].find('"')?;
        return Some(format!("{}{value}{}", &text[..rest], &text[end..]));
    }
    None
}

/// The workspace crates that inherit the workspace version.
fn inheriting_crates(root: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let crates = root.join("crates");
    let mut entries: Vec<_> = fs::read_dir(&crates)
        .map_err(|e| format!("{}: {e}", crates.display()))?
        .filter_map(|e| e.ok().map(|e| e.path().join("Cargo.toml")))
        .filter(|p| p.is_file())
        .collect();
    entries.sort();
    for manifest in entries {
        let text = fs::read_to_string(&manifest).map_err(|e| e.to_string())?;
        if !text.lines().any(|l| l.trim() == "version.workspace = true") {
            continue;
        }
        if let Some(name) = text.lines().find_map(|l| {
            l.trim()
                .strip_prefix("name = \"")
                .and_then(|r| r.strip_suffix('"'))
        }) {
            names.push(name.to_owned());
        }
    }
    Ok(names)
}

pub fn bump(root: &Path, version: &str) -> Result<Vec<String>> {
    if !valid(version) {
        return Err("usage: cargo xtask bump-version 0.1.1".into());
    }
    let mut changed = Vec::new();
    let mut update = |relative: &str, edit: &dyn Fn(&str) -> Option<String>| -> Result<()> {
        let path = root.join(relative);
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let next = edit(&text).ok_or_else(|| format!("failed to update {relative}"))?;
        fs::write(&path, next).map_err(|e| e.to_string())?;
        changed.push(relative.to_owned());
        Ok(())
    };
    // The remote host reports this version, and the app only accepts a host
    // whose version matches its own CARGO_PKG_VERSION (monocode-core/src/remote).
    for host in ["host/package.json", "host/Cargo.toml"] {
        if root.join(host).is_file() {
            let (prefix, line_start) = if host.ends_with(".json") {
                ("\"version\": \"", false)
            } else {
                ("version = \"", true)
            };
            update(host, &|t| replace_value(t, prefix, version, line_start))?;
        }
    }
    update("Cargo.toml", &|t| {
        replace_value(t, "version = \"", version, true)
    })?;
    let crates = inheriting_crates(root)?;
    update("Cargo.lock", &|text| {
        let mut text = text.to_owned();
        for name in &crates {
            text = replace_value(
                &text,
                &format!("name = \"{name}\"\nversion = \""),
                version,
                true,
            )?;
        }
        Some(text)
    })?;
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bumps_the_workspace_its_crates_lock_entries_and_the_host() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\n\n[workspace.package]\nversion = \"0.16.0\"\n\n[workspace.dependencies]\nserde = { version = \"1\" }\n",
        )
        .unwrap();
        for (name, version) in [
            ("monocode-core", "version.workspace = true"),
            ("monocode-gpui", "version.workspace = true"),
            ("monobot-email", "version.workspace = true"),
            ("monocode-mobile-link", "version = \"0.1.0\""),
        ] {
            fs::create_dir_all(root.join("crates").join(name)).unwrap();
            fs::write(
                root.join("crates").join(name).join("Cargo.toml"),
                format!("[package]\nname = \"{name}\"\n{version}\n"),
            )
            .unwrap();
        }
        fs::write(
            root.join("Cargo.lock"),
            "[[package]]\nname = \"monobot-email\"\nversion = \"0.16.0\"\n\n[[package]]\nname = \"monocode-core\"\nversion = \"0.16.0\"\n\n[[package]]\nname = \"monocode-gpui\"\nversion = \"0.16.0\"\n\n[[package]]\nname = \"monocode-mobile-link\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"serde\"\nversion = \"0.16.0\"\n",
        )
        .unwrap();
        fs::create_dir(root.join("host")).unwrap();
        fs::write(
            root.join("host/package.json"),
            "{\n  \"name\": \"monocode-host\",\n  \"version\": \"0.16.0\"\n}\n",
        )
        .unwrap();

        assert!(bump(root, "1.2").is_err());
        assert!(bump(root, "v1.2.3").is_err());
        let changed = bump(root, "0.17.1").unwrap();
        assert_eq!(changed, ["host/package.json", "Cargo.toml", "Cargo.lock"]);
        let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(manifest.contains("version = \"0.17.1\""));
        assert!(manifest.contains("serde = { version = \"1\" }"));
        let lock = fs::read_to_string(root.join("Cargo.lock")).unwrap();
        for name in ["monobot-email", "monocode-core", "monocode-gpui"] {
            assert!(lock.contains(&format!("name = \"{name}\"\nversion = \"0.17.1\"")));
        }
        assert!(lock.contains("name = \"monocode-mobile-link\"\nversion = \"0.1.0\""));
        assert!(lock.contains("name = \"serde\"\nversion = \"0.16.0\""));
        assert!(fs::read_to_string(root.join("host/package.json"))
            .unwrap()
            .contains("\"version\": \"0.17.1\""));
    }
}
