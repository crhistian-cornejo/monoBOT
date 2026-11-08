//! The macOS `.app` around an already-built executable. Never builds or
//! launches the app. Development bundles use separate data; release bundles
//! use the installed app's data.
use crate::util::{copy, create_dir_all, write, Result, Tool, Tools};
use std::path::{Path, PathBuf};

pub const BUNDLE_NAME: &str = "MonoBOT Native.app";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataMode {
    DevData,
    MonocodeData,
}

impl DataMode {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "dev-data" => Ok(DataMode::DevData),
            "monocode-data" => Ok(DataMode::MonocodeData),
            other => Err(format!("Unknown data mode: {other}")),
        }
    }

    /// Release bundles use the installed data; every other profile its own.
    pub fn default_for(profile: &str) -> Self {
        if profile == "release" {
            DataMode::MonocodeData
        } else {
            DataMode::DevData
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            DataMode::DevData => "dev-data",
            DataMode::MonocodeData => "monocode-data",
        }
    }
}

pub struct Bundle<'a> {
    pub root: &'a Path,
    /// The profile folder that holds `monocode-gpui` (e.g. `target/release`).
    pub profile_dir: &'a Path,
    pub version: &'a str,
    pub data_mode: DataMode,
    /// A Developer ID identity; none or `-` signs ad hoc.
    pub identity: Option<&'a str>,
}

/// `CFBundleShortVersionString` and `CFBundleVersion` for a version.
pub fn bundle_versions(version: &str) -> (String, String) {
    let short = version.split('-').next().unwrap_or(version).to_owned();
    let build = match version.rfind("-preview.") {
        Some(index) => version[index + "-preview.".len()..].to_owned(),
        None => short.clone(),
    };
    (short, build)
}

pub fn info_plist(version: &str, data_mode: DataMode) -> String {
    let (short, build) = bundle_versions(version);
    let mode = data_mode.as_str();
    // Launch the binary itself: an exec'ing launcher changes the pid version,
    // and macOS then refuses the menu bar item. The app reads MonocodeDataMode.
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>monocode-gpui</string>
<key>MonocodeDataMode</key><string>{mode}</string>
<key>CFBundleIdentifier</key><string>dev.usemono.gpui</string>
<key>CFBundleName</key><string>MonoBOT Native</string>
<key>CFBundleDisplayName</key><string>MonoBOT Native</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>{short}</string>
<key>CFBundleVersion</key><string>{build}</string>
<key>MonocodeNativeVersion</key><string>{version}</string>
<key>CFBundleIconFile</key><string>monocode.icns</string>
<key>CFBundleIconName</key><string>AppIcon</string>
<key>NSPrincipalClass</key><string>NSApplication</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSMicrophoneUsageDescription</key><string>MonoBOT turns what you say into text while dictation is listening. The audio never leaves this Mac.</string>
</dict></plist>
"#
    )
}

/// License notices shipped with every package, as (source, file name).
pub fn notices() -> Vec<(&'static str, &'static str)> {
    vec![
        ("LICENSE", "LICENSE"),
        ("NOTICE", "NOTICE"),
        ("docs/assets/licenses/LICENSE-GPUI-APACHE", "LICENSE-GPUI-APACHE"),
        ("docs/assets/licenses/LICENSE-MERMAID-MIT", "LICENSE-MERMAID-MIT"),
        (
            "crates/monocode-gpui/assets/hugeicons.LICENSE",
            "hugeicons.LICENSE",
        ),
        (
            "crates/monocode-gpui/assets/dark-plus.LICENSE",
            "dark-plus.LICENSE",
        ),
        (
            "crates/monocode-gpui/assets/native-editor.LICENSE.txt",
            "native-editor.LICENSE.txt",
        ),
        (
            "crates/monocode-gpui/assets/material/LICENSE",
            "material-icons.LICENSE",
        ),
    ]
}

pub fn copy_notices(root: &Path, directory: &Path) -> Result<()> {
    create_dir_all(directory)?;
    for (source, name) in notices() {
        copy(&root.join(source), &directory.join(name))?;
    }
    Ok(())
}

