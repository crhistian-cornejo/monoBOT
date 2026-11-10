//! Installers and updater packages from a prebuilt release executable:
//! macOS `.app.tar.gz` + `.dmg` (signed and notarized), Linux `.deb` +
//! AppImage, Enterprise Linux `.rpm` and the Windows NSIS installer.
//! Writes `artifacts-<kind>-<arch>.json` next to them for `cargo xtask release`.
use crate::{
    bundle::{bundle_macos, copy_notices, Bundle, DataMode, BUNDLE_NAME},
    release::{release_version, Artifact},
    util::{copy, create_dir_all, set_mode, write, Result, Tool, ToolError, Tools},
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// The parts of the environment packaging reads.
#[derive(Clone, Debug, Default)]
pub struct PackageEnv {
    pub apple_signing_identity: Option<String>,
    pub apple_api_key_path: Option<String>,
    pub apple_api_key: Option<String>,
    pub apple_api_issuer: Option<String>,
    pub appimagetool: Option<String>,
}

impl PackageEnv {
    pub fn from_env() -> Self {
        let var = |key| std::env::var(key).ok().filter(|v: &String| !v.is_empty());
        PackageEnv {
            apple_signing_identity: var("APPLE_SIGNING_IDENTITY"),
            apple_api_key_path: var("APPLE_API_KEY_PATH"),
            apple_api_key: var("APPLE_API_KEY"),
            apple_api_issuer: var("APPLE_API_ISSUER"),
            appimagetool: var("APPIMAGETOOL"),
        }
    }
}

pub struct Package<'a> {
    pub root: &'a Path,
    pub kind: &'a str,
    pub binary: &'a Path,
    pub out: &'a Path,
    pub version: &'a str,
    pub arch: &'a str,
    pub env: &'a PackageEnv,
}

const NSIS_TEMPLATE: &str = include_str!("../packaging/native-installer.nsi");
const PUBLISHER: &str = "Crhistian Cornejo";

/// stapler's EX_NOHOST (68) can mean a ticket-service or network failure.
/// Retry it boundedly; every other status stops packaging immediately.
pub fn run_stapler(
    tools: &dyn Tools,
    action: &str,
    file: &Path,
    wait: &mut dyn FnMut(Duration),
) -> Result<(), ToolError> {
    for attempt in 1..=5u64 {
        match tools.run(&Tool::new("xcrun").arg("stapler").arg(action).arg(file)) {
            Ok(_) => return Ok(()),
            Err(error) if error.status != Some(68) || attempt == 5 => return Err(error),
            Err(_) => {
                let delay = Duration::from_secs(attempt * 10);
                eprintln!(
                    "Apple ticket service unavailable during {action}; retrying in {}s ({attempt}/5)",
                    delay.as_secs()
                );
                wait(delay);
            }
        }
    }
    unreachable!()
}

/// Apparent bytes, including sparse files, without following the
/// Applications shortcut or any other symlink.
fn logical_bytes(path: &Path) -> Result<u64> {
    let info = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !info.is_dir() {
        return Ok(info.len());
    }
    let mut total = info.len();
    for entry in std::fs::read_dir(path).map_err(|e| e.to_string())? {
        total += logical_bytes(&entry.map_err(|e| e.to_string())?.path())?;
    }
    Ok(total)
}

const DESKTOP_ENTRY: &str = "[Desktop Entry]\nType=Application\nName=monoBOT\nExec=monocode-native %F\nIcon=monocode-native\nCategories=Development;\nTerminal=false\nStartupWMClass=dev.usemono.gpui\n";

