//! sherpa-onnx-sys (dictation's speech engine) downloads its prebuilt
//! libraries from GitHub into each Cargo target folder. A network that
//! inspects TLS refuses that download, so a fresh target folder could not
//! build. A previous build keeps the libraries under
//! target/sherpa-onnx-prebuilt; later builds (and other git worktrees) reuse
//! them through SHERPA_ONNX_LIB_DIR, and download them only when none are there.
use std::{
    path::{Path, PathBuf},
    process::Command,
};

const ARCHIVES: &[(&str, &str, &str)] = &[
    ("windows", "x86_64", "win-x64-static-MT-Release-lib"),
    ("windows", "aarch64", "win-arm64-static-MT-Release-lib"),
    ("macos", "aarch64", "osx-arm64-static-lib"),
    ("macos", "x86_64", "osx-x64-static-lib"),
    ("linux", "x86_64", "linux-x64-static-lib"),
    ("linux", "aarch64", "linux-aarch64-static-lib"),
];

/// The folder sherpa-onnx-sys unpacks for this platform's static build.
/// `os` and `arch` use Rust's names (`std::env::consts`).
pub fn archive(version: &str, os: &str, arch: &str) -> Option<String> {
    ARCHIVES
        .iter()
        .find(|(o, a, _)| *o == os && *a == arch)
        .map(|(_, _, name)| format!("sherpa-onnx-v{version}-{name}"))
}

/// The sherpa-onnx-sys version the workspace lockfile pins.
pub fn version(root: &Path) -> Option<String> {
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).ok()?;
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line.trim_end() == r#"name = "sherpa-onnx-sys""# {
            return lines
                .next()?
                .trim_end()
                .strip_prefix("version = \"")?
                .strip_suffix('"')
                .map(str::to_owned);
        }
    }
    None
}

/// This checkout and, from a git worktree, the main one.
fn checkouts(root: &Path) -> Vec<PathBuf> {
    let mut roots = vec![root.to_path_buf()];
    let common = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(root)
        .output();
    if let Ok(output) = common {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if output.status.success() && !text.is_empty() {
            if let Some(parent) = Path::new(&text).parent() {
                roots.push(parent.to_path_buf());
            }
        }
    }
    let mut unique: Vec<PathBuf> = Vec::new();
    for path in roots {
        let path = path.canonicalize().unwrap_or(path);
        if !unique.contains(&path) {
            unique.push(path);
        }
    }
    unique
}

/// The SHERPA_ONNX_LIB_DIR to set, if libraries are already on disk and
/// nobody set the variable by hand.
pub fn lib_dir(
    root: &Path,
    already_set: bool,
    os: &str,
    arch: &str,
    roots: Option<&[PathBuf]>,
) -> Option<PathBuf> {
    if already_set {
        return None;
    }
    let archive = archive(&version(root)?, os, arch)?;
    let found = checkouts(root);
    roots
        .unwrap_or(&found)
        .iter()
        .map(|checkout| {
            checkout
                .join("target")
                .join("sherpa-onnx-prebuilt")
                .join(&archive)
                .join("lib")
        })
        .find(|lib| lib.exists())
}

/// The variables to add to cargo's environment for this checkout.
pub fn cargo_env(root: &Path) -> Vec<(String, String)> {
    let set = std::env::var_os("SHERPA_ONNX_LIB_DIR").is_some_and(|v| !v.is_empty());
    lib_dir(
        root,
        set,
        std::env::consts::OS,
        std::env::consts::ARCH,
        None,
    )
    .map(|lib| {
        vec![(
            "SHERPA_ONNX_LIB_DIR".to_owned(),
            lib.to_string_lossy().into_owned(),
        )]
    })
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checkout() -> tempfile::TempDir {
        let root = tempfile::Builder::new()
            .prefix("monocode-sherpa-")
            .tempdir()
            .unwrap();
        std::fs::write(
            root.path().join("Cargo.lock"),
            "[[package]]\nname = \"sherpa-onnx-sys\"\nversion = \"1.13.8\"\n",
        )
        .unwrap();
        root
    }

    #[test]
    fn names_the_static_build_sherpa_onnx_sys_unpacks_for_each_platform() {
        assert_eq!(
            archive("1.13.8", "windows", "x86_64").as_deref(),
            Some("sherpa-onnx-v1.13.8-win-x64-static-MT-Release-lib")
        );
        assert_eq!(
            archive("1.13.8", "macos", "aarch64").as_deref(),
            Some("sherpa-onnx-v1.13.8-osx-arm64-static-lib")
        );
        assert_eq!(
            archive("1.13.8", "linux", "aarch64").as_deref(),
            Some("sherpa-onnx-v1.13.8-linux-aarch64-static-lib")
        );
        assert_eq!(archive("1.13.8", "freebsd", "x86_64"), None);
    }

    #[test]
    fn reuses_the_libraries_a_previous_build_already_fetched() {
        let dir = checkout();
        let root = dir.path();
        assert_eq!(version(root).as_deref(), Some("1.13.8"));
        let roots = [root.to_path_buf()];
        let find = |set| lib_dir(root, set, "windows", "x86_64", Some(&roots));
        assert_eq!(find(false), None, "nothing on disk: sherpa downloads them");
        let lib = root.join(
            "target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-win-x64-static-MT-Release-lib/lib",
        );
        std::fs::create_dir_all(&lib).unwrap();
        assert_eq!(find(false), Some(lib));
        assert_eq!(find(true), None, "a folder set by hand wins");
    }

    #[test]
    fn a_checkout_without_the_native_lockfile_leaves_the_environment_alone() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(version(dir.path()), None);
        let roots = [dir.path().to_path_buf()];
        assert_eq!(
            lib_dir(dir.path(), false, "macos", "aarch64", Some(&roots)),
            None
        );
    }

    #[test]
    fn reads_crlf_lockfiles() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.lock"),
            "[[package]]\r\nname = \"sherpa-onnx-sys\"\r\nversion = \"1.2.3\"\r\n",
        )
        .unwrap();
        assert_eq!(version(dir.path()).as_deref(), Some("1.2.3"));
    }
}
