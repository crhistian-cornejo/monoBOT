//! Poki (AGPL-3.0, github.com/JosssphZhou/poki) drives an iPhone from this
//! Mac over MCP. MonoBOT ships none of its code. When the user turns its
//! connector on, this downloads one pinned release, refuses it unless it is
//! byte for byte the one GitHub published, unpacks it into MonoBOT's own data
//! folder and runs its daemon while the connector is on. Nothing is installed
//! system-wide, nothing asks for admin rights, and no install script runs.

use serde::Serialize;
use sha2::{Digest, Sha256};
#[cfg(target_os = "macos")]
use std::path::PathBuf;
use std::path::{Component, Path};

/// The one release MonoBOT installs, as its GitHub release page lists it.
pub(crate) const VERSION: &str = "v0.1.0";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const URL: &str =
    "https://github.com/JosssphZhou/poki/releases/download/v0.1.0/poki-v0.1.0-macos-arm64.tar.gz";
pub(crate) const SIZE: u64 = 11_370_797;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const SHA256: &str = "bbf02b8763570fde09361f6867a04c2bea72126dc452c2db61c382236dee9a1c";
/// Every entry of the archive lives under this folder.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const FOLDER: &str = "poki-v0.1.0";
/// Written last, so a half-finished install never counts as one.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MARKER: &str = ".monobot-install.json";
/// The daemon's address and token while it runs; its stdio bridge reads it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const ENDPOINT: &str = "Library/Application Support/PokiMCP/mcp.json";
/// What only the user may decide, so the daemon never inherits it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const SCRUBBED_ENV: [&str; 3] = [
    // Pre-approves Poki's sensitive operations.
    "POKI_MCP_APPROVAL_MODE",
    // Moves the endpoint file away from where MonoBOT and the bridge look.
    "POKI_MCP_STATE_DIR",
    // Exposes every tool instead of Poki's default list.
    "POKI_MCP_TOOLS",
];

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PokiSetup {
    /// Whether this Mac can run the release (macOS on Apple Silicon).
    supported: bool,
    /// What it lacks to install or run Poki, as the user would install it.
    missing: Vec<String>,
    /// The folder MonoBOT installed, if it did.
    installed: Option<String>,
    /// Whether MonoBOT started the daemon and it still runs.
    running: bool,
    version: &'static str,
    size: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
struct Progress {
    stage: &'static str,
}

/// Lowercase hex SHA-256.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The download is the published release, or nothing is installed.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn check_download(bytes: &[u8]) -> Result<(), String> {
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
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn check_listing(names: &str, verbose: &str) -> Result<(), String> {
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

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn inside_folder(path: &str) -> bool {
    let mut parts = Path::new(path).components();
    matches!(parts.next(), Some(Component::Normal(first)) if first == FOLDER)
        && parts.all(|part| matches!(part, Component::Normal(_)))
}

/// The daemon's process id from its endpoint file, when it names a local one.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn endpoint_pid(raw: &str) -> Option<u32> {
    let entry: serde_json::Value = serde_json::from_str(raw).ok()?;
    if entry.get("enabled")?.as_bool()? != true {
        return None;
    }
    let url = entry.get("url")?.as_str()?;
    if !url.starts_with("http://127.0.0.1:") || !url.ends_with("/mcp") {
        return None;
    }
    u32::try_from(entry.get("pid")?.as_u64()?).ok()
}

/// The major version in `node -p process.versions.node` output.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn node_major(version: &str) -> Option<u32> {
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
    use crate::harness::{gui_search_path, resolve_gui_binary};
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, Stdio};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    use tauri::{AppHandle, Emitter, Manager};

    /// The daemon MonoBOT started, so it stops it and nothing else.
    static DAEMON: Mutex<Option<Child>> = Mutex::new(None);
    static INSTALL: Mutex<()> = Mutex::new(());

    fn root(app: &AppHandle) -> Result<PathBuf, String> {
        Ok(app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("connectors")
            .join("poki"))
    }

    fn endpoint_file() -> Option<PathBuf> {
        crate::dirs_home().map(|home| Path::new(&home).join(ENDPOINT))
    }

    fn installed(app: &AppHandle) -> Option<PathBuf> {
        let dir = root(app).ok()?.join(FOLDER);
        (dir.join(MARKER).is_file() && dir.join("src/stdio.mjs").is_file()).then_some(dir)
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

    fn running() -> bool {
        let mut daemon = DAEMON.lock().unwrap();
        match daemon.as_mut().map(|child| child.try_wait()) {
            Some(Ok(None)) => true,
            Some(_) => {
                *daemon = None;
                false
            }
            None => false,
        }
    }

    pub(super) fn setup(app: &AppHandle) -> PokiSetup {
        let supported = apple_silicon();
        PokiSetup {
            supported,
            missing: if supported { missing() } else { Vec::new() },
            installed: installed(app).map(|dir| dir.to_string_lossy().into_owned()),
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

    pub(super) fn install(app: &AppHandle) -> Result<String, String> {
        // A second switch flip waits for the first install instead of racing it.
        let _one_at_a_time = INSTALL.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(dir) = installed(app) {
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
        let root = root(app)?;
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let emit = |stage: &'static str| {
            let _ = app.emit("poki-install", Progress { stage });
        };

        emit("download");
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

        emit("verify");
        check_download(&bytes)?;

        emit("unpack");
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

            emit("dependencies");
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
        emit("done");
        Ok(target.to_string_lossy().into_owned())
    }

    fn live_endpoint_pid() -> Option<u32> {
        let raw = std::fs::read_to_string(endpoint_file()?).ok()?;
        let pid = endpoint_pid(&raw)?;
        // A daemon that crashed leaves its file behind.
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
        let log = std::fs::File::create(std::env::temp_dir().join("monobot-poki.log"))
            .map_err(|e| e.to_string())?;
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
            if let Ok(Some(status)) = child.try_wait() {
                let log = std::fs::read_to_string(std::env::temp_dir().join("monobot-poki.log"))
                    .unwrap_or_default();
                let tail: String = log.lines().rev().take(6).collect::<Vec<_>>().join("\n");
                return Err(format!("Poki stopped ({status}): {tail}"));
            }
            if live_endpoint_pid() == Some(pid) {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        *DAEMON.lock().unwrap() = Some(child);
        Ok(())
    }

    /// Stops the daemon MonoBOT started, letting it remove its endpoint file.
    pub(crate) fn stop() {
        let Some(mut child) = DAEMON.lock().unwrap().take() else {
            return;
        };
        let pid = child.id() as i32;
        unsafe { libc::kill(pid, libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        // Its own group: the device helpers it started go with it.
        unsafe { libc::kill(-pid, libc::SIGKILL) };
        let _ = child.wait();
    }
}

#[cfg(not(target_os = "macos"))]
const MAC_ONLY: &str = "Poki runs on macOS only";

#[tauri::command]
pub async fn poki_setup(app: tauri::AppHandle) -> Result<PokiSetup, String> {
    #[cfg(target_os = "macos")]
    return tauri::async_runtime::spawn_blocking(move || mac::setup(&app))
        .await
        .map_err(|e| e.to_string());
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Ok(PokiSetup {
            supported: false,
            missing: Vec::new(),
            installed: None,
            running: false,
            version: VERSION,
            size: SIZE,
        })
    }
}

/// Downloads, verifies and unpacks the pinned release; returns its folder.
#[tauri::command]
pub async fn poki_install(app: tauri::AppHandle) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    return tauri::async_runtime::spawn_blocking(move || mac::install(&app))
        .await
        .map_err(|e| e.to_string())?;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Err(MAC_ONLY.into())
    }
}

/// Starts Poki's daemon from `dir` unless one already runs.
#[tauri::command]
pub async fn poki_start(dir: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return tauri::async_runtime::spawn_blocking(move || mac::start(&dir))
        .await
        .map_err(|e| e.to_string())?;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = dir;
        Err(MAC_ONLY.into())
    }
}

/// Stops the daemon MonoBOT started; one the user started is left alone.
#[tauri::command]
pub async fn poki_stop() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    tauri::async_runtime::spawn_blocking(mac::stop)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// On quit.
pub(crate) fn shutdown() {
    #[cfg(target_os = "macos")]
    mac::stop();
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let good_names = "poki-v0.1.0/
poki-v0.1.0/src/stdio.mjs
";
        let good_kinds = format!(
            "{dir}
{file}
"
        );
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
        assert!(check_listing(
            "poki-v0.1.0/a
b",
            file
        )
        .is_err());
        assert!(check_listing("", "").is_err());
    }

    #[test]
    fn reads_the_daemon_endpoint_as_its_bridge_does() {
        let live = r#"{"url":"http://127.0.0.1:47825/mcp","port":47825,"token":"t","enabled":true,"pid":42}"#;
        assert_eq!(endpoint_pid(live), Some(42));
        assert_eq!(endpoint_pid(&live.replace("true", "false")), None);
        assert_eq!(endpoint_pid(&live.replace("127.0.0.1", "10.0.0.2")), None);
        assert_eq!(endpoint_pid("{}"), None);
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
}