/// The installed file tree shared by the deb, rpm and AppImage. Package
/// names and paths stay `monocode-native` so existing installs upgrade.
fn linux_tree(root: &Path, directory: &Path, binary: &Path, kind: &str) -> Result<()> {
    let app = directory.join("usr/lib/monocode-native");
    create_dir_all(&app)?;
    copy(binary, &app.join("monocode-gpui"))?;
    set_mode(&app.join("monocode-gpui"), 0o755)?;
    write(
        &app.join("native-install.json"),
        serde_json::json!({ "app_id": "dev.usemono.gpui", "kind": kind }).to_string(),
    )?;
    copy_notices(root, &app.join("licenses"))?;
    create_dir_all(&directory.join("usr/bin"))?;
    let launcher = directory.join("usr/bin/monocode-native");
    write(
        &launcher,
        "#!/bin/sh\nexec /usr/lib/monocode-native/monocode-gpui \"$@\"\n",
    )?;
    set_mode(&launcher, 0o755)?;
    create_dir_all(&directory.join("usr/share/applications"))?;
    write(
        &directory.join("usr/share/applications/monocode-native.desktop"),
        DESKTOP_ENTRY,
    )?;
    let icons = directory.join("usr/share/icons/hicolor/256x256/apps");
    create_dir_all(&icons)?;
    copy(&app_icon(root), &icons.join("monocode-native.png"))?;
    Ok(())
}

fn app_icon(root: &Path) -> PathBuf {
    root.join("assets/icons/icon.png")
}

/// NSIS string quoting: `$` doubles and `"` becomes `$\"`.
fn nsis_quote(value: &str) -> String {
    value.replace('$', "$$").replace('"', "$\\\"")
}

