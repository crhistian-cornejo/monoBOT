//! MonoBOT's desktop tooling: `cargo xtask <command>`.
//!
//! Builds, runs, packages and releases the GPUI app without Node, Bun or
//! Python. External tools that only exist as programs (codesign, notarytool,
//! hdiutil, appimagetool, rpmbuild, makensis, sshd) are called as programs.
mod build_app;
mod bundle;
mod dependency_guard;
mod dev;
mod icons;
mod logo;
mod package;
mod perf;
mod release;
mod sherpa;
#[cfg(unix)]
mod ssh_smoke;
mod util;
mod version;

use std::{collections::HashMap, path::Path};
use util::{root, run_system, Result};

const HELP: &str = "\
Usage: cargo xtask <command> [args]

Development
  dev [app args]        Build (native-dev), run on separate data; r + Enter rebuilds
                        and restarts. App args pass through: --cwd <dir>, --fixture,
                        --monocode-data, --dev-data.
  build                 Optimized build (and monoBOT.app on macOS)
  test [core|gpui]      Core and GPUI tests (GPUI single-threaded)
  check                 fmt --check, clippy -D warnings, xtask tests
  cargo <args>          cargo with the dictation libraries already on disk found
  bump-version <x.y.z>  Set the app, crate and host version

Packaging and releases
  bundle-macos [profile] [target dir] [version] [dev-data|monocode-data]
                        Wrap a built executable in monoBOT.app and sign it
  package <macos|linux|rpm|windows> <binary> <out dir> <arch>
                        Installers and updater packages (version from
                        MONOCODE_NATIVE_VERSION; macOS signs and notarizes)
  preview-version <v>   Check a native preview version (0.16.0-preview.1)
  release <dir>         Sign every updater package and write latest.json
  verify-release <dir>  Check latest.json and the packages with the app's verifier
  update-keys <dir>     Create the update signing key pair
  sign <file>...        Write <file>.sig with the update signing key

Tools
  icons hugeicons <package dir> <out dir> name=Icon@stroke...
  icons material <package dir> <out.json>
  logo                  Redraw every app icon from assets/logo/monobot.svg
  ssh-smoke [--desktop <bin>] [--host <monocode-host>] [--profile <p>]
                        Native SSH transport against a disposable loopback sshd
  perf <frame-run|stream-profile> [binary] [seconds]
                        Profile the UI while a replayed reply streams
";

fn env_map() -> HashMap<String, String> {
    std::env::vars().collect()
}

/// Runs cargo from the repository root with the sherpa libraries found.
fn cargo(args: &[&str]) -> Result<()> {
    run_system(&build_app::cargo(&root()).args(args))?;
    Ok(())
}

fn exit_with_cargo(args: &[String]) -> Result<()> {
    let tool = build_app::cargo(&root()).args(args);
    let mut command = util::command(&tool.program);
    command
        .args(&tool.args)
        .envs(tool.env.iter().map(|(k, v)| (k, v)))
        .current_dir(root());
    let status = command.status().map_err(|e| e.to_string())?;
    std::process::exit(status.code().unwrap_or(1));
}

