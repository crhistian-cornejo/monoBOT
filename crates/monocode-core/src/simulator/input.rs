//! Touches, typing, hardware buttons and the accessibility tree, through an
//! XCUITest runner (`runner/`) that monoBOT builds from source on this Mac
//! and starts on the device. The runner serves a small HTTP API on the
//! loopback interface, guarded by a token, and quits after half an hour
//! without requests. It uses only public XCTest APIs, which keep working
//! across Xcode versions; CoreSimulator's own HID channel changed in Xcode 27.
//!
//! One runner per device serves every agent session: its port and token sit
//! in the user's caches, readable only by the user.
use super::{run, tail, XCRUN};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime},
};

/// The runner's sources, as `runner/` holds them (regenerated with xcodegen).
const SOURCES: [(&str, &str); 7] = [
    (
        "MonoRunner.xcodeproj/project.pbxproj",
        include_str!("runner/MonoRunner.xcodeproj/project.pbxproj"),
    ),
    (
        "MonoRunner.xcodeproj/project.xcworkspace/contents.xcworkspacedata",
        include_str!("runner/MonoRunner.xcodeproj/project.xcworkspace/contents.xcworkspacedata"),
    ),
    (
        "MonoRunner.xcodeproj/xcshareddata/xcschemes/MonoRunner.xcscheme",
        include_str!("runner/MonoRunner.xcodeproj/xcshareddata/xcschemes/MonoRunner.xcscheme"),
    ),
    (
        "MonoRunner/App.swift",
        include_str!("runner/MonoRunner/App.swift"),
    ),
    (
        "MonoRunnerUITests/Runner.swift",
        include_str!("runner/MonoRunnerUITests/Runner.swift"),
    ),
    (
        "MonoRunnerUITests/Server.swift",
        include_str!("runner/MonoRunnerUITests/Server.swift"),
    ),
    (
        "MonoRunnerUITests/Tree.swift",
        include_str!("runner/MonoRunnerUITests/Tree.swift"),
    ),
];
/// How long the runner waits without a request before it ends.
const IDLE_SECONDS: u32 = 1800;
/// A build left by a process that died is taken over after this long.
const STALE_BUILD: Duration = Duration::from_secs(600);

fn root() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("com.monocode.desktop/simulator-runner")
}

/// Names one build of the runner: its sources and the Xcode that builds it.
pub(crate) fn build_key(xcode_version: &str) -> String {
    let mut digest = Sha256::new();
    for (path, text) in SOURCES {
        digest.update(path.as_bytes());
        digest.update([0]);
        digest.update(text.as_bytes());
        digest.update([0]);
    }
    digest.update(xcode_version.trim().as_bytes());
    digest.finalize()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The `.xctestrun` file a finished build leaves in its products folder.
fn xctestrun(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir.join("DerivedData/Build/Products"))
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "xctestrun"))
}

/// Removes the build lock when the build ends, however it ends.
struct BuildLock(PathBuf);

impl Drop for BuildLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn take_lock(path: &Path) -> Option<BuildLock> {
    if File::create_new(path).is_ok() {
        return Some(BuildLock(path.to_owned()));
    }
    let stale = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|at| SystemTime::now().duration_since(at).ok())
        .is_some_and(|age| age > STALE_BUILD);
    if stale {
        let _ = std::fs::remove_file(path);
        return File::create_new(path)
            .ok()
            .map(|_| BuildLock(path.to_owned()));
    }
    None
}

/// The build this process uses: its sources with the selected Xcode.
fn current_key() -> Result<String, String> {
    static KEY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    if let Some(key) = KEY.get() {
        return Ok(key.clone());
    }
    let version = run(XCRUN, &["xcodebuild", "-version"], Duration::from_secs(30))?
        .ok("xcodebuild -version")?;
    Ok(KEY.get_or_init(|| build_key(&version)).clone())
}