pub fn package_native(package: &Package, tools: &dyn Tools) -> Result<Vec<Artifact>> {
    let Package {
        root,
        kind,
        binary,
        out,
        version,
        arch,
        env,
    } = *package;
    let parsed = release_version(version)?;
    let binary = std::path::absolute(binary).map_err(|e| e.to_string())?;
    let out = std::path::absolute(out).map_err(|e| e.to_string())?;
    let embedded = tools.run(&Tool::new(&binary).arg("--version").capture())?;
    if embedded.trim() != format!("monoBOT {version}") {
        return Err(format!(
            "Native executable version does not match {version}"
        ));
    }
    let work = out.join("work");
    create_dir_all(&work)?;
    let mut artifacts = Vec::new();
    let mut add = |filename: String, target: Option<&str>, download: Option<&str>| {
        artifacts.push(Artifact {
            filename: filename.clone(),
            target: target.map(str::to_owned),
            download: download.map(str::to_owned),
            signature: None,
        });
        out.join(filename)
    };
    match kind {
        "macos" => {
            let expected = if arch == "aarch64" { "arm64" } else { "x86_64" };
            let actual = tools.run(&Tool::new("lipo").arg("-archs").arg(&binary).capture())?;
            if actual.trim() != expected {
                return Err(format!("Expected {expected}, found {}", actual.trim()));
            }
            let identity = env
                .apple_signing_identity
                .as_deref()
                .filter(|id| *id != "-")
                .ok_or("Public previews require a Developer ID signing identity")?;
            let profile_dir = binary.parent().ok_or("The executable has no folder")?;
            let app = bundle_macos(
                &Bundle {
                    root,
                    profile_dir,
                    version,
                    data_mode: DataMode::MonocodeData,
                    identity: Some(identity),
                },
                tools,
            )?;
            let notarize = |file: &Path, staple: Option<&Path>| -> Result<()> {
                let text = tools.run(
                    &Tool::new("xcrun")
                        .args(["notarytool", "submit"])
                        .arg(file)
                        .arg("--key")
                        .arg(env.apple_api_key_path.as_deref().unwrap_or_default())
                        .arg("--key-id")
                        .arg(env.apple_api_key.as_deref().unwrap_or_default())
                        .arg("--issuer")
                        .arg(env.apple_api_issuer.as_deref().unwrap_or_default())
                        .args(["--wait", "--output-format", "json"])
                        .capture(),
                )?;
                let status = serde_json::from_str::<serde_json::Value>(&text)
                    .ok()
                    .and_then(|v| v["status"].as_str().map(str::to_owned));
                if status.as_deref() != Some("Accepted") {
                    return Err(format!(
                        "Apple did not accept {} for notarization",
                        file.file_name().unwrap_or_default().to_string_lossy()
                    ));
                }
                if let Some(staple) = staple {
                    let mut wait = std::thread::sleep;
                    run_stapler(tools, "staple", staple, &mut wait)?;
                    run_stapler(tools, "validate", staple, &mut wait)?;
                }
                Ok(())
            };
            let zip = work.join("notarize.zip");
            tools.run(
                &Tool::new("ditto")
                    .args(["-c", "-k", "--keepParent"])
                    .arg(&app)
                    .arg(&zip),
            )?;
            notarize(&zip, Some(&app))?;
            tools.run(
                &Tool::new("codesign")
                    .args(["--verify", "--deep", "--strict"])
                    .arg(&app),
            )?;
            tools.run(
                &Tool::new("spctl")
                    .args(["--assess", "--type", "execute"])
                    .arg(&app),
            )?;
            let name = if arch == "aarch64" { "aarch64" } else { "x64" };
            let target = format!("darwin-{arch}-app");
            let archive = add(
                format!("MonoBOT_{version}_{name}.app.tar.gz"),
                Some(&target),
                None,
            );
            tools.run(
                &Tool::new("tar")
                    .arg("-czf")
                    .arg(&archive)
                    .arg("-C")
                    .arg(profile_dir)
                    .arg(BUNDLE_NAME)
                    .env("COPYFILE_DISABLE", "1"),
            )?;
            let image_root = work.join("dmg");
            std::fs::create_dir(&image_root)
                .map_err(|e| format!("{}: {e}", image_root.display()))?;
            tools.run(
                &Tool::new("ditto")
                    .arg(&app)
                    .arg(image_root.join(BUNDLE_NAME)),
            )?;
            #[cfg(unix)]
            std::os::unix::fs::symlink("/Applications", image_root.join("Applications"))
                .map_err(|e| e.to_string())?;
            let download = if arch == "aarch64" {
                "macos-arm64"
            } else {
                "macos-x64"
            };
            let dmg = add(
                format!("MonoBOT_{version}_{name}.dmg"),
                None,
                Some(download),
            );
            let size_mib = logical_bytes(&image_root)?.div_ceil(1_048_576) + 64;
            tools.run(
                &Tool::new("hdiutil")
                    .args(["create", "-volname", "monoBOT", "-srcfolder"])
                    .arg(&image_root)
                    .args(["-ov", "-format", "UDZO", "-fs", "HFS+", "-size"])
                    .arg(format!("{size_mib}m"))
                    .arg(&dmg),
            )?;
            tools.run(
                &Tool::new("codesign")
                    .args(["--sign", identity, "--timestamp"])
                    .arg(&dmg),
            )?;
            notarize(&dmg, Some(&dmg))?;
        }
        "linux" => {
            let deb_root = work.join("deb");
            linux_tree(root, &deb_root, &binary, "deb")?;
            create_dir_all(&deb_root.join("DEBIAN"))?;
            write(
                &deb_root.join("DEBIAN/control"),
                format!(
                    "Package: monocode-native\nVersion: {}\nArchitecture: amd64\nMaintainer: {PUBLISHER}\nSection: devel\nPriority: optional\nDepends: libasound2, libfontconfig1, libfreetype6, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libx11-xcb1, libxcb1, libxcb-shape0, libxcb-xfixes0, libxcb-randr0, libegl1, libvulkan1, libdbus-1-3, pkexec | policykit-1\nDescription: monoBOT, a desktop workspace for coding agents\n",
                    version.replace("-preview.", "~preview.")
                ),
            )?;
            let deb = add(
                format!("monobot_{version}_amd64.deb"),
                Some("linux-x86_64-deb"),
                Some("linux-deb"),
            );
            tools.run(
                &Tool::new("dpkg-deb")
                    .args(["--build", "--root-owner-group"])
                    .arg(&deb_root)
                    .arg(&deb),
            )?;
            let app_dir = work.join("MonoBOT.AppDir");
            linux_tree(root, &app_dir, &binary, "appimage")?;
            write(
                &app_dir.join("monocode-native.desktop"),
                DESKTOP_ENTRY.replace("Exec=monocode-native %F", "Exec=monocode-gpui %F"),
            )?;
            copy(&app_icon(root), &app_dir.join("monocode-native.png"))?;
            let app_run = app_dir.join("AppRun");
            write(
                &app_run,
                "#!/bin/sh\nexec \"$APPDIR/usr/lib/monocode-native/monocode-gpui\" \"$@\"\n",
            )?;
            set_mode(&app_run, 0o755)?;
            let image = add(
                format!("MonoBOT_{version}_amd64.AppImage"),
                Some("linux-x86_64-appimage"),
                Some("linux-appimage"),
            );
            tools.run(
                &Tool::new(env.appimagetool.as_deref().unwrap_or("appimagetool"))
                    .arg(&app_dir)
                    .arg(&image)
                    .env("ARCH", "x86_64")
                    .env("APPIMAGE_EXTRACT_AND_RUN", "1"),
            )?;
        }
        "rpm" => {
            let top = work.join("rpmbuild");
            for name in ["BUILD", "RPMS", "SOURCES", "SPECS", "SRPMS"] {
                create_dir_all(&top.join(name))?;
            }
            linux_tree(root, &top.join("SOURCES/tree"), &binary, "rpm")?;
            let spec = top.join("SPECS/monocode-native.spec");
            write(
                &spec,
                format!(
                    "Name: monocode-native\nVersion: {}\nRelease: {}%{{?dist}}\nSummary: monoBOT, a desktop workspace for coding agents\nLicense: MIT AND Apache-2.0\nVendor: {PUBLISHER}\nBuildArch: x86_64\nRequires: polkit\n%description\nmonoBOT runs coding agents and personal bots from one desktop app.\n%install\nmkdir -p %{{buildroot}}\ncp -a %{{_sourcedir}}/tree/. %{{buildroot}}/\n%files\n/usr/bin/monocode-native\n/usr/lib/monocode-native\n/usr/share/applications/monocode-native.desktop\n/usr/share/icons/hicolor/256x256/apps/monocode-native.png\n",
                    parsed.base,
                    parsed.rpm_release()
                ),
            )?;
            tools.run(
                &Tool::new("rpmbuild")
                    .arg("--define")
                    .arg(format!("_topdir {}", top.display()))
                    .args(["--define", "_build_id_links none", "-bb"])
                    .arg(&spec),
            )?;
            let built = top.join("RPMS/x86_64");
            let rpms: Vec<String> = std::fs::read_dir(&built)
                .map_err(|e| format!("{}: {e}", built.display()))?
                .filter_map(|e| e.ok()?.file_name().into_string().ok())
                .filter(|n| {
                    n.ends_with(".rpm") && !n.contains("debuginfo") && !n.contains("debugsource")
                })
                .collect();
            let [rpm] = rpms.as_slice() else {
                return Err("Expected exactly one native rpm".into());
            };
            let name = format!(
                "monobot-{}-{}.x86_64.rpm",
                parsed.base,
                parsed.rpm_release()
            );
            copy(
                &built.join(rpm),
                &add(name, Some("linux-x86_64-rpm"), Some("linux-rpm")),
            )?;
        }
        "windows" => {
            let stage = work.join("windows");
            create_dir_all(&stage)?;
            copy(&binary, &stage.join("monocode-gpui.exe"))?;
            write(
                &stage.join("native-install.json"),
                serde_json::json!({ "app_id": "dev.usemono.gpui", "kind": "nsis" }).to_string(),
            )?;
            copy_notices(root, &stage.join("licenses"))?;
            let installer = add(
                format!("MonoBOT_{version}_x64-setup.exe"),
                Some("windows-x86_64-nsis"),
                Some("windows"),
            );
            let script = nsis_script(
                &stage,
                &installer,
                &root.join("assets"),
                version,
                &parsed.windows,
            );
            let nsi = work.join("installer.nsi");
            write(&nsi, script)?;
            tools.run(&Tool::new("makensis").arg(&nsi))?;
        }
        other => return Err(format!("Unknown native package kind: {other}")),
    }
    let list: Vec<serde_json::Value> = artifacts.iter().map(Artifact::to_json).collect();
    write(
        &out.join(format!("artifacts-{kind}-{arch}.json")),
        serde_json::to_string_pretty(&list).map_err(|e| e.to_string())?,
    )?;
    Ok(artifacts)
}