fn run(args: &[String]) -> Result<()> {
    let (command, rest) = match args.split_first() {
        Some((command, rest)) => (command.as_str(), rest),
        None => ("help", &[][..]),
    };
    let one = |what: &str| -> Result<&String> {
        match rest {
            [value] => Ok(value),
            _ => Err(format!("usage: cargo xtask {command} <{what}>")),
        }
    };
    match command {
        "help" | "-h" | "--help" => {
            print!("{HELP}");
            Ok(())
        }
        "dev" => dev::main(rest),
        "build" => build_app::main(),
        "test" => {
            let which = rest.first().map(String::as_str);
            if matches!(which, None | Some("core")) {
                cargo(&[
                    "test",
                    "--locked",
                    "-p",
                    "monocode-core",
                    "--profile",
                    "native-dev",
                ])?;
            }
            if matches!(which, None | Some("gpui")) {
                cargo(&[
                    "test",
                    "--locked",
                    "-p",
                    "monocode-gpui",
                    "--profile",
                    "native-dev",
                    "--",
                    "--test-threads=1",
                ])?;
            }
            Ok(())
        }
        "check" => {
            cargo(&["fmt", "--all", "--check"])?;
            cargo(&[
                "clippy",
                "--locked",
                "-p",
                "monocode-core",
                "-p",
                "monocode-gpui",
                "-p",
                "xtask",
                "--all-targets",
                "--profile",
                "native-dev",
                "--",
                "-D",
                "warnings",
            ])?;
            cargo(&["test", "--locked", "-p", "xtask"])
        }
        "cargo" => exit_with_cargo(rest),
        "bump-version" => {
            let version = one("x.y.z")?;
            for file in version::bump(&root(), version)? {
                println!("updated {file}");
            }
            println!("version {version}");
            Ok(())
        }
        "bundle-macos" => {
            if util::Platform::current() != util::Platform::Mac {
                return Err("macOS app bundles must be packaged on macOS.".into());
            }
            let root = root();
            let profile = rest.first().map_or("release", String::as_str);
            let target = match rest.get(1) {
                Some(dir) => dir.into(),
                None => std::env::var_os("CARGO_TARGET_DIR")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| root.join("target")),
            };
            let version = match rest.get(2) {
                Some(version) => version.clone(),
                None => util::workspace_version(&root)?,
            };
            let data_mode = match rest.get(3) {
                Some(mode) => bundle::DataMode::parse(mode)?,
                None => bundle::DataMode::default_for(profile),
            };
            let identity = std::env::var("MONOCODE_NATIVE_SIGNING_IDENTITY").ok();
            bundle::bundle_macos(
                &bundle::Bundle {
                    root: &root,
                    profile_dir: &target.join(profile),
                    version: &version,
                    data_mode,
                    identity: identity.as_deref(),
                },
                &util::System,
            )
            .map(|_| ())
        }
        "package" => {
            let [kind, binary, out, arch] = rest else {
                return Err(
                    "usage: cargo xtask package <macos|linux|rpm|windows> <binary> <out dir> <arch>"
                        .into(),
                );
            };
            let version = std::env::var("MONOCODE_NATIVE_VERSION").unwrap_or_default();
            package::package_native(
                &package::Package {
                    root: &root(),
                    kind,
                    binary: Path::new(binary),
                    out: Path::new(out),
                    version: &version,
                    arch,
                    env: &package::PackageEnv::from_env(),
                },
                &util::System,
            )
            .map(|_| ())
        }
        "preview-version" => {
            let parsed = release::preview_version(one("version")?)?;
            println!(
                "{} (base {}, preview {}, Windows {})",
                parsed.version, parsed.base, parsed.sequence, parsed.windows
            );
            Ok(())
        }
        "release" => {
            let files = release::prepare_release(Path::new(one("dir")?), &env_map(), &root())?;
            println!("Signed the native feed; {} files to publish.", files.len());
            Ok(())
        }
        "verify-release" => {
            let dir = std::path::absolute(one("dir")?).map_err(|e| e.to_string())?;
            let dir = dir.to_string_lossy();
            // The app's own update client checks the feed and every package.
            cargo(&[
                "run",
                "--locked",
                "-p",
                "monocode-core",
                "--example",
                "verify_native_update",
                "--profile",
                "native-dev",
                "--",
                &dir,
            ])
        }
        "update-keys" => release::generate_keys(Path::new(one("dir")?), &env_map()),
        "sign" => {
            if rest.is_empty() {
                return Err("usage: cargo xtask sign <file>...".into());
            }
            let signer = release::Signer::from_env(&env_map())?;
            for file in rest {
                signer.sign(Path::new(file))?;
            }
            Ok(())
        }
        "icons" => icons::main(rest),
        "logo" => logo::main(rest),
        #[cfg(unix)]
        "ssh-smoke" => ssh_smoke::main(rest),
        "perf" => perf::main(rest),
        other => Err(format!("Unknown command {other}. Run cargo xtask help.")),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(error) = run(&args) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
