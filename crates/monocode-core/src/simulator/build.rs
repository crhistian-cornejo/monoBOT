//! `xcodebuild` for the simulator: a build runs in the background while the
//! agent asks for its progress, and on success the built `.app` is located
//! from the build settings so `launch` can install it.
use super::{absolute, run, tail, XCRUN};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Condvar, Mutex, OnceLock},
    time::{Duration, Instant},
};

/// Errors and warnings kept per build; the log holds every one.
const KEPT_DIAGNOSTICS: usize = 30;
/// Folders no project lives in, or that hold thousands of files.
const SKIPPED: [&str; 7] = [
    "node_modules",
    "Pods",
    "build",
    "DerivedData",
    "target",
    "Carthage",
    ".build",
];

/// An Xcode project or workspace named by an absolute path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Container {
    pub path: PathBuf,
    pub workspace: bool,
}

pub(crate) fn container(path: &str) -> Result<Container, String> {
    let path = absolute(path, "project_path")?;
    let workspace = match path.extension().and_then(|ext| ext.to_str()) {
        Some("xcworkspace") => true,
        Some("xcodeproj") => false,
        _ => {
            return Err(format!(
                "project_path must be an .xcodeproj or .xcworkspace (got '{}'). Call find_projects to list them.",
                path.display()
            ))
        }
    };
    let path = path
        .canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Container { path, workspace })
}

/// The arguments that name what to build, before the action.
pub(crate) fn selection(
    container: &Container,
    scheme: &str,
    configuration: &str,
    destination: &str,
    derived_data: &Path,
) -> Vec<String> {
    [
        if container.workspace {
            "-workspace"
        } else {
            "-project"
        },
        &container.path.to_string_lossy(),
        "-scheme",
        scheme,
        "-configuration",
        configuration,
        "-destination",
        destination,
        "-derivedDataPath",
        &derived_data.to_string_lossy(),
        "-skipMacroValidation",
    ]
    .iter()
    .map(|arg| (*arg).to_owned())
    .collect()
}

/// Where a project's builds go: its own folder in the user's caches, so a
/// build never mixes with Xcode's DerivedData or another project's.
pub(crate) fn build_root(container: &Path) -> PathBuf {
    let digest = Sha256::digest(container.to_string_lossy().as_bytes());
    let id: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("com.monocode.desktop/simulator-builds")
        .join(id)
}

/// Projects and workspaces under `dir`, three folders deep at most. Never
/// descends into a bundle, so a project's own workspace is not listed.
pub fn find_projects(dir: &str) -> Result<Vec<String>, String> {
    let dir = absolute(dir, "directory")?;
    if !dir.is_dir() {
        return Err(format!("{} is not a folder", dir.display()));
    }
    let mut found = Vec::new();
    let mut queue = vec![(dir.to_path_buf(), 0)];
    while let Some((folder, depth)) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name.starts_with('.') || SKIPPED.contains(&name.as_str()) || !path.is_dir() {
                continue;
            }
            if name.ends_with(".xcodeproj") || name.ends_with(".xcworkspace") {
                found.push(path.to_string_lossy().into_owned());
                if found.len() >= 50 {
                    return Ok(found);
                }
            } else if depth < 3 {
                queue.push((path, depth + 1));
            }
        }
    }
    // Workspaces first: they build a project together with its packages and pods.
    found.sort_by_key(|path| (!path.ends_with(".xcworkspace"), path.matches('/').count()));
    Ok(found)
}

/// The schemes and targets `xcodebuild -list -json` reports.
pub(crate) fn parse_list(json: &str) -> Result<Value, String> {
    let value: Value = serde_json::from_str(json).map_err(|e| format!("xcodebuild -list: {e}"))?;
    let body = value
        .get("workspace")
        .or_else(|| value.get("project"))
        .ok_or("xcodebuild -list reported neither a project nor a workspace")?;
    Ok(json!({
        "schemes": body["schemes"],
        "targets": body.get("targets").cloned().unwrap_or(Value::Null),
        "configurations": body.get("configurations").cloned().unwrap_or(Value::Null),
    }))
}

pub fn schemes(project_path: &str) -> Result<Value, String> {
    let container = container(project_path)?;
    let path = container.path.to_string_lossy().into_owned();
    let kind = if container.workspace {
        "-workspace"
    } else {
        "-project"
    };
    let json = run(
        XCRUN,
        &["xcodebuild", "-list", "-json", kind, &path],
        Duration::from_secs(120),
    )?
    .ok("xcodebuild -list")?;
    parse_list(&json)
}