fn nsis_script(
    stage: &Path,
    installer: &Path,
    assets: &Path,
    version: &str,
    windows: &str,
) -> String {
    NSIS_TEMPLATE
        .replace("@STAGE@", &nsis_quote(&stage.to_string_lossy()))
        .replace("@OUTFILE@", &nsis_quote(&installer.to_string_lossy()))
        .replace("@ASSETS@", &nsis_quote(&assets.to_string_lossy()))
        .replace("@VERSION@", version)
        .replace("@WINDOWS_VERSION@", windows)
        .replace("@PUBLISHER@", PUBLISHER)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::util::{root, run_system};
    use std::{cell::RefCell, fs};

    /// Stands in for Apple's tools, like the fake scripts the Node test used.
    #[derive(Default)]
    struct FakeApple {
        notary_status: RefCell<String>,
        stapler_mode: RefCell<String>,
        stapler_calls: RefCell<u32>,
        hdiutil_args: RefCell<Vec<String>>,
        sparse_bytes: u64,
    }

    impl Tools for FakeApple {
        fn run(&self, tool: &Tool) -> Result<String, ToolError> {
            let args: Vec<String> = tool
                .args
                .iter()
                .map(|a| a.to_string_lossy().into())
                .collect();
            let fail = |status| ToolError {
                status: Some(status),
                message: format!("{} failed ({status})", tool.name()),
            };
            match tool.name().as_str() {
                "lipo" => Ok("arm64\n".into()),
                "codesign" | "spctl" => Ok(String::new()),
                "ditto" if args[0] == "-c" => {
                    fs::write(args.last().unwrap(), "zip").unwrap();
                    Ok(String::new())
                }
                "ditto" => {
                    crate::util::copy_tree(Path::new(&args[0]), Path::new(&args[1])).unwrap();
                    let binary = Path::new(&args[1]).join("Contents/MacOS/monocode-gpui");
                    fs::OpenOptions::new()
                        .write(true)
                        .open(binary)
                        .unwrap()
                        .set_len(self.sparse_bytes)
                        .unwrap();
                    Ok(String::new())
                }
                "tar" => {
                    fs::write(&args[1], "archive").unwrap();
                    Ok(String::new())
                }
                "hdiutil" => {
                    fs::write(args.last().unwrap(), "dmg").unwrap();
                    *self.hdiutil_args.borrow_mut() = args;
                    Ok(String::new())
                }
                "xcrun" if args[0] == "notarytool" => Ok(format!(
                    "{{\"status\":\"{}\"}}",
                    self.notary_status.borrow()
                )),
                "xcrun" if args[0] == "stapler" => {
                    let mode = self.stapler_mode.borrow().clone();
                    if mode.is_empty() {
                        return Ok(String::new());
                    }
                    let mut calls = self.stapler_calls.borrow_mut();
                    *calls += 1;
                    match mode.as_str() {
                        "invalid" => Err(fail(65)),
                        "offline" => Err(fail(68)),
                        _ if *calls < 3 => Err(fail(68)),
                        _ => Ok(String::new()),
                    }
                }
                // The executable itself answers --version.
                _ => run_system(tool),
            }
        }
    }

    #[test]
    fn mac_preview_packaging_checks_notarization_preserves_preview_identity_and_emits_both_download_and_updater_packages(
    ) {
        let directory = tempfile::Builder::new()
            .prefix("native packaging with spaces-")
            .tempdir()
            .unwrap();
        let release = directory.path().join("target/release");
        fs::create_dir_all(&release).unwrap();
        let binary = release.join("monocode-gpui");
        fs::write(&binary, "#!/bin/sh\nprintf 'monoBOT 0.16.0-preview.2\\n'\n").unwrap();
        set_mode(&binary, 0o755).unwrap();
        let tools = FakeApple {
            notary_status: RefCell::new("Accepted".into()),
            // A sparse executable exposes image estimates based on allocated blocks.
            sparse_bytes: 128 * 1024 * 1024,
            ..Default::default()
        };
        let env = PackageEnv {
            apple_signing_identity: Some("Developer ID Application: Fixture (TESTTEAM)".into()),
            apple_api_key_path: Some(directory.path().join("fixture.p8").to_string_lossy().into()),
            apple_api_key: Some("FIXTURE".into()),
            apple_api_issuer: Some("fixture-issuer".into()),
            appimagetool: None,
        };
        let root = root();
        let package = |out: &str, version: &'static str| {
            let out = directory.path().join(out);
            package_native(
                &Package {
                    root: &root,
                    kind: "macos",
                    binary: &binary,
                    out: &out,
                    version,
                    arch: "aarch64",
                    env: &env,
                },
                &tools,
            )
            .map(|artifacts| (artifacts, out))
        };
        let (artifacts, out) = package("output", "0.16.0-preview.2").unwrap();
        assert_eq!(artifacts.len(), 2);
        assert!(artifacts
            .iter()
            .any(|a| a.target.as_deref() == Some("darwin-aarch64-app")
                && a.filename.ends_with(".app.tar.gz")));
        assert!(artifacts
            .iter()
            .any(|a| a.target.is_none() && a.filename.ends_with(".dmg")));
        for artifact in &artifacts {
            assert!(artifact
                .filename
                .starts_with("MonoBOT_0.16.0-preview.2_aarch64"));
            assert!(out.join(&artifact.filename).is_file());
        }
        let listed: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(out.join("artifacts-macos-aarch64.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(listed.as_array().unwrap().len(), 2);
        let image = tools.hdiutil_args.borrow().clone();
        let after = |flag: &str| image[image.iter().position(|a| a == flag).unwrap() + 1].clone();
        assert_eq!(after("-fs"), "HFS+");
        let size = after("-size");
        let mib: u64 = size.strip_suffix('m').unwrap().parse().unwrap();
        assert!(
            mib >= 192,
            "capacity covers logical bytes and filesystem headroom"
        );
        assert!(mib < 256, "Applications symlink is not traversed");
        let plist = fs::read_to_string(release.join("monoBOT.app/Contents/Info.plist")).unwrap();
        assert!(plist.contains("<key>CFBundleShortVersionString</key><string>0.16.0</string>"));
        assert!(plist.contains("<key>CFBundleVersion</key><string>2</string>"));
        assert!(plist.contains("<key>MonocodeNativeVersion</key><string>0.16.0-preview.2</string>"));
        assert!(plist.contains("<string>dev.usemono.gpui</string>"));

        // staple and validate recover from ticket-service failures.
        for action in ["staple", "validate"] {
            *tools.stapler_mode.borrow_mut() = "recover".into();
            *tools.stapler_calls.borrow_mut() = 0;
            let mut delays = Vec::new();
            run_stapler(&tools, action, &release, &mut |d| delays.push(d.as_secs())).unwrap();
            assert_eq!(delays, [10, 20]);
            assert_eq!(*tools.stapler_calls.borrow(), 3);
        }
        // Permanent ticket validation failures stop immediately.
        *tools.stapler_mode.borrow_mut() = "invalid".into();
        *tools.stapler_calls.borrow_mut() = 0;
        let mut delays = Vec::new();
        let error = run_stapler(&tools, "validate", &release, &mut |d| delays.push(d)).unwrap_err();
        assert_eq!(error.message, "xcrun failed (65)");
        assert!(delays.is_empty());
        assert_eq!(*tools.stapler_calls.borrow(), 1);
        // Persistent ticket-service failures stop after five attempts.
        *tools.stapler_mode.borrow_mut() = "offline".into();
        *tools.stapler_calls.borrow_mut() = 0;
        let mut delays = Vec::new();
        let error = run_stapler(&tools, "staple", &release, &mut |d| {
            delays.push(d.as_secs())
        })
        .unwrap_err();
        assert_eq!(error.message, "xcrun failed (68)");
        assert_eq!(delays, [10, 20, 30, 40]);
        assert_eq!(*tools.stapler_calls.borrow(), 5);

        *tools.stapler_mode.borrow_mut() = String::new();
        *tools.notary_status.borrow_mut() = "Invalid".into();
        let error = package("mismatched", "0.16.0-preview.3").unwrap_err();
        assert!(
            error.contains("executable version does not match"),
            "{error}"
        );
        fs::write(&binary, "#!/bin/sh\nprintf 'monoBOT 0.16.0-preview.3\\n'\n").unwrap();
        let error = package("rejected", "0.16.0-preview.3").unwrap_err();
        assert!(error.contains("Apple did not accept"), "{error}");
    }

    #[test]
    fn mac_packages_need_a_developer_id() {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("monocode-gpui");
        fs::write(&binary, "#!/bin/sh\nprintf 'monoBOT 0.16.0-preview.2\\n'\n").unwrap();
        set_mode(&binary, 0o755).unwrap();
        let error = package_native(
            &Package {
                root: &root(),
                kind: "macos",
                binary: &binary,
                out: &directory.path().join("out"),
                version: "0.16.0-preview.2",
                arch: "aarch64",
                env: &PackageEnv::default(),
            },
            &FakeApple::default(),
        )
        .unwrap_err();
        assert!(error.contains("Developer ID"), "{error}");
    }

    #[test]
    fn linux_packages_install_the_app_its_launcher_icon_and_notices() {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("monocode-gpui");
        fs::write(&binary, "binary").unwrap();
        let tree = directory.path().join("tree");
        linux_tree(&root(), &tree, &binary, "deb").unwrap();
        for file in [
            "usr/lib/monocode-native/monocode-gpui",
            "usr/lib/monocode-native/native-install.json",
            "usr/lib/monocode-native/licenses/LICENSE",
            "usr/bin/monocode-native",
            "usr/share/applications/monocode-native.desktop",
            "usr/share/icons/hicolor/256x256/apps/monocode-native.png",
        ] {
            assert!(tree.join(file).is_file(), "{file}");
        }
        assert_eq!(
            fs::read_to_string(tree.join("usr/lib/monocode-native/native-install.json")).unwrap(),
            r#"{"app_id":"dev.usemono.gpui","kind":"deb"}"#
        );
    }

    #[test]
    fn the_windows_installer_quotes_paths_and_carries_the_version() {
        let script = nsis_script(
            Path::new("C:\\stage $dir"),
            Path::new("C:\\out\\\"MonoBOT\".exe"),
            Path::new("C:\\repo\\assets"),
            "0.16.0-preview.2",
            "0.16.0.2",
        );
        assert!(script.contains("\"C:\\stage $$dir\\monocode-gpui.exe\""));
        assert!(script.contains("OutFile \"C:\\out\\$\\\"MonoBOT$\\\".exe\""));
        assert!(script.contains("VIProductVersion \"0.16.0.2\""));
        assert!(script.contains("\"C:\\repo\\assets\\installer\\nsis-sidebar.bmp\""));
        assert!(script.contains("LangString RunApp ${LANG_SPANISH} \"Abrir monoBOT\""));
        assert!(script.contains(PUBLISHER));
        assert!(!script.contains('@'), "every placeholder is filled");
    }
}
