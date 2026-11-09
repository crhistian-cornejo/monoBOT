//! Process, path and metadata helpers shared by the commands.
use std::{
    ffi::{OsStr, OsString},
    fmt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub type Result<T, E = String> = std::result::Result<T, E>;

/// The repository root (the folder that holds `xtask/`).
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives inside the repository")
        .to_path_buf()
}

/// The workspace version from the root `Cargo.toml` (`[workspace.package]`).
pub fn workspace_version(root: &Path) -> Result<String> {
    let text = std::fs::read_to_string(root.join("Cargo.toml")).map_err(|e| e.to_string())?;
    text.lines()
        .find_map(|line| {
            line.strip_prefix("version = \"")
                .and_then(|rest| rest.split('"').next())
        })
        .map(str::to_owned)
        .ok_or_else(|| "No version in Cargo.toml".into())
}

/// A `Command` without the variables `cargo run` sets for xtask itself, so the
/// tools and the app it starts see the same environment a terminal gives them.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    for (key, _) in std::env::vars_os() {
        let Some(key) = key.to_str() else { continue };
        if key.starts_with("CARGO_PKG_")
            || key.starts_with("CARGO_MANIFEST_")
            || matches!(
                key,
                "CARGO_CRATE_NAME" | "CARGO_BIN_NAME" | "CARGO_PRIMARY_PACKAGE"
            )
        {
            command.env_remove(key);
        }
    }
    command
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Mac,
    Windows,
    Linux,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Platform::Mac
        } else if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Linux
        }
    }

    pub fn executable(self) -> &'static str {
        if self == Platform::Windows {
            "monocode-gpui.exe"
        } else {
            "monocode-gpui"
        }
    }
}

/// One external tool invocation (codesign, hdiutil, rpmbuild…).
#[derive(Clone, Debug, Default)]
pub struct Tool {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
    /// Return stdout instead of passing it through.
    pub capture: bool,
}

impl Tool {
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Tool {
            program: program.as_ref().to_owned(),
            ..Default::default()
        }
    }
    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.args.push(arg.as_ref().to_owned());
        self
    }
    pub fn args<I: IntoIterator<Item = S>, S: AsRef<OsStr>>(mut self, args: I) -> Self {
        self.args
            .extend(args.into_iter().map(|a| a.as_ref().to_owned()));
        self
    }
    pub fn env(mut self, key: &str, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }
    pub fn capture(mut self) -> Self {
        self.capture = true;
        self
    }
    pub fn name(&self) -> String {
        Path::new(&self.program)
            .file_name()
            .unwrap_or(self.program.as_os_str())
            .to_string_lossy()
            .into_owned()
    }
}

