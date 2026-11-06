//! Embeds MonoBOT's bundled Claude Code plugins (src/features/skills/bundled)
//! so the native app ships them like the desktop app's frontend bundle does.
use std::{
    env,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.join("../../src/features/skills/bundled");
    println!("cargo:rerun-if-changed={}", root.display());
    assert!(
        root.is_dir(),
        "monocode-core embeds the skill packs in {}",
        root.display()
    );
    let mut files = Vec::new();
    for pack in sorted_dirs(&root) {
        let name = pack.file_name().unwrap().to_str().unwrap().to_owned();
        // The same files the desktop bundle carries: the manifest and
        // everything under skills/, minus dot files.
        let manifest = pack.join(".claude-plugin/plugin.json");
        if manifest.is_file() {
            files.push((
                name.clone(),
                ".claude-plugin/plugin.json".to_owned(),
                manifest,
            ));
        }
        collect(&pack.join("skills"), "skills", &name, &mut files);
    }
    files.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));

    // FNV-1a over every path and byte: a new bundle gets a new folder.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    };
    let mut table = String::from("pub(super) static FILES: &[(&str, &str, &[u8])] = &[\n");
    for (pack, relative, path) in &files {
        let content = fs::read(path).unwrap();
        feed(pack.as_bytes());
        feed(&[0]);
        feed(relative.as_bytes());
        feed(&[0]);
        feed(&(content.len() as u64).to_le_bytes());
        feed(&content);
        let path = path.to_str().unwrap();
        writeln!(
            table,
            "    ({pack:?}, {relative:?}, include_bytes!({path:?})),"
        )
        .unwrap();
    }
    table.push_str("];\n");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("bundled_plugins.rs");
    fs::write(
        out,
        format!("pub(super) const VERSION: &str = \"{hash:016x}\";\n{table}"),
    )
    .unwrap();
}

fn sorted_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir() && !hidden(path))
        .collect();
    dirs.sort();
    dirs
}

fn hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_none_or(|name| name.starts_with('.'))
}

fn collect(dir: &Path, relative: &str, pack: &str, files: &mut Vec<(String, String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if hidden(&path) {
            continue;
        }
        let name = path.file_name().unwrap().to_str().unwrap();
        let child = format!("{relative}/{name}");
        if path.is_dir() {
            collect(&path, &child, pack, files);
        } else {
            files.push((pack.to_owned(), child, path));
        }
    }
}