/// Builds the runner for this Xcode once, and returns its `.xctestrun`.
fn ensure_built(key: &str) -> Result<PathBuf, String> {
    let builds = root().join("builds");
    let dir = builds.join(key);
    if let Some(found) = xctestrun(&dir) {
        return Ok(found);
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let deadline = Instant::now() + STALE_BUILD;
    let _lock = loop {
        if let Some(lock) = take_lock(&dir.join(".building")) {
            break lock;
        }
        // Another session is building it; use theirs when it lands.
        if let Some(found) = xctestrun(&dir) {
            return Ok(found);
        }
        if Instant::now() >= deadline {
            return Err(
                "Another session has been building the simulator runner for too long".into(),
            );
        }
        std::thread::sleep(Duration::from_millis(500));
    };
    if let Some(found) = xctestrun(&dir) {
        return Ok(found);
    }
    let source = dir.join("source");
    for (path, text) in SOURCES {
        let path = source.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let project = source.join("MonoRunner.xcodeproj");
    let derived = dir.join("DerivedData");
    let out = run(
        XCRUN,
        &[
            "xcodebuild",
            "build-for-testing",
            "-project",
            &project.to_string_lossy(),
            "-scheme",
            "MonoRunner",
            "-destination",
            "generic/platform=iOS Simulator",
            "-derivedDataPath",
            &derived.to_string_lossy(),
            "-skipMacroValidation",
        ],
        Duration::from_secs(900),
    )?;
    if out.code != Some(0) {
        let log = format!("{}\n{}", out.stdout, out.stderr);
        let errors: Vec<&str> = log
            .lines()
            .filter(|line| line.contains("error:"))
            .take(10)
            .collect();
        return Err(format!(
            "The simulator runner did not build:\n{}",
            if errors.is_empty() {
                tail(&log, 3000).to_owned()
            } else {
                errors.join("\n")
            }
        ));
    }
    // Earlier builds belong to other Xcode versions or older sources.
    if let Ok(entries) = std::fs::read_dir(&builds) {
        for entry in entries.flatten() {
            if entry.path() != dir {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    xctestrun(&dir).ok_or_else(|| "The runner built but left no .xctestrun file".into())
}

/// A runner serving one device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Runner {
    pub port: u16,
    pub token: String,
    /// The build serving: a runner from older sources or another Xcode is replaced.
    pub key: String,
}

/// Why a call to a runner failed.
#[derive(Debug)]
pub(crate) enum Failure {
    /// Nothing answers there: start a runner.
    Unreachable,
    /// The runner answered with an error, or broke off.
    Said(String),
}

impl Runner {
    fn state_path(udid: &str) -> PathBuf {
        root().join("devices").join(format!("{udid}.json"))
    }

    fn saved(udid: &str) -> Option<Self> {
        let text = std::fs::read_to_string(Self::state_path(udid)).ok()?;
        let value: Value = serde_json::from_str(&text).ok()?;
        Some(Self {
            port: u16::try_from(value["port"].as_u64()?).ok()?,
            token: value["token"].as_str()?.to_owned(),
            key: value["key"].as_str().unwrap_or_default().to_owned(),
        })
    }

    fn save(&self, udid: &str) -> Result<(), String> {
        let path = Self::state_path(udid);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let text = json!({"port": self.port, "token": self.token, "key": self.key}).to_string();
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        use std::io::Write;
        options
            .open(&path)
            .and_then(|mut file| file.write_all(text.as_bytes()))
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn call(&self, action: &str, body: &Value, timeout: Duration) -> Result<Value, Failure> {
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .timeout_connect(Duration::from_secs(2))
            .timeout(timeout)
            .build();
        let response = agent
            .post(&format!("http://127.0.0.1:{}/{action}", self.port))
            .set("X-Monobot-Token", &self.token)
            .set("Content-Type", "application/json")
            .send_string(&body.to_string());
        let response = match response {
            Ok(response) | Err(ureq::Error::Status(_, response)) => response,
            Err(ureq::Error::Transport(error)) => {
                return Err(if error.kind() == ureq::ErrorKind::ConnectionFailed {
                    Failure::Unreachable
                } else {
                    Failure::Said(format!("The simulator runner did not answer: {error}"))
                })
            }
        };
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(4 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .map_err(|e| Failure::Said(e.to_string()))?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| Failure::Said("The simulator runner sent an unreadable answer".into()))?;
        if value["ok"] == Value::Bool(true) {
            Ok(value["result"].clone())
        } else {
            Err(Failure::Said(
                value["error"]
                    .as_str()
                    .unwrap_or("The simulator runner refused the request")
                    .to_owned(),
            ))
        }
    }
}

fn free_port() -> Result<u16, String> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .map_err(|e| e.to_string())
}

/// Starts a runner on a booted device and waits until it answers.
fn start(udid: &str, key: &str) -> Result<Runner, String> {
    let xctestrun = ensure_built(key)?;
    let runner = Runner {
        port: free_port()?,
        token: format!("{:032x}", rand::random::<u128>()),
        key: key.to_owned(),
    };
    let logs = root().join("logs");
    std::fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
    let log_path = logs.join(format!("{udid}.log"));
    let log = File::create(&log_path).map_err(|e| format!("{}: {e}", log_path.display()))?;
    let results = root().join("results").join(udid);
    // Each run leaves a result bundle; only the newest one is of any use.
    let _ = std::fs::remove_dir_all(&results);
    let mut command = Command::new(XCRUN);
    command
        .args(["xcodebuild", "test-without-building", "-xctestrun"])
        .arg(&xctestrun)
        .args(["-destination", &format!("id={udid}")])
        .arg("-derivedDataPath")
        .arg(&results)
        .env("TEST_RUNNER_MONOBOT_RUNNER_PORT", runner.port.to_string())
        .env("TEST_RUNNER_MONOBOT_RUNNER_TOKEN", &runner.token)
        .env("TEST_RUNNER_MONOBOT_RUNNER_IDLE", IDLE_SECONDS.to_string())
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    // Its own process group: the runner outlives the agent session that
    // started it and serves the next one, until it has been idle a while.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|e| format!("xcodebuild: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        match runner.call("status", &json!({}), Duration::from_secs(5)) {
            Ok(_) => break,
            Err(Failure::Said(error)) => {
                let _ = child.kill();
                return Err(error);
            }
            Err(Failure::Unreachable) => {}
        }
        if let Ok(Some(status)) = child.try_wait() {
            let log = std::fs::read_to_string(&log_path).unwrap_or_default();
            return Err(format!(
                "The simulator runner stopped ({status}) before it was ready:\n{}",
                tail(&log, 3000)
            ));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "The simulator runner did not start within 3 minutes. Its log: {}",
                log_path.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    runner.save(udid)?;
    // Reaps it when it ends, while this process still runs.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(runner)
}

/// Runs `action` on the device's runner, starting one when none answers.
pub fn act(udid: &str, action: &str, body: &Value, timeout: Duration) -> Result<Value, String> {
    let key = current_key()?;
    if let Some(runner) = Runner::saved(udid) {
        if runner.key == key {
            match runner.call(action, body, timeout) {
                Ok(result) => return Ok(result),
                Err(Failure::Said(error)) => return Err(error),
                Err(Failure::Unreachable) => {}
            }
        } else {
            // Built from older sources or for another Xcode: end it so the
            // device has one runner, the current one.
            let _ = runner.call("stop", &json!({}), Duration::from_secs(5));
        }
    }
    let runner = start(udid, &key)?;
    runner
        .call(action, body, timeout)
        .map_err(|failure| match failure {
            Failure::Unreachable => "The simulator runner stopped answering".into(),
            Failure::Said(error) => error,
        })
}
