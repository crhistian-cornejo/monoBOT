//! `cargo xtask dev`: builds the app with the fast `native-dev` profile, runs
//! it on separate development data, and rebuilds and restarts it when you type
//! `r` then Enter. File saves never trigger builds. On macOS it runs from a
//! `.app` bundle so Finder, the menu bar and the microphone behave as installed.
use crate::{
    bundle::{bundle_macos, Bundle, DataMode, BUNDLE_NAME},
    util::{self, metadata_args, parse_metadata, target_dir, Platform, Result, APP_MANIFEST},
};
use std::{
    ffi::OsString,
    io::{BufRead, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex, MutexGuard,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// One process the runner starts: cargo or the app.
#[derive(Clone, Debug)]
pub struct Spawn {
    pub program: OsString,
    pub args: Vec<String>,
    /// Added to the inherited environment.
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
    pub capture_stdout: bool,
    pub pipe_stdin: bool,
    /// Its own process group, so Ctrl+C reaches the runner, which then lets
    /// the app run its quit hooks.
    pub detached: bool,
}

/// What the runner does to the outside world. Tests replace it with fakes.
pub trait Backend: Send + Sync {
    fn spawn(&self, spawn: &Spawn) -> std::io::Result<Child>;
    fn bundle(&self, profile_dir: &Path, version: &str, data_mode: DataMode) -> Result<()>;
}

pub struct SystemBackend {
    pub root: PathBuf,
}

impl Backend for SystemBackend {
    fn spawn(&self, spawn: &Spawn) -> std::io::Result<Child> {
        let mut command = util::command(&spawn.program);
        command
            .args(&spawn.args)
            .envs(spawn.env.iter().map(|(k, v)| (k, v)))
            .current_dir(&spawn.cwd);
        if spawn.pipe_stdin {
            command.stdin(Stdio::piped());
        }
        if spawn.capture_stdout {
            command.stdin(Stdio::null()).stdout(Stdio::piped());
        }
        #[cfg(unix)]
        if spawn.detached {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        command.spawn()
    }

    fn bundle(&self, profile_dir: &Path, version: &str, data_mode: DataMode) -> Result<()> {
        let identity = std::env::var("MONOCODE_NATIVE_SIGNING_IDENTITY").ok();
        bundle_macos(
            &Bundle {
                root: &self.root,
                profile_dir,
                version,
                data_mode,
                identity: identity.as_deref(),
            },
            &util::System,
        )
        .map(|_| ())
    }
}

pub type Log = Arc<dyn Fn(&str) + Send + Sync>;

pub struct DevOptions {
    pub root: PathBuf,
    /// Passed to the app as they are.
    pub args: Vec<String>,
    pub platform: Platform,
    /// Added to every process's environment (sherpa libraries, job count).
    pub env: Vec<(String, String)>,
    /// `CARGO_BUILD_TARGET`, when set.
    pub build_target: Option<String>,
    pub debounce: Duration,
    pub log: Log,
    pub backend: Arc<dyn Backend>,
}

/// The app forwards these flags and keeps the value that follows them.
const VALUE_FLAGS: [&str; 2] = ["--cwd", "--monocode-db"];

/// The app arguments, minus `--` and the retired runner flags.
pub fn parse_dev_args(argv: &[String]) -> Result<Vec<String>> {
    let mut args = Vec::new();
    let mut index = 0;
    while index < argv.len() {
        let arg = &argv[index];
        if arg == "--" {
            args.extend(argv[index + 1..].iter().cloned());
            break;
        }
        if arg == "--watch" || arg == "--bundle" {
            return Err(format!(
                "{arg} is not supported. Use cargo xtask dev, then type r and Enter to rebuild and restart."
            ));
        }
        args.push(arg.clone());
        // Keep values even when an app path happens to match a runner flag.
        if VALUE_FLAGS.contains(&arg.as_str()) && index + 1 < argv.len() {
            index += 1;
            args.push(argv[index].clone());
        }
        index += 1;
    }
    Ok(args)
}

/// The data a Finder launch of the dev bundle uses: the last data flag wins.
pub fn data_mode(args: &[String]) -> DataMode {
    let mut mode = DataMode::DevData;
    for (index, arg) in args.iter().enumerate() {
        if index > 0 && VALUE_FLAGS.contains(&args[index - 1].as_str()) {
            continue;
        }
        match arg.as_str() {
            "--dev-data" => mode = DataMode::DevData,
            "--monocode-data" => mode = DataMode::MonocodeData,
            _ => {}
        }
    }
    mode
}

struct AppProcess {
    child: Mutex<Child>,
}

#[derive(Default)]
struct State {
    revision: u64,
    /// The revision the last build started from.
    attempted: u64,
    deadline: Option<Instant>,
    closing: bool,
    failed: bool,
    finished: bool,
    /// The running cargo command.
    command: Option<Child>,
    app: Option<Arc<AppProcess>>,
}

struct Inner {
    options: DevOptions,
    state: Mutex<State>,
    wake: Condvar,
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn log(&self, message: &str) {
        (self.options.log)(message)
    }
    fn closing(&self) -> bool {
        self.lock().closing
    }
    /// Another restart request arrived after `built` started.
    fn stale(&self, built: u64) -> bool {
        self.lock().revision != built
    }
}

pub struct Dev {
    inner: Arc<Inner>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

fn terminate(child: &mut Child) {
    #[cfg(unix)]
    // SAFETY: kill(2) on our own child's pid; it is not reaped yet because
    // the runner holds the Child.
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    #[cfg(not(unix))]
    let _ = child.kill();
}

fn describe(status: &ExitStatus) -> String {
    if let Some(code) = status.code() {
        return code.to_string();
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return format!("signal {signal}");
        }
    }
    "unknown".into()
}

/// Asks the app to quit through its opt-in dev hook (persistence and provider
/// cleanup run as usual), then ends it after 10 seconds.
fn quit(inner: &Inner, app: &AppProcess) {
    if let Some(mut stdin) = app.child.lock().unwrap().stdin.take() {
        // An app that exits while a restart is requested may close its pipe first.
        let _ = stdin.write_all(b"quit\n");
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        {
            let mut child = app.child.lock().unwrap();
            if !matches!(child.try_wait(), Ok(None)) {
                return;
            }
            if Instant::now() >= deadline {
                inner.log("App did not finish shutting down within 10s; terminating it.");
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn stop_app(inner: &Inner) {
    let app = inner.lock().app.take();
    if let Some(app) = app {
        quit(inner, &app);
    }
}

/// Runs cargo; returns whether it succeeded and, if captured, its stdout.
fn run_cargo(inner: &Inner, args: &[&str], capture: bool) -> (bool, String) {
    let options = &inner.options;
    let spawn = Spawn {
        program: "cargo".into(),
        args: args.iter().map(|a| a.to_string()).collect(),
        env: options.env.clone(),
        cwd: options.root.clone(),
        capture_stdout: capture,
        pipe_stdin: false,
        detached: false,
    };
    let mut child = match options.backend.spawn(&spawn) {
        Ok(child) => child,
        Err(error) => {
            inner.log(&format!("cargo: {error}"));
            return (false, String::new());
        }
    };
    let reader = child.stdout.take().map(|mut stdout| {
        thread::spawn(move || {
            let mut text = String::new();
            let _ = stdout.read_to_string(&mut text);
            text
        })
    });
    {
        let mut state = inner.lock();
        if state.closing {
            terminate(&mut child);
        }
        state.command = Some(child);
    }
    let status = loop {
        {
            let mut state = inner.lock();
            let Some(command) = state.command.as_mut() else {
                break None;
            };
            match command.try_wait() {
                Ok(None) => {}
                Ok(Some(status)) => {
                    state.command = None;
                    break Some(status);
                }
                Err(_) => {
                    state.command = None;
                    break None;
                }
            }
        }
        thread::sleep(Duration::from_millis(20));
    };
    let stdout = reader
        .map(|r| r.join().unwrap_or_default())
        .unwrap_or_default();
    (status.is_some_and(|s| s.success()), stdout)
}

fn watch_app(inner: Arc<Inner>, app: Arc<AppProcess>) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(50));
        let exited = match app.child.lock().unwrap().try_wait() {
            Ok(None) => continue,
            Ok(Some(status)) => describe(&status),
            Err(error) => error.to_string(),
        };
        let mut state = inner.lock();
        if state.app.as_ref().is_some_and(|a| Arc::ptr_eq(a, &app)) {
            state.app = None;
            drop(state);
            inner.log(&format!(
                "App exited ({exited}). Type r to rebuild and relaunch."
            ));
        }
        return;
    });
}

fn rebuild(inner: &Arc<Inner>) -> Result<()> {
    let options = &inner.options;
    loop {
        let built = {
            let mut state = inner.lock();
            if state.closing {
                return Ok(());
            }
            state.attempted = state.revision;
            state.revision
        };
        // A failed step retries only when a restart was requested meanwhile.
        let again = || !inner.closing() && inner.stale(built);
        let started = Instant::now();
        inner.log("Building… (the current app stays open)");
        let (ok, stdout) = run_cargo(inner, &metadata_args(), true);
        if inner.closing() {
            return Ok(());
        }
        if !ok {
            inner.log("Cargo metadata failed. Fix the error, then type r to retry.");
            if again() {
                continue;
            }
            return Ok(());
        }
        let metadata = parse_metadata(&stdout)?;
        let target = target_dir(&metadata, options.build_target.as_deref());
        let build = [
            "build",
            "--manifest-path",
            APP_MANIFEST,
            "--locked",
            "-p",
            util::APP_PACKAGE,
            "--bin",
            util::APP_PACKAGE,
            "--profile",
            "native-dev",
        ];
        let (ok, _) = run_cargo(inner, &build, false);
        if inner.closing() {
            return Ok(());
        }
        if !ok {
            inner.log(
                "Build failed. Keeping the current app; type r to retry after fixing the error.",
            );
            if again() {
                continue;
            }
            return Ok(());
        }
        // Another restart request during compilation gets a fresh build first.
        if inner.stale(built) {
            continue;
        }
        let profile_dir = target.join("native-dev");
        let mut executable = profile_dir.join(options.platform.executable());
        if options.platform == Platform::Mac {
            let packaged =
                options
                    .backend
                    .bundle(&profile_dir, &metadata.version, data_mode(&options.args));
            if inner.closing() {
                return Ok(());
            }
            if let Err(error) = packaged {
                inner.log(&error);
                inner.log("App packaging failed. Fix the error, then type r to retry.");
                if again() {
                    continue;
                }
                return Ok(());
            }
            executable = profile_dir
                .join(BUNDLE_NAME)
                .join("Contents/MacOS/monocode-gpui");
        }
        if inner.stale(built) {
            continue;
        }
        stop_app(inner);
        if inner.closing() {
            return Ok(());
        }
        if inner.stale(built) {
            continue;
        }
        let mut env = options.env.clone();
        env.push(("MONOCODE_DEV_RELOAD".into(), "1".into()));
        let spawn = Spawn {
            program: executable.into_os_string(),
            args: options.args.clone(),
            env,
            cwd: options.root.clone(),
            capture_stdout: false,
            pipe_stdin: true,
            detached: options.platform != Platform::Windows,
        };
        match options.backend.spawn(&spawn) {
            Err(error) => inner.log(&format!("Could not start the app: {error}")),
            Ok(child) => {
                let app = Arc::new(AppProcess {
                    child: Mutex::new(child),
                });
                let mut state = inner.lock();
                if state.closing {
                    drop(state);
                    quit(inner, &app);
                    return Ok(());
                }
                state.app = Some(app.clone());
                drop(state);
                watch_app(inner.clone(), app);
                inner.log(&format!(
                    "Ready in {:.1}s. Type r then Enter to rebuild and restart.",
                    started.elapsed().as_secs_f64()
                ));
            }
        }
        if !again() {
            return Ok(());
        }
    }
}

/// Waits for a restart request; false once the runner closes.
fn wait_for_request(inner: &Inner) -> bool {
    let mut state = inner.lock();
    loop {
        if state.closing {
            return false;
        }
        let now = Instant::now();
        state = match state.deadline {
            Some(deadline) if now >= deadline => {
                state.deadline = None;
                if state.revision != state.attempted {
                    return true;
                }
                state
            }
            Some(deadline) => {
                inner
                    .wake
                    .wait_timeout(state, deadline - now)
                    .unwrap_or_else(|e| e.into_inner())
                    .0
            }
            None => inner.wake.wait(state).unwrap_or_else(|e| e.into_inner()),
        };
    }
}

impl Dev {
    pub fn start(options: DevOptions) -> Dev {
        let inner = Arc::new(Inner {
            options,
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
        });
        inner.log("Type r then Enter to rebuild and restart; Ctrl+C to stop. File saves do not trigger builds.");
        let worker = {
            let inner = inner.clone();
            thread::spawn(move || {
                let mut first = true;
                loop {
                    if !first {
                        if !wait_for_request(&inner) {
                            break;
                        }
                        inner.log("Rebuilding and restarting…");
                    }
                    first = false;
                    if let Err(error) = rebuild(&inner) {
                        inner.log(&error);
                        {
                            let mut state = inner.lock();
                            state.failed = true;
                            state.closing = true;
                        }
                        stop_app(&inner);
                        break;
                    }
                }
                inner.lock().finished = true;
            })
        };
        Dev {
            inner,
            worker: Mutex::new(Some(worker)),
        }
    }

    pub fn request_build(&self) {
        let mut state = self.inner.lock();
        if state.closing {
            return;
        }
        state.revision += 1;
        state.deadline = Some(Instant::now() + self.inner.options.debounce);
        self.inner.wake.notify_all();
    }

    /// Stops cargo (only the one this runner started), quits the app and
    /// waits for the runner to finish.
    pub fn close(&self) {
        {
            let mut state = self.inner.lock();
            state.closing = true;
            state.deadline = None;
            if let Some(command) = state.command.as_mut() {
                terminate(command);
            }
            self.inner.wake.notify_all();
        }
        stop_app(&self.inner);
        if let Some(worker) = self.worker.lock().unwrap().take() {
            let _ = worker.join();
        }
    }

    pub fn finished(&self) -> bool {
        self.inner.lock().finished
    }

    pub fn failed(&self) -> bool {
        self.inner.lock().failed
    }
}

static STOP: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

pub fn main(argv: &[String]) -> Result<()> {
    let args = parse_dev_args(argv)?;
    let root = util::root();
    let mut env = crate::sherpa::cargo_env(&root);
    if std::env::var("CARGO_BUILD_JOBS").map_or(true, |jobs| jobs.is_empty()) {
        env.push(("CARGO_BUILD_JOBS".into(), "1".into()));
    }
    let dev = Arc::new(Dev::start(DevOptions {
        root: root.clone(),
        args,
        platform: Platform::current(),
        env,
        build_target: std::env::var("CARGO_BUILD_TARGET").ok(),
        debounce: Duration::from_millis(300),
        log: Arc::new(|message| println!("[dev] {message}")),
        backend: Arc::new(SystemBackend { root }),
    }));
    #[cfg(unix)]
    // SAFETY: the handler only stores to an atomic, which is signal-safe.
    unsafe {
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
    }
    {
        let dev = dev.clone();
        thread::spawn(move || {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if line.trim().eq_ignore_ascii_case("r") {
                    dev.request_build();
                }
            }
        });
    }
    while !STOP.load(Ordering::SeqCst) && !dev.finished() {
        thread::sleep(Duration::from_millis(100));
    }
    dev.close();
    if dev.failed() {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn forwards_app_options_and_rejects_the_retired_runner_flags() {
        assert_eq!(parse_dev_args(&[]).unwrap(), Vec::<String>::new());
        let args = strings(&["--monocode-data", "--cwd", "a project"]);
        assert_eq!(parse_dev_args(&args).unwrap(), args);
        let args = strings(&[
            "--dev-data",
            "--cwd",
            "--watch",
            "--monocode-db",
            "--bundle",
        ]);
        assert_eq!(parse_dev_args(&args).unwrap(), args);
        assert_eq!(
            parse_dev_args(&strings(&["--", "--cwd", "a project"])).unwrap(),
            strings(&["--cwd", "a project"])
        );
        for arg in ["--watch", "--bundle"] {
            let error = parse_dev_args(&strings(&[arg])).unwrap_err();
            assert!(error.contains("not supported"), "{error}");
            assert!(error.contains("rebuild and restart"), "{error}");
        }
    }

    #[test]
    fn the_last_data_flag_picks_the_bundle_data() {
        assert_eq!(data_mode(&[]), DataMode::DevData);
        assert_eq!(
            data_mode(&strings(&["--monocode-data"])),
            DataMode::MonocodeData
        );
        assert_eq!(
            data_mode(&strings(&[
                "--monocode-data",
                "--dev-data",
                "--cwd",
                "--monocode-data"
            ])),
            DataMode::DevData
        );
    }

    #[cfg(unix)]
    mod runner {
        use super::*;
        use std::fs;

        #[derive(Clone, Debug)]
        struct BundleCall {
            profile_dir: PathBuf,
            version: String,
            data_mode: DataMode,
        }

        /// Runs a shell fake for cargo and the app; records every call.
        struct Fake {
            root: PathBuf,
            helper: PathBuf,
            calls: Mutex<Vec<Spawn>>,
            bundles: Mutex<Vec<BundleCall>>,
        }

        impl Fake {
            fn event(&self, name: &str) {
                let mut file = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(self.root.join("events"))
                    .unwrap();
                writeln!(file, "{name}").unwrap();
            }
        }

        impl Backend for Fake {
            fn spawn(&self, spawn: &Spawn) -> std::io::Result<Child> {
                self.calls.lock().unwrap().push(spawn.clone());
                let mut command = std::process::Command::new("sh");
                command.arg(&self.helper);
                if spawn.program == "cargo" {
                    command.arg(&spawn.args[0]);
                } else {
                    command.arg("app").args(&spawn.args);
                }
                command
                    .envs(spawn.env.iter().map(|(k, v)| (k, v)))
                    .current_dir(&spawn.cwd)
                    .stdin(if spawn.pipe_stdin {
                        Stdio::piped()
                    } else {
                        Stdio::null()
                    })
                    .stdout(if spawn.capture_stdout {
                        Stdio::piped()
                    } else {
                        Stdio::null()
                    });
                command.spawn()
            }

            fn bundle(&self, profile_dir: &Path, version: &str, data_mode: DataMode) -> Result<()> {
                self.bundles.lock().unwrap().push(BundleCall {
                    profile_dir: profile_dir.into(),
                    version: version.into(),
                    data_mode,
                });
                self.event("package");
                if self.root.join("fail-package").exists() {
                    self.event("package-failed");
                    return Err("packaging failed".into());
                }
                Ok(())
            }
        }

        struct Fixture {
            _dir: tempfile::TempDir,
            root: PathBuf,
            dev: Dev,
            fake: Arc<Fake>,
            messages: Arc<Mutex<Vec<String>>>,
        }

        impl Drop for Fixture {
            fn drop(&mut self) {
                self.dev.close();
            }
        }

        impl Fixture {
            fn events(&self) -> Vec<String> {
                fs::read_to_string(self.root.join("events"))
                    .unwrap_or_default()
                    .lines()
                    .map(str::to_owned)
                    .collect()
            }
            fn count(&self, name: &str) -> usize {
                self.events().iter().filter(|e| *e == name).count()
            }
            fn starts(&self) -> usize {
                self.events()
                    .iter()
                    .filter(|e| e.starts_with("start:"))
                    .count()
            }
            fn until(&self, what: &str, done: impl Fn(&Fixture) -> bool) {
                let deadline = Instant::now() + Duration::from_secs(10);
                while Instant::now() < deadline {
                    if done(self) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                panic!(
                    "Timed out waiting for {what}. Events: {:?}. Logs: {:?}",
                    self.events(),
                    self.messages.lock().unwrap()
                );
            }
            fn launches(&self) -> Vec<Spawn> {
                let calls = self.fake.calls.lock().unwrap();
                calls
                    .iter()
                    .filter(|c| c.program != "cargo")
                    .cloned()
                    .collect()
            }
        }

        fn fixture(env: &[(&str, &str)], platform: Platform, args: &[&str]) -> Fixture {
            let dir = tempfile::Builder::new()
                .prefix("monocode-dev-test-")
                .tempdir()
                .unwrap();
            let root = dir.path().canonicalize().unwrap();
            fs::create_dir_all(root.join("crates/app/src")).unwrap();
            fs::write(root.join("crates/app/src/main.rs"), "initial").unwrap();
            let helper = root.join("fake-process.sh");
            let quoted = root.display().to_string().replace('\'', r"'\''");
            fs::write(
                &helper,
                format!(
                    r#"root='{quoted}'
event() {{ printf '%s\n' "$1" >> "$root/events"; }}
case "$1" in
  metadata)
    printf '{{"target_directory":"%s/custom-target","packages":[{{"name":"monocode-gpui","version":"0.15.0"}}]}}\n' "$root" ;;
  build)
    event build
    sleep "${{TEST_BUILD_DELAY:-0.03}}"
    if [ -e "$root/fail-build" ]; then event failed; exit 1; fi
    event built ;;
  app)
    shift
    event "start:$*"
    if read -r line; then event quit; fi ;;
esac
"#
                ),
            )
            .unwrap();
            let fake = Arc::new(Fake {
                root: root.clone(),
                helper,
                calls: Mutex::default(),
                bundles: Mutex::default(),
            });
            let messages = Arc::new(Mutex::new(Vec::new()));
            let log = {
                let messages = messages.clone();
                Arc::new(move |m: &str| messages.lock().unwrap().push(m.to_owned()))
            };
            let build_target = env
                .iter()
                .find(|(k, _)| *k == "CARGO_BUILD_TARGET")
                .map(|(_, v)| v.to_string());
            let dev = Dev::start(DevOptions {
                root: root.clone(),
                args: strings(args),
                platform,
                env: env
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                build_target,
                debounce: Duration::from_millis(40),
                log,
                backend: fake.clone(),
            });
            Fixture {
                _dir: dir,
                root,
                dev,
                fake,
                messages,
            }
        }

        const PROJECT: [&str; 3] = ["--dev-data", "--cwd", "a project with spaces"];

        #[test]
        fn file_saves_never_rebuild_including_edits_during_a_requested_build() {
            let f = fixture(&[("TEST_BUILD_DELAY", "0.25")], Platform::Linux, &PROJECT);
            f.until("the first launch", |f| f.starts() == 1);
            let source = f.root.join("crates/app/src/main.rs");
            for edit in 0..3 {
                fs::write(&source, format!("edit {edit}")).unwrap();
                thread::sleep(Duration::from_millis(75));
            }
            thread::sleep(Duration::from_millis(600));
            assert_eq!(f.count("build"), 1);

            f.dev.request_build();
            f.until("the second build", |f| f.count("build") == 2);
            fs::write(&source, "edit during manual build").unwrap();
            f.until("the second launch", |f| f.starts() == 2);
            thread::sleep(Duration::from_millis(600));
            assert_eq!(f.count("build"), 2);
            assert_eq!(f.count("quit"), 1);
        }

        #[test]
        fn manual_restart_keeps_the_app_on_a_failed_build_and_shuts_it_down_after_a_successful_retry(
        ) {
            let f = fixture(&[], Platform::Linux, &PROJECT);
            f.until("the first launch", |f| f.starts() == 1);
            let build = f
                .fake
                .calls
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.args[0] == "build")
                .unwrap()
                .args
                .clone();
            assert_eq!(
                build,
                strings(&[
                    "build",
                    "--manifest-path",
                    "crates/monocode-gpui/Cargo.toml",
                    "--locked",
                    "-p",
                    "monocode-gpui",
                    "--bin",
                    "monocode-gpui",
                    "--profile",
                    "native-dev"
                ])
            );
            let launch = f.launches()[0].clone();
            assert!(Path::new(&launch.program).starts_with(f.root.join("custom-target")));
            assert!(launch
                .env
                .contains(&("MONOCODE_DEV_RELOAD".into(), "1".into())));
            assert_eq!(launch.args, strings(&PROJECT));
            assert!(launch.detached);

            fs::write(f.root.join("fail-build"), "").unwrap();
            f.dev.request_build();
            f.until("the failed build", |f| {
                f.events().contains(&"failed".into())
            });
            assert_eq!(f.starts(), 1);
            assert_eq!(f.count("quit"), 0);

            fs::remove_file(f.root.join("fail-build")).unwrap();
            f.dev.request_build();
            f.until("the relaunch", |f| f.starts() == 2);
            let events = f.events();
            let last_built = events.iter().rposition(|e| e == "built").unwrap();
            let quit = events.iter().position(|e| e == "quit").unwrap();
            let last_start = events
                .iter()
                .rposition(|e| e.starts_with("start:"))
                .unwrap();
            assert!(last_built < quit, "{events:?}");
            assert!(quit < last_start, "{events:?}");
            f.dev.close();
            assert_eq!(f.count("quit"), 2);
        }

        #[test]
        fn coalesces_restart_requests_during_a_build_without_launching_stale_code_or_overlapping_cargo(
        ) {
            let f = fixture(&[("TEST_BUILD_DELAY", "0.9")], Platform::Linux, &PROJECT);
            f.until("the first launch", |f| f.starts() == 1);
            f.dev.request_build();
            f.until("the second build", |f| f.count("build") == 2);
            f.dev.request_build();
            thread::sleep(Duration::from_millis(100));
            f.dev.request_build();
            f.until("the second launch", |f| f.starts() == 2);
            let events = f.events();
            assert_eq!(f.count("build"), 3, "{events:?}");
            assert_eq!(f.count("quit"), 1);
            let quit = events.iter().position(|e| e == "quit").unwrap();
            assert!(quit > events.iter().rposition(|e| e == "built").unwrap());
        }

        #[test]
        fn macos_bundles_automatically_and_preserves_the_selected_data_mode_for_finder() {
            for (args, mode) in [
                (vec![], DataMode::DevData),
                (vec!["--monocode-data"], DataMode::MonocodeData),
                (
                    vec!["--monocode-data", "--dev-data", "--cwd", "--monocode-data"],
                    DataMode::DevData,
                ),
            ] {
                let f = fixture(&[], Platform::Mac, &args);
                f.until("the first launch", |f| f.starts() == 1);
                let bundle = f.fake.bundles.lock().unwrap()[0].clone();
                assert_eq!(bundle.profile_dir, f.root.join("custom-target/native-dev"));
                assert_eq!(bundle.version, "0.15.0");
                assert_eq!(bundle.data_mode, mode, "{args:?}");
                let app = f.launches()[0].clone();
                assert_eq!(
                    PathBuf::from(&app.program),
                    f.root
                        .join("custom-target/native-dev/monoBOT.app/Contents/MacOS/monocode-gpui")
                );
                assert_eq!(app.args, strings(&args));
                let events = f.events();
                let index = |name: &str| events.iter().position(|e| e.starts_with(name)).unwrap();
                assert!(index("built") < index("package"));
                assert!(index("package") < index("start:"));
            }
        }

        #[test]
        fn failed_macos_packaging_keeps_the_current_app_running() {
            let f = fixture(&[], Platform::Mac, &[]);
            f.until("the first launch", |f| f.starts() == 1);
            fs::write(f.root.join("fail-package"), "").unwrap();
            f.dev.request_build();
            f.until("the failed packaging", |f| {
                f.events().contains(&"package-failed".into())
            });
            assert_eq!(f.starts(), 1);
            assert_eq!(f.count("quit"), 0);
        }

        #[test]
        fn windows_launches_the_executable_from_the_configured_build_target() {
            let f = fixture(
                &[("CARGO_BUILD_TARGET", "x86_64-pc-windows-msvc")],
                Platform::Windows,
                &[],
            );
            f.until("the first launch", |f| f.starts() == 1);
            assert!(f.fake.bundles.lock().unwrap().is_empty());
            let app = f.launches()[0].clone();
            assert_eq!(
                PathBuf::from(&app.program),
                f.root
                    .join("custom-target/x86_64-pc-windows-msvc/native-dev/monocode-gpui.exe")
            );
            assert!(!app.detached);
        }

        #[test]
        fn closing_during_compilation_terminates_cargo_and_never_launches_an_app() {
            let f = fixture(&[("TEST_BUILD_DELAY", "5")], Platform::Linux, &[]);
            f.until("the build", |f| f.events().contains(&"build".into()));
            let started = Instant::now();
            f.dev.close();
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "cargo was stopped"
            );
            assert_eq!(f.starts(), 0);
            assert!(f.launches().is_empty());
        }
    }
}
