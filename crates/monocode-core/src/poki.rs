//! Poki (AGPL-3.0, github.com/JosssphZhou/poki) drives an iPhone from this
//! Mac over MCP. MonoBOT ships none of its code. When the user turns its
//! connector on, this downloads one pinned release, refuses it unless it is
//! byte for byte the one GitHub published, unpacks it into MonoBOT's own data
//! folder and runs its daemon while the connector is on. Nothing is installed
//! system-wide, nothing asks for admin rights, and no install script runs.
//!
//! Ported from src-tauri/src/poki.rs and src/features/settings/model/pokiSetup.ts.
//! Everything here blocks: callers run it on a background executor.
#![cfg_attr(not(target_os = "macos"), allow(dead_code, unused_imports))]

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

/// The one release MonoBOT installs, as its GitHub release page lists it.
pub const VERSION: &str = "v0.1.0";
const URL: &str =
    "https://github.com/JosssphZhou/poki/releases/download/v0.1.0/poki-v0.1.0-macos-arm64.tar.gz";
pub const SIZE: u64 = 11_370_797;
const SHA256: &str = "bbf02b8763570fde09361f6867a04c2bea72126dc452c2db61c382236dee9a1c";
/// Every entry of the archive lives under this folder.
const FOLDER: &str = "poki-v0.1.0";
/// Written last, so a half-finished install never counts as one.
const MARKER: &str = ".monobot-install.json";
/// The daemon's address and token while it runs; its stdio bridge reads it.
const ENDPOINT: &str = "Library/Application Support/PokiMCP/mcp.json";
/// The script Claude and Codex run, relative to Poki's folder.
pub const BRIDGE: &str = "src/stdio.mjs";
/// What only the user may decide, so the daemon never inherits it.
const SCRUBBED_ENV: [&str; 3] = [
    // Pre-approves Poki's sensitive operations.
    "POKI_MCP_APPROVAL_MODE",
    // Moves the endpoint file away from where MonoBOT and the bridge look.
    "POKI_MCP_STATE_DIR",
    // Exposes every tool instead of Poki's default list.
    "POKI_MCP_TOOLS",
];

/// What this Mac has for Poki, and whether MonoBOT installed or runs it.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
    /// Whether this Mac can run the release (macOS on Apple Silicon).
    pub supported: bool,
    /// What it lacks to install or run Poki, as the user would install it.
    pub missing: Vec<String>,
    /// The folder MonoBOT installed, if it did.
    pub installed: Option<String>,
    /// Whether MonoBOT started the daemon and it still runs.
    pub running: bool,
    pub version: &'static str,
    pub size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Download,
    Verify,
    Unpack,
    Dependencies,
    Done,
}

impl Stage {
    pub fn text(self) -> &'static str {
        match self {
            Stage::Download => "Downloading Poki…",
            Stage::Verify => "Checking it is the published release…",
            Stage::Unpack => "Unpacking into MonoBOT's folder…",
            Stage::Dependencies => "Installing its packages (no install scripts)…",
            Stage::Done => "Installed",
        }
    }
}

/// How far the setup got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// No folder saved yet.
    NoFolder,
    /// The folder has no `src/stdio.mjs`: not a Poki checkout.
    NoBridge,
    /// The daemon is not running (`npm start`).
    Stopped,
    Ready,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::NoFolder => "Not set up",
            Status::NoBridge => "Not a Poki folder",
            Status::Stopped => "Poki is not running",
            Status::Ready => "Ready",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Status::NoFolder => "Install it from here, or choose a folder where you cloned it.",
            Status::NoBridge => "There is no src/stdio.mjs in it. Pick the folder you cloned.",
            Status::Stopped => "Start it here; MonoBOT stops it when you turn this off or quit.",
            Status::Ready => "Give the connector to a Phone bot in its settings.",
        }
    }
}

/// What turning the switch on does next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    /// Not an Apple Silicon Mac: only the manual tutorial.
    Unsupported,
    /// Install these first; MonoBOT never installs them.
    Missing(Vec<String>),
    /// Ask, then download and verify the pinned release.
    Install,
    /// Use the copy MonoBOT already installed.
    Use(String),
    /// Start the daemon from the saved folder.
    Start(String),
    Ready,
}