/// The built app in `xcodebuild -showBuildSettings -json`: the application
/// target built for the simulator, and its bundle identifier.
pub(crate) fn find_app(json: &str) -> Option<(String, Option<String>)> {
    let targets: Vec<Value> = serde_json::from_str(json).ok()?;
    let apps: Vec<&Value> = targets
        .iter()
        .map(|target| &target["buildSettings"])
        .filter(|settings| {
            settings["PRODUCT_TYPE"].as_str() == Some("com.apple.product-type.application")
        })
        .collect();
    let app = apps
        .iter()
        .find(|settings| settings["PLATFORM_NAME"].as_str() == Some("iphonesimulator"))
        .or_else(|| apps.first())?;
    let dir = app["TARGET_BUILD_DIR"].as_str()?;
    let name = app["FULL_PRODUCT_NAME"]
        .as_str()
        .or_else(|| app["WRAPPER_NAME"].as_str())?;
    Some((
        Path::new(dir).join(name).to_string_lossy().into_owned(),
        app["PRODUCT_BUNDLE_IDENTIFIER"].as_str().map(str::to_owned),
    ))
}

/// What a line of build output says, if it is an error or a warning.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Diagnostic<'a> {
    Error(&'a str),
    Warning(&'a str),
}

pub(crate) fn classify(line: &str) -> Option<Diagnostic<'_>> {
    let line = line.trim();
    if line.contains(": error: ") || line.starts_with("error: ") {
        Some(Diagnostic::Error(line))
    } else if line.contains(": warning: ") || line.starts_with("warning: ") {
        Some(Diagnostic::Warning(line))
    } else {
        None
    }
}

/// A build as `build_status` reports it.
#[derive(Clone, Debug, Serialize)]
pub struct BuildStatus {
    pub build_id: String,
    /// running, succeeded or failed.
    pub state: &'static str,
    pub project_path: String,
    pub scheme: String,
    pub configuration: String,
    pub destination: String,
    pub errors: Vec<String>,
    pub error_count: usize,
    pub warnings: Vec<String>,
    pub warning_count: usize,
    pub log_path: String,
    pub seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// The built app, to pass to `launch`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,
    /// The end of the log, when the build failed without a recognised error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_tail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip)]
    started: Option<Instant>,
}

impl BuildStatus {
    pub(crate) fn new(
        build_id: String,
        project_path: String,
        scheme: &str,
        configuration: &str,
        destination: &str,
        log_path: String,
    ) -> Self {
        Self {
            build_id,
            state: "running",
            project_path,
            scheme: scheme.to_owned(),
            configuration: configuration.to_owned(),
            destination: destination.to_owned(),
            errors: Vec::new(),
            error_count: 0,
            warnings: Vec::new(),
            warning_count: 0,
            log_path,
            seconds: 0,
            exit_code: None,
            app_path: None,
            bundle_id: None,
            log_tail: None,
            note: None,
            started: Some(Instant::now()),
        }
    }

    /// Counts a line of build output when it is an error or a warning.
    pub(crate) fn note_line(&mut self, line: &str) {
        let (list, count) = match classify(line) {
            Some(Diagnostic::Error(_)) => (&mut self.errors, &mut self.error_count),
            Some(Diagnostic::Warning(_)) => (&mut self.warnings, &mut self.warning_count),
            None => return,
        };
        let text = tail(line.trim(), 500).to_owned();
        if list.contains(&text) {
            return;
        }
        *count += 1;
        if list.len() < KEPT_DIAGNOSTICS {
            list.push(text);
        }
    }
}

struct Builds {
    list: Mutex<HashMap<String, BuildStatus>>,
    changed: Condvar,
    next: Mutex<u32>,
}

fn builds() -> &'static Builds {
    static BUILDS: OnceLock<Builds> = OnceLock::new();
    BUILDS.get_or_init(|| Builds {
        list: Mutex::new(HashMap::new()),
        changed: Condvar::new(),
        next: Mutex::new(0),
    })
}

fn update(id: &str, change: impl FnOnce(&mut BuildStatus)) {
    let builds = builds();
    if let Ok(mut list) = builds.list.lock() {
        if let Some(status) = list.get_mut(id) {
            change(status);
        }
    }
    builds.changed.notify_all();
}