#[derive(Debug)]
pub struct ToolError {
    /// The exit status, when the tool ran and failed.
    pub status: Option<i32>,
    pub message: String,
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<ToolError> for String {
    fn from(error: ToolError) -> String {
        error.message
    }
}

/// Runs external tools. Tests swap in fakes instead of editing `PATH`.
pub trait Tools {
    /// Runs the tool; returns its stdout when `capture` is set.
    fn run(&self, tool: &Tool) -> Result<String, ToolError>;
}

/// Runs the real tools.
pub struct System;

impl Tools for System {
    fn run(&self, tool: &Tool) -> Result<String, ToolError> {
        run_system(tool)
    }
}

pub fn run_system(tool: &Tool) -> Result<String, ToolError> {
    let mut command = command(&tool.program);
    command.args(&tool.args);
    command.envs(tool.env.iter().map(|(k, v)| (k, v)));
    command.current_dir(tool.cwd.clone().unwrap_or_else(root));
    if tool.capture {
        command.stdin(Stdio::null()).stdout(Stdio::piped());
    }
    let output = command.spawn().and_then(|child| child.wait_with_output());
    let output = output.map_err(|error| ToolError {
        status: None,
        message: format!("{}: {error}", tool.program.to_string_lossy()),
    })?;
    if !output.status.success() {
        let status = output.status.code();
        return Err(ToolError {
            status,
            message: format!(
                "{} failed ({})",
                tool.name(),
                status.map_or_else(|| "signal".into(), |s| s.to_string())
            ),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The parts of `cargo metadata` the build commands need.
#[derive(Debug, PartialEq, Eq)]
pub struct Metadata {
    pub target_directory: PathBuf,
    pub version: String,
}

pub const APP_PACKAGE: &str = "monocode-gpui";
pub const APP_MANIFEST: &str = "crates/monocode-gpui/Cargo.toml";

pub fn metadata_args() -> Vec<&'static str> {
    vec![
        "metadata",
        "--manifest-path",
        APP_MANIFEST,
        "--locked",
        "--no-deps",
        "--format-version",
        "1",
    ]
}

pub fn parse_metadata(json: &str) -> Result<Metadata> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("Invalid cargo metadata: {e}"))?;
    let package = value["packages"]
        .as_array()
        .and_then(|packages| {
            packages
                .iter()
                .find(|p| p["name"].as_str() == Some(APP_PACKAGE))
        })
        .ok_or("The monocode-gpui package was not found.")?;
    Ok(Metadata {
        target_directory: value["target_directory"]
            .as_str()
            .ok_or("cargo metadata has no target_directory")?
            .into(),
        version: package["version"]
            .as_str()
            .ok_or("monocode-gpui has no version")?
            .into(),
    })
}

/// The build output folder, including `CARGO_BUILD_TARGET` when one is set.
pub fn target_dir(metadata: &Metadata, build_target: Option<&str>) -> PathBuf {
    match build_target.filter(|t| !t.is_empty()) {
        Some(triple) => metadata.target_directory.join(triple),
        None => metadata.target_directory.clone(),
    }
}

/// Copies a folder tree (files and folders; symlinks are recreated).
#[cfg(test)]
pub fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    let info = std::fs::symlink_metadata(from).map_err(|e| format!("{}: {e}", from.display()))?;
    if info.file_type().is_symlink() {
        #[cfg(unix)]
        {
            let link = std::fs::read_link(from).map_err(|e| e.to_string())?;
            std::os::unix::fs::symlink(link, to).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    if info.is_dir() {
        std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
        for entry in std::fs::read_dir(from).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        return Ok(());
    }
    std::fs::copy(from, to).map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))?;
    Ok(())
}

/// Runs `cargo xtask <command> <argv>` again with `feature` compiled in, for a
/// command whose heavy dependency the everyday build leaves out. The command
/// is named after its feature.
pub fn rerun_with_feature(feature: &str, argv: &[String]) -> Result<()> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    // Its own folder inside the target directory: Windows cannot replace the
    // xtask that is running this, nor one that is running `cargo xtask dev`.
    let target = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.parent()?.to_path_buf()))
        .unwrap_or_else(|| root().join("target"))
        .join(format!("xtask-{feature}"));
    let status = command(cargo)
        .args([
            "run",
            "--quiet",
            "--package",
            "xtask",
            "--features",
            feature,
            "--target-dir",
        ])
        .arg(target)
        .args(["--", feature])
        .args(argv)
        .current_dir(root())
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{feature} failed ({status})"))
    }
}

pub fn write(path: &Path, contents: impl AsRef<[u8]>) -> Result<()> {
    std::fs::write(path, contents).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn create_dir_all(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn copy(from: &Path, to: &Path) -> Result<()> {
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))
}

#[cfg(unix)]
pub fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(not(unix))]
pub fn set_mode(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_finds_the_app_and_its_target() {
        let metadata = parse_metadata(
            r#"{"target_directory":"/t","packages":[{"name":"x","version":"1.0.0"},{"name":"monocode-gpui","version":"0.15.0"}]}"#,
        )
        .unwrap();
        assert_eq!(
            metadata,
            Metadata {
                target_directory: "/t".into(),
                version: "0.15.0".into()
            }
        );
        assert_eq!(target_dir(&metadata, None), PathBuf::from("/t"));
        assert_eq!(
            target_dir(&metadata, Some("x86_64-pc-windows-msvc")),
            PathBuf::from("/t/x86_64-pc-windows-msvc")
        );
        assert!(parse_metadata(r#"{"target_directory":"/t","packages":[]}"#)
            .unwrap_err()
            .contains("not found"));
    }

    #[test]
    fn reads_the_workspace_version() {
        let version = workspace_version(&root()).unwrap();
        assert!(
            version.starts_with(|c: char| c.is_ascii_digit()),
            "the root Cargo.toml holds the app version, found {version:?}"
        );
    }
}