pub fn next(folder: &str, status: Status, machine: &Machine) -> Next {
    if status == Status::Ready {
        return Next::Ready;
    }
    let folder = folder.trim();
    if !folder.is_empty() && status == Status::Stopped {
        return Next::Start(folder.to_owned());
    }
    if let Some(installed) = &machine.installed {
        return Next::Use(installed.clone());
    }
    // A folder that is not Poki's is the user's to fix in the field.
    if !folder.is_empty() {
        return Next::Ready;
    }
    if !machine.supported {
        return Next::Unsupported;
    }
    if !machine.missing.is_empty() {
        return Next::Missing(machine.missing.clone());
    }
    Next::Install
}

/// The question before downloading anything.
pub fn install_question(machine: &Machine) -> String {
    let megabytes = (machine.size as f64 / 1_000_000.).round() as u64;
    [
        format!(
            "Download Poki {} ({megabytes} MB, AGPL-3.0) from its GitHub release?",
            machine.version
        ),
        "MonoBOT checks it against the SHA-256 GitHub published and refuses anything else, unpacks it only inside its own data folder, and runs no install scripts. It needs no admin rights and changes nothing else on your Mac.".into(),
        "Poki's daemon runs while this connector is on and stops when you turn it off or quit.".into(),
    ]
    .join("\n\n")
}

/// Claude runs `node <folder>/src/stdio.mjs` without a shell, so `~` is not
/// expanded there: the folder is saved with the home folder spelled out.
pub fn expand_home(path: &str, home: &str) -> String {
    let trimmed = path.trim();
    if trimmed == "~" {
        return home.to_owned();
    }
    match trimmed.strip_prefix("~/") {
        Some(rest) => format!("{}/{rest}", home.trim_end_matches('/')),
        None => trimmed.to_owned(),
    }
}

/// Why a folder cannot be Poki's, if it cannot.
pub fn folder_problem(path: &str) -> Option<&'static str> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed.starts_with('/') || trimmed.starts_with("~/") {
        return None;
    }
    Some("Use the folder's full path, such as ~/poki")
}

/// Whether the daemon's file names a local endpoint, as the bridge checks.
pub fn endpoint_live(raw: &str) -> bool {
    let Ok(entry) = serde_json::from_str::<serde_json::Value>(raw) else {
        return false;
    };
    entry.get("enabled").and_then(|v| v.as_bool()) == Some(true)
        && entry
            .get("url")
            .and_then(|v| v.as_str())
            .and_then(|url| url::Url::parse(url).ok())
            .is_some_and(|url| {
                url.scheme() == "http"
                    && url.host_str() == Some("127.0.0.1")
                    && url.path() == "/mcp"
            })
}

fn endpoint_file() -> Option<PathBuf> {
    crate::scm::shell::dirs_home().map(|home| Path::new(&home).join(ENDPOINT))
}

/// How far the setup got. A daemon that crashed can leave its file behind;
/// the bridge then says it is not running, and so does the bot.
pub fn status(folder: &str) -> Status {
    let folder = folder.trim();
    if folder.is_empty() {
        return Status::NoFolder;
    }
    if !Path::new(&crate::connectors::checkout_file(folder, BRIDGE)).is_file() {
        return Status::NoBridge;
    }
    let raw = endpoint_file()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    if endpoint_live(&raw) {
        Status::Ready
    } else {
        Status::Stopped
    }
}

/// Lowercase hex SHA-256.
fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The download is the published release, or nothing is installed.
pub fn check_download(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 != SIZE || sha256_hex(bytes) != SHA256 {
        return Err(format!(
            "The download is not Poki {VERSION} as GitHub published it, so nothing was installed."
        ));
    }
    Ok(())
}

/// Reads the archive's listing before anything is unpacked: `names` from
/// `tar -tzf` (one path per line, exactly as stored) must all sit inside
/// `FOLDER`, and `verbose` from `tar -tvzf` must hold only plain files and
/// folders. Links, devices and paths that climb out are refused.
pub fn check_listing(names: &str, verbose: &str) -> Result<(), String> {
    let names: Vec<&str> = names.lines().filter(|line| !line.is_empty()).collect();
    if names.is_empty() {
        return Err("The archive is empty".into());
    }
    if let Some(path) = names.iter().find(|path| !inside_folder(path)) {
        return Err(format!("The archive writes outside its folder: {path}"));
    }
    let kinds: Vec<&str> = verbose.lines().filter(|line| !line.is_empty()).collect();
    if kinds.len() != names.len() {
        return Err("The archive's two listings disagree".into());
    }
    if let Some(line) = kinds
        .iter()
        .find(|line| !matches!(line.chars().next(), Some('-' | 'd')))
    {
        return Err(format!("The archive holds a link or special file: {line}"));
    }
    Ok(())
}