/// Builds `<profile_dir>/MonoBOT Native.app` and signs it. Returns its path.
pub fn bundle_macos(bundle: &Bundle, tools: &dyn Tools) -> Result<PathBuf> {
    let binary = bundle.profile_dir.join("monocode-gpui");
    if !binary.is_file() {
        return Err(format!(
            "Missing native executable: {}\nBuild it with cargo first, or use cargo xtask build.",
            binary.display()
        ));
    }
    let app = bundle.profile_dir.join(BUNDLE_NAME);
    let contents = app.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    create_dir_all(&macos)?;
    create_dir_all(&resources)?;
    // Replace the inode atomically so macOS does not reuse an older signature cache.
    let next = macos.join("monocode-gpui.next");
    copy(&binary, &next)?;
    std::fs::rename(&next, macos.join("monocode-gpui")).map_err(|e| e.to_string())?;
    let root = bundle.root;
    copy(
        &root.join("assets/icons/icon.icns"),
        &resources.join("monocode.icns"),
    )?;
    // macOS 26 draws the Liquid Glass icon from Assets.car; the .icns covers older releases.
    copy(
        &root.join("assets/macos/Assets.car"),
        &resources.join("Assets.car"),
    )?;
    // Add the notices before signing so the signature covers them too.
    let licenses = resources.join("licenses");
    copy_notices(root, &licenses)?;
    let _ = std::fs::remove_file(licenses.join("Geist-OFL.txt"));
    let _ = std::fs::remove_file(macos.join("monocode-launcher"));
    write(
        &contents.join("Info.plist"),
        info_plist(bundle.version, bundle.data_mode),
    )?;
    let codesign = match bundle.identity.filter(|id| !id.is_empty() && *id != "-") {
        None => Tool::new("codesign").args(["--force", "--sign", "-"]),
        Some(identity) => Tool::new("codesign")
            .args([
                "--force",
                "--sign",
                identity,
                "--options",
                "runtime",
                "--timestamp",
            ])
            .arg("--entitlements")
            .arg(root.join("assets/macos/Entitlements.plist")),
    };
    tools.run(&codesign.arg(&app))?;
    println!("App: {}", app.display());
    Ok(app)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::util::{root, ToolError};
    use std::process::Command;

    struct FakeCodesign;
    impl Tools for FakeCodesign {
        fn run(&self, tool: &Tool) -> Result<String, ToolError> {
            assert_eq!(tool.name(), "codesign");
            Ok(String::new())
        }
    }

    #[test]
    fn finder_launches_use_development_data_for_dev_bundles_and_installed_data_for_release_bundles()
    {
        let directory = tempfile::Builder::new()
            .prefix("monocode-bundle-data-")
            .tempdir()
            .unwrap();
        let target = directory.path().join("target with spaces");
        for (profile, overridden, mode) in [
            ("native-dev", None, "dev-data"),
            ("release", None, "monocode-data"),
            ("release", Some("dev-data"), "dev-data"),
            ("native-dev", Some("monocode-data"), "monocode-data"),
        ] {
            let output = target.join(profile);
            std::fs::create_dir_all(&output).unwrap();
            let executable = output.join("monocode-gpui");
            std::fs::write(&executable, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
            crate::util::set_mode(&executable, 0o755).unwrap();
            let data_mode = match overridden {
                Some(value) => DataMode::parse(value).unwrap(),
                None => DataMode::default_for(profile),
            };
            bundle_macos(
                &Bundle {
                    root: &root(),
                    profile_dir: &output,
                    version: "0.16.0",
                    data_mode,
                    identity: None,
                },
                &FakeCodesign,
            )
            .unwrap();
            let bundle = output.join("MonoBOT Native.app/Contents");
            let plist = std::fs::read_to_string(bundle.join("Info.plist")).unwrap();
            assert!(
                plist.contains(&format!(
                    "<key>MonocodeDataMode</key><string>{mode}</string>"
                )),
                "{profile}: {plist}"
            );
            // macOS rejects menu bar items from a process that exec'd after launch.
            assert!(plist.contains("<key>CFBundleExecutable</key><string>monocode-gpui</string>"));
            // Without it macOS ends the app the moment dictation opens the microphone.
            assert!(plist.contains("<key>NSMicrophoneUsageDescription</key><string>MonoBOT "));
            assert!(!bundle.join("MacOS/monocode-launcher").exists());
            let launched = Command::new(bundle.join("MacOS/monocode-gpui"))
                .args(["--cwd", "a project with spaces"])
                .output()
                .unwrap();
            assert!(launched.status.success());
            assert_eq!(
                String::from_utf8_lossy(&launched.stdout)
                    .trim()
                    .split('\n')
                    .collect::<Vec<_>>(),
                ["--cwd", "a project with spaces"]
            );
            assert!(bundle
                .join("Resources/licenses/material-icons.LICENSE")
                .is_file());
        }
        assert!(DataMode::parse("shared").is_err());
    }

    #[test]
    fn preview_versions_keep_their_sequence_as_the_build_number() {
        assert_eq!(
            bundle_versions("0.16.0-preview.2"),
            ("0.16.0".into(), "2".into())
        );
        assert_eq!(
            bundle_versions("0.16.0"),
            ("0.16.0".into(), "0.16.0".into())
        );
    }
}
