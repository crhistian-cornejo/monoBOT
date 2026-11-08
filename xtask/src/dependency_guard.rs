//! Tests for crates/monocode-gpui/dependency_guard.rs and GPUI's screen
//! capture guard, built in offline fixture crates.
#![cfg(test)]

use crate::util::root;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const MANIFEST: &str = "[package]\nname = \"dependency-guard-fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[workspace]\n";

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("monocode-dependency-guard-")
            .tempdir()
            .unwrap();
        let root = dir.path().to_path_buf();
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "").unwrap();
        // The real build script also embeds the Windows icon, which needs a
        // registry dependency this offline fixture can't fetch.
        fs::copy(
            crate::util::root().join("crates/monocode-gpui/dependency_guard.rs"),
            root.join("dependency_guard.rs"),
        )
        .unwrap();
        fs::write(
            root.join("build.rs"),
            "mod dependency_guard;\nfn main() {\n    dependency_guard::check(std::path::Path::new(env!(\"CARGO_MANIFEST_DIR\")));\n}\n",
        )
        .unwrap();
        fs::write(root.join("Cargo.toml"), MANIFEST).unwrap();
        Fixture { _dir: dir, root }
    }

    fn cargo_build(&self) -> (bool, String) {
        let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .args(["build", "--offline"])
            .current_dir(&self.root)
            .env("CARGO_TARGET_DIR", self.root.join("target"))
            .env("CARGO_TERM_COLOR", "never")
            .output()
            .unwrap();
        (
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned()
                + &String::from_utf8_lossy(&output.stderr),
        )
    }

    fn add_dependency(&self, optional: bool) {
        fs::create_dir_all(self.root.join("capture/src")).unwrap();
        // A harmless local fixture; no code or license from zed-scap is copied.
        fs::write(self.root.join("capture/src/lib.rs"), "").unwrap();
        fs::write(
            self.root.join("capture/Cargo.toml"),
            "[package]\nname = \"zed-scap\"\nversion = \"9.9.9\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::write(
            self.root.join("Cargo.toml"),
            format!("{MANIFEST}\n[dependencies]\ncapture = {{ package = \"zed-scap\", path = \"capture\", optional = {optional} }}\n"),
        )
        .unwrap();
    }
}

#[test]
fn native_build_rejects_even_an_unused_optional_dependency_and_recovers_after_removal() {
    let f = Fixture::new();
    let (ok, output) = f.cargo_build();
    assert!(ok, "{output}");

    f.add_dependency(true);
    let (ok, output) = f.cargo_build();
    assert!(!ok, "{output}");
    assert!(output.contains("zed-scap is prohibited"), "{output}");
    let lock = fs::read_to_string(f.root.join("Cargo.lock")).unwrap();
    assert!(lock.contains("name = \"zed-scap\""));

    fs::write(f.root.join("Cargo.toml"), MANIFEST).unwrap();
    let (ok, output) = f.cargo_build();
    assert!(ok, "{output}");
}

#[test]
fn native_build_rejects_a_renamed_active_dependency_at_a_different_version() {
    let f = Fixture::new();
    f.add_dependency(false);
    let (ok, output) = f.cargo_build();
    assert!(!ok, "{output}");
    assert!(output.contains("zed-scap is prohibited"), "{output}");
}

#[test]
fn gpui_refuses_to_enable_screen_capture_before_compiling_its_implementation() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new("rustc")
        .arg(root().join("vendor/gpui-pre/build.rs"))
        .args([
            "--edition",
            "2024",
            "--cfg",
            "feature=\"screen-capture\"",
            "--out-dir",
        ])
        .arg(Path::new(dir.path()))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("Screen capture is disabled in MonoBOT"),
        "{stderr}"
    );
}