fn inside_folder(path: &str) -> bool {
    let mut parts = Path::new(path).components();
    matches!(parts.next(), Some(Component::Normal(first)) if first == FOLDER)
        && parts.all(|part| matches!(part, Component::Normal(_)))
}

/// The daemon's process id from its endpoint file, when it names a local one.
pub fn endpoint_pid(raw: &str) -> Option<u32> {
    let entry: serde_json::Value = serde_json::from_str(raw).ok()?;
    if !entry.get("enabled")?.as_bool()? {
        return None;
    }
    let url = entry.get("url")?.as_str()?;
    if !url.starts_with("http://127.0.0.1:") || !url.ends_with("/mcp") {
        return None;
    }
    u32::try_from(entry.get("pid")?.as_u64()?).ok()
}

/// The major version in `node -p process.versions.node` output.
pub fn node_major(version: &str) -> Option<u32> {
    version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .next()?
        .parse()
        .ok()
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use crate::scm::{resolve_gui_binary, terminal_search_path as gui_search_path};
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, Stdio};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    /// The daemon MonoBOT started, so it stops it and nothing else.
    static DAEMON: Mutex<Option<Child>> = Mutex::new(None);
    static INSTALL: Mutex<()> = Mutex::new(());

    fn root(data_dir: &Path) -> PathBuf {
        data_dir.join("connectors").join("poki")
    }

    fn installed(data_dir: &Path) -> Option<PathBuf> {
        let dir = root(data_dir).join(FOLDER);
        (dir.join(MARKER).is_file() && dir.join(BRIDGE).is_file()).then_some(dir)
    }

    fn apple_silicon() -> bool {
        Command::new("/usr/sbin/sysctl")
            .args(["-n", "hw.optional.arm64"])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim() == "1")
            .unwrap_or(false)
    }

    fn quiet(program: &Path, args: &[&str]) -> Option<String> {
        let out = Command::new(program)
            .args(args)
            .env("PATH", gui_search_path())
            .stdin(Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// What Poki's own setup checks for the prebuilt package.
    fn missing() -> Vec<String> {
        let mut missing = Vec::new();
        let node_ok = resolve_gui_binary("node")
            .and_then(|node| quiet(&node, &["-p", "process.versions.node"]))
            .and_then(|version| node_major(&version))
            .is_some_and(|major| major >= 24);
        if !node_ok {
            missing.push("Node.js 24 or later (nodejs.org or brew install node)".into());
        }
        if resolve_gui_binary("npm").is_none() {
            missing.push("npm, which comes with Node.js".into());
        }
        if resolve_gui_binary("ffmpeg").is_none() || resolve_gui_binary("ffprobe").is_none() {
            missing.push("FFmpeg (brew install ffmpeg)".into());
        }
        if resolve_gui_binary("python3").is_none() {
            missing.push("Python 3".into());
        }
        let xcode = resolve_gui_binary("xcodebuild")
            .and_then(|tool| quiet(&tool, &["-version"]))
            .is_some();
        if !xcode {
            missing.push("The full Xcode from the App Store, opened once".into());
        }
        missing
    }

    pub(super) fn running() -> bool {
        let mut daemon = DAEMON.lock().unwrap_or_else(|e| e.into_inner());
        match daemon.as_mut().map(|child| child.try_wait()) {
            Some(Ok(None)) => true,
            Some(_) => {
                *daemon = None;
                false
            }
            None => false,
        }
    }

    pub(super) fn machine(data_dir: &Path) -> Machine {
        let supported = apple_silicon();
        Machine {
            supported,
            missing: if supported { missing() } else { Vec::new() },
            installed: installed(data_dir).map(|dir| dir.to_string_lossy().into_owned()),
            running: running(),
            version: VERSION,
            size: SIZE,
        }
    }

    fn run(cmd: &mut Command, what: &str) -> Result<String, String> {
        let out = cmd
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("{what}: {e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let tail: String = err.lines().rev().take(6).collect::<Vec<_>>().join("\n");
            return Err(format!("{what} failed: {tail}"));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// No link may survive unpacking, wherever tar put it.
    fn no_links(dir: &Path) -> Result<(), String> {
        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_symlink() {
                return Err(format!("Refused a link: {}", entry.path().display()));
            }
            if kind.is_dir() {
                no_links(&entry.path())?;
            }
        }
        Ok(())
    }

    pub(super) fn install(data_dir: &Path, progress: &dyn Fn(Stage)) -> Result<String, String> {
        // A second switch flip waits for the first install instead of racing it.
        let _one_at_a_time = INSTALL.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(dir) = installed(data_dir) {
            return Ok(dir.to_string_lossy().into_owned());
        }
        if !apple_silicon() {
            return Err("Poki's package runs on Apple Silicon Macs only; follow the tutorial to build it yourself.".into());
        }
        let lacking = missing();
        if !lacking.is_empty() {
            return Err(format!("Install first: {}", lacking.join("; ")));
        }
        let npm = resolve_gui_binary("npm").ok_or("npm not found")?;
        let root = root(data_dir);
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;

        progress(Stage::Download);
        let response = monobot_http::agent()
            .timeout(Duration::from_secs(300))
            .build()
            .get(URL)
            .call()
            .map_err(|e| format!("Could not download Poki: {e}"))?;
        let mut bytes = Vec::with_capacity(SIZE as usize);
        response
            .into_reader()
            .take(SIZE + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("Could not download Poki: {e}"))?;

        progress(Stage::Verify);
        check_download(&bytes)?;

        progress(Stage::Unpack);
        let archive = root.join("download.tar.gz");
        let staging = root.join(".staging");
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
        std::fs::write(&archive, &bytes).map_err(|e| e.to_string())?;
        let result = (|| -> Result<PathBuf, String> {
            let names = run(
                Command::new("/usr/bin/tar").arg("-tzf").arg(&archive),
                "Reading the archive",
            )?;
            let verbose = run(
                Command::new("/usr/bin/tar").arg("-tvzf").arg(&archive),
                "Reading the archive",
            )?;
            check_listing(&names, &verbose)?;
            run(
                Command::new("/usr/bin/tar")
                    .arg("-xzf")
                    .arg(&archive)
                    .arg("--no-same-owner")
                    .arg("-C")
                    .arg(&staging),
                "Unpacking",
            )?;
            no_links(&staging)?;
            let unpacked = staging.join(FOLDER);

            progress(Stage::Dependencies);
            // The lockfile pins every package and its integrity hash; no
            // install script runs (sharp's macOS build comes prebuilt).
            run(
                Command::new(&npm)
                    .args(["ci", "--ignore-scripts", "--no-audit", "--no-fund"])
                    .current_dir(&unpacked)
                    .env("PATH", gui_search_path()),
                "npm ci",
            )?;
            std::fs::write(
                unpacked.join(MARKER),
                serde_json::json!({ "version": VERSION, "sha256": SHA256, "url": URL }).to_string(),
            )
            .map_err(|e| e.to_string())?;
            let target = root.join(FOLDER);
            let _ = std::fs::remove_dir_all(&target);
            std::fs::rename(&unpacked, &target).map_err(|e| e.to_string())?;
            Ok(target)
        })();
        let _ = std::fs::remove_file(&archive);
        let _ = std::fs::remove_dir_all(&staging);
        let target = result?;
        progress(Stage::Done);
        Ok(target.to_string_lossy().into_owned())
    }

    fn live_endpoint_pid() -> Option<u32> {
        let raw = std::fs::read_to_string(endpoint_file()?).ok()?;
        let pid = endpoint_pid(&raw)?;
        // A daemon that crashed leaves its file behind.
        // SAFETY: signal 0 only checks that the process exists.
        (unsafe { libc::kill(pid as i32, 0) } == 0).then_some(pid)
    }

    pub(super) fn start(dir: &str) -> Result<(), String> {
        if running() || live_endpoint_pid().is_some() {
            return Ok(());
        }
        let dir = PathBuf::from(dir);
        if !dir.is_absolute() || !dir.join("src/daemon.mjs").is_file() {
            return Err("That folder is not a Poki checkout".into());
        }
        if !dir.join("node_modules").is_dir() {
            return Err("Run npm ci in the Poki folder first".into());
        }
        let node = resolve_gui_binary("node").ok_or("Node.js not found")?;
        let log_path = std::env::temp_dir().join("monobot-poki.log");
        let log = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;
        let mut cmd = Command::new(node);
        cmd.arg("src/daemon.mjs")
            .current_dir(&dir)
            .env("PATH", gui_search_path())
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log)
            .process_group(0);
        for key in SCRUBBED_ENV {
            cmd.env_remove(key);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Could not start Poki: {e}"))?;
        let pid = child.id();
        // Ready once its endpoint file names this process.
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if let Ok(Some(exit)) = child.try_wait() {
                let log = std::fs::read_to_string(&log_path).unwrap_or_default();
                let tail: String = log.lines().rev().take(6).collect::<Vec<_>>().join("\n");
                return Err(format!("Poki stopped ({exit}): {tail}"));
            }
            if live_endpoint_pid() == Some(pid) {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        *DAEMON.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
        Ok(())
    }

    /// Stops the daemon MonoBOT started, letting it remove its endpoint file.
    pub(super) fn stop() {
        let Some(mut child) = DAEMON.lock().unwrap_or_else(|e| e.into_inner()).take() else {
            return;
        };
        let pid = child.id() as i32;
        // SAFETY: the pid is a child this process started and still owns.
        unsafe { libc::kill(pid, libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        // Its own group: the device helpers it started go with it.
        // SAFETY: as above, for the group the child leads.
        unsafe { libc::kill(-pid, libc::SIGKILL) };
        let _ = child.wait();
    }
}

#[cfg(not(target_os = "macos"))]
const MAC_ONLY: &str = "Poki runs on macOS only";

/// What this computer has for Poki. `data_dir` is the app's data folder.
pub fn machine(data_dir: &Path) -> Machine {
    #[cfg(target_os = "macos")]
    return mac::machine(data_dir);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = data_dir;
        Machine {
            supported: false,
            missing: Vec::new(),
            installed: None,
            running: false,
            version: VERSION,
            size: SIZE,
        }
    }
}

/// Downloads, verifies and unpacks the pinned release; returns its folder.
pub fn install(data_dir: &Path, progress: &dyn Fn(Stage)) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    return mac::install(data_dir, progress);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (data_dir, progress);
        Err(MAC_ONLY.into())
    }
}

/// Starts Poki's daemon from `dir` unless one already runs.
pub fn start(dir: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return mac::start(dir);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = dir;
        Err(MAC_ONLY.into())
    }
}

/// Stops the daemon MonoBOT started; one the user started is left alone.
pub fn stop() {
    #[cfg(target_os = "macos")]
    mac::stop();
}

/// On quit.
pub fn shutdown() {
    stop();
}

/// Before a turn that has Poki: brings the daemon up when it is not, so the
/// phone tools find it. Failing is fine here; the bot says Poki is not running.
pub fn ensure_started(folder: &str) {
    let folder = folder.trim();
    if folder.is_empty() {
        return;
    }
    let _ = start(folder);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine_with(supported: bool, missing: &[&str], installed: Option<&str>) -> Machine {
        Machine {
            supported,
            missing: missing.iter().map(|m| (*m).to_owned()).collect(),
            installed: installed.map(str::to_owned),
            running: false,
            version: VERSION,
            size: SIZE,
        }
    }

    #[test]
    fn refuses_anything_but_the_published_release() {
        assert!(check_download(b"not poki").is_err());
        let mut wrong = vec![0u8; SIZE as usize];
        wrong[0] = 1;
        assert!(check_download(&wrong).is_err());
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn unpacks_only_plain_files_inside_its_folder() {
        let dir = "drwxr-xr-x  0 a staff 0 Nov  5 10:43 poki-v0.1.0/";
        let file = "-rw-r--r--  0 a staff 9 Nov  5 10:43 poki-v0.1.0/src/stdio.mjs";
        let good_names = "poki-v0.1.0/\npoki-v0.1.0/src/stdio.mjs\n";
        let good_kinds = format!("{dir}\n{file}\n");
        assert!(check_listing(good_names, &good_kinds).is_ok());
        // A name with spaces is read whole, not by its last word.
        assert!(check_listing("poki-v0.1.0/a b.txt", file).is_ok());
        for name in [
            "poki-v0.1.0/../../.zshrc",
            "/usr/local/bin/node",
            "elsewhere/file",
            "../../evil poki-v0.1.0/ok",
            "poki-v0.1.0",
        ] {
            let ok = name == "poki-v0.1.0";
            assert_eq!(check_listing(name, file).is_ok(), ok, "{name}");
        }
        for kind in [
            "lrwxr-xr-x  0 a staff 0 Nov  5 10:43 poki-v0.1.0/x -> /etc/passwd",
            "hrw-r--r--  0 a staff 0 Nov  5 10:43 poki-v0.1.0/x link to poki-v0.1.0/y",
            "crw-r--r--  0 a staff 9 Nov  5 10:43 poki-v0.1.0/dev",
        ] {
            assert!(check_listing("poki-v0.1.0/x", kind).is_err(), "{kind}");
        }
        // A name split over two lines makes the listings disagree.
        assert!(check_listing("poki-v0.1.0/a\nb", file).is_err());
        assert!(check_listing("", "").is_err());
    }

    #[test]
    fn reads_the_daemon_endpoint_as_its_bridge_does() {
        let live = r#"{"url":"http://127.0.0.1:47825/mcp","port":47825,"token":"t","enabled":true,"pid":42}"#;
        assert_eq!(endpoint_pid(live), Some(42));
        assert_eq!(endpoint_pid(&live.replace("true", "false")), None);
        assert_eq!(endpoint_pid(&live.replace("127.0.0.1", "10.0.0.2")), None);
        assert_eq!(endpoint_pid("{}"), None);
        assert!(endpoint_live(live));
        assert!(!endpoint_live(&live.replace("true", "false")));
        assert!(!endpoint_live(&live.replace("127.0.0.1", "10.0.0.2")));
        assert!(!endpoint_live(&live.replace("/mcp", "/other")));
        assert!(!endpoint_live(&live.replace("http:", "https:")));
        assert!(!endpoint_live("not json"));
        assert!(!endpoint_live(""));
    }

    #[test]
    fn checks_node_24() {
        assert_eq!(node_major("24.3.0\n"), Some(24));
        assert_eq!(node_major("v22.1.0"), Some(22));
        assert_eq!(node_major(""), None);
    }

    #[test]
    fn never_passes_on_the_approval_mode() {
        assert!(SCRUBBED_ENV.contains(&"POKI_MCP_APPROVAL_MODE"));
    }

    #[test]
    fn spells_out_the_home_folder_for_the_bridge() {
        assert_eq!(expand_home(" ~/poki ", "/Users/me/"), "/Users/me/poki");
        assert_eq!(expand_home("~", "/Users/me"), "/Users/me");
        assert_eq!(expand_home("/opt/poki", "/Users/me"), "/opt/poki");
        assert_eq!(expand_home("~other/poki", "/Users/me"), "~other/poki");
    }

    #[test]
    fn asks_for_a_full_path() {
        assert_eq!(folder_problem(""), None);
        assert_eq!(folder_problem("  "), None);
        assert_eq!(folder_problem("/opt/poki"), None);
        assert_eq!(folder_problem("~/poki"), None);
        assert!(folder_problem("poki").is_some());
        assert!(folder_problem("../poki").is_some());
    }

    #[test]
    fn a_saved_folder_decides_before_the_machine_does() {
        let mac = machine_with(true, &[], None);
        assert_eq!(next("/p", Status::Ready, &mac), Next::Ready);
        assert_eq!(
            next(" /p ", Status::Stopped, &mac),
            Next::Start("/p".into())
        );
        // A folder that is not Poki's is left for the user to fix.
        assert_eq!(next("/p", Status::NoBridge, &mac), Next::Ready);
    }

    #[test]
    fn the_installed_copy_is_used_before_installing_again() {
        let mac = machine_with(true, &[], Some("/data/poki-v0.1.0"));
        assert_eq!(
            next("", Status::NoFolder, &mac),
            Next::Use("/data/poki-v0.1.0".into())
        );
    }

    #[test]
    fn installing_waits_for_support_and_the_tools_it_needs() {
        assert_eq!(
            next("", Status::NoFolder, &machine_with(false, &[], None)),
            Next::Unsupported
        );
        assert_eq!(
            next(
                "",
                Status::NoFolder,
                &machine_with(true, &["Python 3"], None)
            ),
            Next::Missing(vec!["Python 3".into()])
        );
        assert_eq!(
            next("", Status::NoFolder, &machine_with(true, &[], None)),
            Next::Install
        );
    }

    #[test]
    fn the_question_names_the_size_and_the_checks() {
        let text = install_question(&machine_with(true, &[], None));
        assert!(text.contains("Poki v0.1.0 (11 MB, AGPL-3.0)"));
        assert!(text.contains("SHA-256"));
        assert!(text.contains("no install scripts"));
    }

    #[test]
    fn status_reads_the_folder_then_the_daemon() {
        assert_eq!(status(""), Status::NoFolder);
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(status(dir.path().to_str().unwrap()), Status::NoBridge);
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join(BRIDGE), "").unwrap();
        // Ready needs the daemon's file, which this test does not write.
        assert_ne!(status(dir.path().to_str().unwrap()), Status::NoBridge);
        assert_ne!(status(dir.path().to_str().unwrap()), Status::NoFolder);
    }
}