/// Starts building `scheme` for `destination` and returns at once.
pub fn start_build(
    project_path: &str,
    scheme: &str,
    configuration: &str,
    destination: &str,
) -> Result<BuildStatus, String> {
    let container = container(project_path)?;
    if scheme.trim().is_empty() || scheme.contains('\0') {
        return Err("scheme is required. Call schemes to list them.".into());
    }
    let project = container.path.to_string_lossy().into_owned();
    let builds = builds();
    if let Some(running) = builds
        .list
        .lock()
        .map_err(|_| "Builds unavailable")?
        .values()
        .find(|b| b.state == "running" && b.project_path == project)
    {
        return Err(format!(
            "Build {} of this project is still running. Call build_status with its build_id.",
            running.build_id
        ));
    }
    let id = {
        let mut next = builds.next.lock().map_err(|_| "Builds unavailable")?;
        *next += 1;
        format!("build-{}", *next)
    };
    let root = build_root(&container.path);
    let derived = root.join("DerivedData");
    let logs = root.join("logs");
    std::fs::create_dir_all(&logs).map_err(|e| format!("{}: {e}", logs.display()))?;
    let log_path = logs.join(format!("{}-{id}.log", std::process::id()));
    let log = File::create(&log_path).map_err(|e| format!("{}: {e}", log_path.display()))?;
    let selection = selection(&container, scheme, configuration, destination, &derived);
    let mut child = Command::new(XCRUN)
        .arg("xcodebuild")
        .args(&selection)
        .arg("build")
        .current_dir(container.path.parent().unwrap_or(Path::new("/")))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("xcodebuild: {e}"))?;
    let status = BuildStatus::new(
        id.clone(),
        project,
        scheme,
        configuration,
        destination,
        log_path.to_string_lossy().into_owned(),
    );
    builds
        .list
        .lock()
        .map_err(|_| "Builds unavailable")?
        .insert(id.clone(), status.clone());

    let log = Arc::new(Mutex::new(log));
    let readers: Vec<_> = [
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    .map(|pipe| {
        let (log, id) = (log.clone(), id.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).split(b'\n').map_while(Result::ok) {
                let line = String::from_utf8_lossy(&line);
                if let Ok(mut log) = log.lock() {
                    let _ = writeln!(log, "{line}");
                }
                if classify(&line).is_some() {
                    update(&id, |status| status.note_line(&line));
                }
            }
        })
    })
    .collect();
    std::thread::spawn(move || {
        let code = child.wait().ok().and_then(|status| status.code());
        for reader in readers {
            let _ = reader.join();
        }
        let located = (code == Some(0)).then(|| locate(&selection));
        let log_text =
            (code != Some(0)).then(|| std::fs::read_to_string(&log_path).unwrap_or_default());
        update(&id, |status| {
            status.exit_code = code;
            if let Some(started) = status.started.take() {
                status.seconds = started.elapsed().as_secs();
            }
            match located {
                Some(Ok((app, bundle))) => {
                    status.state = "succeeded";
                    status.app_path = Some(app);
                    status.bundle_id = bundle;
                }
                Some(Err(error)) => {
                    status.state = "succeeded";
                    status.note = Some(format!(
                        "Built, but the .app was not found: {error}. Look under the build's DerivedData/Build/Products."
                    ));
                }
                None => {
                    status.state = "failed";
                    if status.error_count == 0 {
                        status.log_tail = log_text.map(|text| tail(&text, 4000).to_owned());
                    }
                }
            }
        });
    });
    Ok(status)
}

/// The built app's path and bundle identifier, from the build settings.
fn locate(selection: &[String]) -> Result<(String, Option<String>), String> {
    let mut args: Vec<&str> = vec!["xcodebuild"];
    args.extend(selection.iter().map(String::as_str));
    args.extend(["-showBuildSettings", "-json"]);
    let json = run(XCRUN, &args, Duration::from_secs(120))?.ok("xcodebuild -showBuildSettings")?;
    let (app, bundle) = find_app(&json).ok_or("no application target in the build settings")?;
    if !Path::new(&app).is_dir() {
        return Err(format!("{app} does not exist"));
    }
    Ok((app, bundle))
}

/// A build's progress, waiting up to `wait` for a running one to finish.
pub fn build_status(id: &str, wait: Duration) -> Result<BuildStatus, String> {
    let builds = builds();
    let list = builds.list.lock().map_err(|_| "Builds unavailable")?;
    if !list.contains_key(id) {
        return Err(format!(
            "No build '{id}' in this session. Builds last as long as the agent's session."
        ));
    }
    let (list, _) = builds
        .changed
        .wait_timeout_while(list, wait, |list| {
            list.get(id).is_some_and(|b| b.state == "running")
        })
        .map_err(|_| "Builds unavailable")?;
    let mut status = list.get(id).cloned().ok_or("The build disappeared")?;
    if let Some(started) = status.started {
        status.seconds = started.elapsed().as_secs();
    }
    Ok(status)
}
