//! Child policy extracted from src-tauri/src/background_work.rs.
use std::process::Command;

/// A command for an agent CLI. On Windows, npm installs CLIs as `.cmd`
/// shims, and Rust refuses to pass arguments with quotes, newlines or `%` to
/// a batch file (the "batch file arguments are invalid" error). Launch what
/// the shim launches instead: its `.exe`, or `node` with its script, as npm's
/// cmd-shim writes them. Anything else starts as given.
pub fn command(binary: impl AsRef<std::path::Path>) -> Command {
    let binary = binary.as_ref();
    match shim_target(binary) {
        Some((program, script)) => {
            let mut command = Command::new(program);
            if let Some(script) = script {
                command.arg(script);
            }
            command
        }
        None => Command::new(binary),
    }
}

/// What an npm `.cmd` shim runs: the program, and for Node CLIs its script.
pub fn shim_target(
    binary: &std::path::Path,
) -> Option<(std::path::PathBuf, Option<std::path::PathBuf>)> {
    let extension = binary.extension()?.to_string_lossy().to_ascii_lowercase();
    if extension != "cmd" && extension != "bat" {
        return None;
    }
    let text = std::fs::read_to_string(binary).ok()?;
    let dir = binary.parent()?;
    let target = shim_paths(&text)
        .into_iter()
        .rfind(|path| !path.eq_ignore_ascii_case("node.exe"))?;
    let target = dir.join(target.replace('\\', std::path::MAIN_SEPARATOR_STR));
    if !target.is_file() {
        return None;
    }
    if target
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        return Some((target, None));
    }
    let local = dir.join("node.exe");
    let node = if local.is_file() {
        local
    } else {
        std::path::PathBuf::from("node")
    };
    Some((node, Some(target)))
}

/// The quoted `%dp0%\…` (or `%~dp0\…`) paths in a cmd-shim, in order.
fn shim_paths(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for quoted in text.split('"').skip(1).step_by(2) {
        let rest = quoted
            .strip_prefix("%dp0%\\")
            .or_else(|| quoted.strip_prefix("%~dp0\\"))
            .or_else(|| quoted.strip_prefix("%~dp0"));
        if let Some(rest) = rest.filter(|rest| !rest.is_empty()) {
            paths.push(rest.to_owned());
        }
    }
    paths
}

/// Apply scheduling hints before exec so descendants inherit them.
/// Best effort: a refused hint must not prevent a provider from launching.
pub fn configure_child(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Only system calls here: the post-fork child must not allocate or lock.
        unsafe {
            command.pre_exec(|| {
                // A GUI executor's worker may block signals. exec preserves
                // that mask; Tokio-based provider children then never receive
                // SIGCHLD and their command sessions never finish. Give the
                // provider the ordinary process mask before it starts.
                let mut signals = std::mem::zeroed();
                libc::sigemptyset(&mut signals);
                if libc::sigprocmask(libc::SIG_SETMASK, &signals, std::ptr::null_mut()) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let priority = libc::getpriority(libc::PRIO_PROCESS, 0).max(10);
                libc::setpriority(libc::PRIO_PROCESS, 0, priority);
                #[cfg(target_os = "macos")]
                setiopolicy_np(0, 0, 3); // DISK, PROCESS, THROTTLE
                Ok(())
            });
        }
    }
    // A GUI-subsystem parent gives every console child (git, gh) its own
    // visible console window unless asked not to.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = command;
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn setiopolicy_np(kind: i32, scope: i32, policy: i32) -> i32;
}

/// New native provider processes use a separate group for cancellation.
pub fn isolate(command: &mut Command) {
    configure_child(command);
    command.env("MONOCODE_HARNESS_PARENT", std::process::id().to_string());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW | 0x0000_0200);
    }
}

/// Called only by the provider worker. Always reap the immediate child.
pub fn terminate(child: &mut std::process::Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    #[cfg(windows)]
    {
        let mut kill = Command::new("taskkill");
        kill.args(["/PID", &child.id().to_string(), "/T", "/F"]);
        use std::os::windows::process::CommandExt;
        kill.creation_flags(CREATE_NO_WINDOW);
        let _ = kill.status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// How long a provider whose stdin was closed may take to exit by itself.
const EXIT_GRACE: std::time::Duration = std::time::Duration::from_millis(1500);
/// How long the group leader may take to shut down after SIGTERM. Windows has
/// no SIGTERM for these children, so there it extends the stdin-EOF wait.
const TERM_GRACE: std::time::Duration = std::time::Duration::from_secs(2);
/// How long quitting waits for providers whose stdin it closed.
const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

/// Stop a provider child without cutting off a credential write, on a
/// background thread. Close its stdin before calling this.
///
/// Claude Code and Codex rotate their OAuth refresh token on every refresh and
/// save the new one (Claude through a `security -i` child on macOS, by
/// truncating and rewriting `.credentials.json` elsewhere). Killing the
/// process in that window loses the new token and leaves a spent one on disk,
/// which signs the account out. Close stdin first, then send SIGTERM to the
/// leader only (Claude shuts down cleanly on it), and SIGKILL the group last.
/// Windows waits for the same total time before `taskkill /T /F`.
pub fn retire(mut child: std::process::Child) {
    drop(child.stdin.take());
    #[cfg(unix)]
    {
        let pid = child.id();
        // std never reaps a dropped Child; `retire_pid` waits for it below.
        drop(child);
        std::thread::spawn(move || retire_pid(pid, || {}));
    }
    #[cfg(windows)]
    {
        let watch = Watch::new(child.id());
        std::thread::spawn(move || {
            retire_watch(watch, || {});
            let _ = child.wait();
        });
    }
}

/// An open handle on a provider child. It outlives the `Child` value, so the
/// wait and the kill never reach another process that reused the pid.
#[cfg(windows)]
pub(crate) struct Watch {
    pid: u32,
    handle: windows_sys::Win32::Foundation::HANDLE,
}
// A process handle may be waited on and closed from any thread.
#[cfg(windows)]
unsafe impl Send for Watch {}
#[cfg(windows)]
impl Watch {
    /// `None` when the process is already gone.
    pub(crate) fn new(pid: u32) -> Option<Self> {
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        (!handle.is_null()).then_some(Self { pid, handle })
    }
    fn exited(&self, within: std::time::Duration) -> bool {
        use windows_sys::Win32::{
            Foundation::WAIT_TIMEOUT, System::Threading::WaitForSingleObject,
        };
        let ms = within.as_millis().min(u32::MAX as u128 - 1) as u32;
        unsafe { WaitForSingleObject(self.handle, ms) != WAIT_TIMEOUT }
    }
}
#[cfg(windows)]
impl Drop for Watch {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.handle) };
    }
}

/// Windows form of [`retire_pid`]: wait for the child to exit on stdin EOF,
/// and kill its tree only if it is still running. Open the [`Watch`] while
/// the `Child` is still held. `after` runs once the process is gone.
#[cfg(windows)]
pub(crate) fn retire_watch(watch: Option<Watch>, after: impl FnOnce()) {
    if let Some(watch) = watch {
        if !watch.exited(EXIT_GRACE + TERM_GRACE) {
            kill_tree(watch.pid);
            watch.exited(std::time::Duration::from_secs(1));
        }
    }
    after();
}

#[cfg(windows)]
fn kill_tree(pid: u32) {
    use std::os::windows::process::CommandExt;
    let mut kill = Command::new("taskkill");
    kill.args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    let _ = kill.status();
}

/// Blocking form of [`retire`] for a child whose `Child` value cannot be
/// moved. `after` runs once the process is gone (to join pipe readers).
#[cfg(unix)]
pub(crate) fn retire_pid(pid: u32, after: impl FnOnce()) {
    let pid = pid as libc::pid_t;
    let exited = |within: std::time::Duration| {
        let deadline = std::time::Instant::now() + within;
        loop {
            let mut status = 0;
            match unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) } {
                0 if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(20))
                }
                0 => return false,
                // Reaped now, or already reaped / not our child.
                _ => return true,
            }
        }
    };
    if !exited(EXIT_GRACE) {
        unsafe { libc::kill(pid, libc::SIGTERM) };
        if !exited(TERM_GRACE) {
            unsafe { libc::kill(-pid, libc::SIGKILL) };
            let mut status = 0;
            unsafe { libc::waitpid(pid, &mut status, 0) };
        }
    }
    // The leader has finished its own writes. Clear any helpers it left behind.
    unsafe { libc::kill(-pid, libc::SIGKILL) };
    after();
}

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
/// A provider child that belongs to a session, for the running-work list.
#[derive(Clone, Debug)]
pub struct Agent {
    pub session: i64,
    pub pid: u32,
    /// Birth stamp at spawn; a reused PID no longer matches it.
    pub identity: Option<String>,
}
fn active() -> &'static Mutex<HashMap<u32, Option<Agent>>> {
    static ACTIVE: OnceLock<Mutex<HashMap<u32, Option<Agent>>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(HashMap::new()))
}
thread_local! {
    static OWNER: Cell<Option<i64>> = const { Cell::new(None) };
}
/// Children spawned by `run` on this thread are attributed to `session`.
pub fn owned_by<T>(session: i64, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<i64>);
    impl Drop for Restore {
        fn drop(&mut self) {
            OWNER.set(self.0);
        }
    }
    let _restore = Restore(OWNER.replace(Some(session)));
    run()
}
/// Live provider children that belong to a session.
pub fn agents() -> Vec<Agent> {
    active()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .flatten()
        .cloned()
        .collect()
}
pub(crate) fn owner() -> Option<i64> {
    OWNER.get()
}
pub(crate) fn mark_idle(pid: u32, idle: bool) {
    let mut processes = active().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(slot) = processes.get_mut(&pid) {
        *slot = if idle {
            None
        } else {
            OWNER.get().map(|session| Agent {
                session,
                pid,
                identity: crate::resources::identity(pid),
            })
        };
    }
}
pub(crate) struct Registration(u32);
pub(crate) fn register(pid: u32) -> Registration {
    let agent = OWNER.get().map(|session| Agent {
        session,
        pid,
        identity: crate::resources::identity(pid),
    });
    active()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(pid, agent);
    Registration(pid)
}
impl Drop for Registration {
    fn drop(&mut self) {
        active()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.0);
        inputs()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.0);
    }
}
type Close = Box<dyn FnOnce() + Send>;
fn inputs() -> &'static Mutex<HashMap<u32, Close>> {
    static INPUTS: OnceLock<Mutex<HashMap<u32, Close>>> = OnceLock::new();
    INPUTS.get_or_init(|| Mutex::new(HashMap::new()))
}
/// How quitting closes a registered child's stdin, so a long-lived provider
/// can exit by itself before anything is killed. Forgotten with the
/// child's [`Registration`].
pub(crate) fn on_shutdown(pid: u32, close: impl FnOnce() + Send + 'static) {
    inputs()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(pid, Box::new(close));
}
fn close_inputs(pids: &[u32]) {
    let closes: Vec<Close> = {
        let mut inputs = inputs().lock().unwrap_or_else(|e| e.into_inner());
        pids.iter().filter_map(|pid| inputs.remove(pid)).collect()
    };
    for close in closes {
        close();
    }
}
/// Explicit native-app quit hook (also quitting to install an update). Run off
/// the UI thread and wait briefly for the workers to reap their children,
/// rather than leaving isolated provider groups.
///
/// Every provider's stdin is closed first, and on Unix its leader gets
/// SIGTERM, so a CLI that is saving a rotated OAuth token (Claude Code, Codex)
/// can finish; only what is still running after [`SHUTDOWN_GRACE`] is killed.
pub fn shutdown_all() {
    let registered = || -> Vec<u32> {
        active()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .copied()
            .collect()
    };
    close_inputs(&registered());
    #[cfg(unix)]
    {
        for pid in registered() {
            unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        }
        let deadline = std::time::Instant::now() + SHUTDOWN_GRACE;
        while !registered().is_empty() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    #[cfg(windows)]
    let watches: Vec<Watch> = {
        // A child that exits stays registered until its worker reaps it, so
        // wait on the processes themselves.
        let watches: Vec<Watch> = registered().into_iter().filter_map(Watch::new).collect();
        let deadline = std::time::Instant::now() + SHUTDOWN_GRACE;
        watches
            .into_iter()
            .filter(|watch| {
                !watch.exited(deadline.saturating_duration_since(std::time::Instant::now()))
            })
            .collect()
    };
    #[cfg(unix)]
    for pid in registered() {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    for watch in &watches {
        kill_tree(watch.pid);
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    while !active()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_empty()
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    fn spawn(script: &str) -> std::process::Child {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]).stdin(Stdio::piped());
        isolate(&mut command);
        command.spawn().unwrap()
    }
    #[test]
    // `retire_pid` reaps the child with waitpid.
    #[allow(clippy::zombie_processes)]
    fn retire_lets_a_child_finish_on_stdin_eof_before_any_signal() {
        let dir = std::env::temp_dir().join(format!("monocode-retire-{}", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        // Exits 0 on EOF after a short write; a signal would skip the write.
        let mut child = spawn(&format!(
            "trap '' TERM; cat >/dev/null; sleep 0.2; echo saved > '{}'",
            dir.display()
        ));
        let pid = child.id();
        drop(child.stdin.take());
        let started = Instant::now();
        retire_pid(pid, || {});
        assert!(started.elapsed() < EXIT_GRACE);
        assert_eq!(std::fs::read_to_string(&dir).unwrap().trim(), "saved");
        let _ = std::fs::remove_file(&dir);
    }
    #[test]
    // `retire_pid` reaps the child with waitpid.
    #[allow(clippy::zombie_processes)]
    fn retire_escalates_when_a_child_ignores_stdin_and_sigterm() {
        let mut child = spawn("trap '' TERM; while :; do sleep 0.05; done");
        let pid = child.id();
        drop(child.stdin.take());
        let started = Instant::now();
        retire_pid(pid, || {});
        assert!(started.elapsed() >= EXIT_GRACE + TERM_GRACE - Duration::from_millis(100));
        // The group is sent SIGKILL as `retire_pid` returns; its other members
        // (the loop's `sleep`) can take a moment to exit and be reaped, longer
        // on a loaded CI runner.
        let deadline = Instant::now() + Duration::from_secs(2);
        while unsafe { libc::kill(-(pid as i32), 0) } == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(unsafe { libc::kill(-(pid as i32), 0) }, -1);
    }
    #[test]
    fn provider_child_does_not_inherit_executor_signal_mask() {
        const CHILD: &str = "MONOCODE_SIGNAL_REGRESSION_CHILD";
        unsafe {
            let mut mask = std::mem::zeroed();
            libc::pthread_sigmask(libc::SIG_SETMASK, std::ptr::null(), &mut mask);
            if std::env::var_os(CHILD).is_some() {
                assert_eq!(libc::sigismember(&mask, libc::SIGCHLD), 0);
                return;
            }
            let mut blocked = std::mem::zeroed();
            libc::sigemptyset(&mut blocked);
            libc::sigaddset(&mut blocked, libc::SIGCHLD);
            assert_eq!(
                libc::pthread_sigmask(libc::SIG_BLOCK, &blocked, std::ptr::null_mut()),
                0
            );
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "process::tests::provider_child_does_not_inherit_executor_signal_mask",
                    "--nocapture",
                ])
                .env(CHILD, "1");
            isolate(&mut command);
            let result = command.output();
            libc::pthread_sigmask(libc::SIG_SETMASK, &mask, std::ptr::null_mut());
            let output = result.unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn background_children_keep_piped_output_and_exit_status() {
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "(echo stdout)&(echo stderr 1>&2)&exit /b 7"]);
        configure_child(&mut command);

        let output = command.output().expect("background command should run");
        assert_eq!(output.status.code(), Some(7));
        assert!(String::from_utf8_lossy(&output.stdout).contains("stdout"));
        assert!(String::from_utf8_lossy(&output.stderr).contains("stderr"));
    }

    fn spawn(script: &str) -> std::process::Child {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new("cmd.exe");
        // cmd does not parse the backslash-escaped quotes `args` would write.
        command
            .args(["/D", "/C"])
            .raw_arg(script)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        isolate(&mut command);
        command.spawn().unwrap()
    }

    fn saved_file(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("monocode-{name}-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn retire_lets_a_child_finish_on_stdin_eof_before_any_kill() {
        let path = saved_file("retire");
        // `sort` reads until EOF; a kill would skip the write after it.
        let mut child = spawn(&format!("sort >NUL & echo saved>\"{}\"", path.display()));
        let watch = Watch::new(child.id());
        drop(child.stdin.take());
        let started = std::time::Instant::now();
        retire_watch(watch, || {});
        assert!(started.elapsed() < EXIT_GRACE + TERM_GRACE);
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), "saved");
        let _ = child.wait();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn retire_kills_a_child_that_ignores_stdin_after_the_grace() {
        let mut child = spawn("ping -n 30 127.0.0.1 >NUL");
        let watch = Watch::new(child.id());
        drop(child.stdin.take());
        let started = std::time::Instant::now();
        retire_watch(watch, || {});
        assert!(
            started.elapsed() >= EXIT_GRACE + TERM_GRACE - std::time::Duration::from_millis(100)
        );
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    fn quitting_closes_a_registered_childs_stdin_so_it_exits_by_itself() {
        let path = saved_file("shutdown");
        let mut child = spawn(&format!("sort >NUL & echo saved>\"{}\"", path.display()));
        let pid = child.id();
        let _registration = register(pid);
        let pipe = child.stdin.take();
        on_shutdown(pid, move || drop(pipe));
        close_inputs(&[pid]);
        assert!(Watch::new(pid).unwrap().exited(SHUTDOWN_GRACE));
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), "saved");
        let _ = child.wait();
        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod input_tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    #[test]
    fn a_shutdown_closer_runs_once_and_is_forgotten_with_its_registration() {
        // Pids no real child here can have.
        let (kept, gone) = (u32::MAX - 7, u32::MAX - 8);
        let ran = Arc::new(AtomicBool::new(false));
        let _kept = register(kept);
        let flag = ran.clone();
        on_shutdown(kept, move || flag.store(true, Ordering::SeqCst));
        let stale = Arc::new(AtomicBool::new(false));
        let registration = register(gone);
        let flag = stale.clone();
        on_shutdown(gone, move || flag.store(true, Ordering::SeqCst));
        drop(registration);

        close_inputs(&[kept, gone]);
        assert!(ran.load(Ordering::SeqCst));
        assert!(!stale.load(Ordering::SeqCst));
        assert!(!inputs().lock().unwrap().contains_key(&kept));
    }
}

#[cfg(test)]
mod shim_tests {
    use super::*;

    #[test]
    fn npm_shims_launch_their_exe_or_node_script() {
        let dir = std::env::temp_dir().join(format!("monobot-shim-{}", std::process::id()));
        let bin = dir.join("node_modules").join("cli").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("claude.exe"), b"").unwrap();
        std::fs::write(bin.join("codex.js"), b"").unwrap();
        // The two shapes npm's cmd-shim writes: a native exe, and a Node script.
        let exe = dir.join("claude.cmd");
        std::fs::write(
            &exe,
            "@ECHO off\r\nCALL :find_dp0\r\n\"%dp0%\\node_modules\\cli\\bin\\claude.exe\"   %*\r\n",
        )
        .unwrap();
        let node = dir.join("codex.cmd");
        std::fs::write(
            &node,
            concat!(
                "IF EXIST \"%dp0%\\node.exe\" (\r\n",
                "  SET \"_prog=%dp0%\\node.exe\"\r\n",
                ")\r\n",
                "\"%_prog%\"  \"%dp0%\\node_modules\\cli\\bin\\codex.js\" %*\r\n",
            ),
        )
        .unwrap();
        let (program, script) = shim_target(&exe).unwrap();
        assert!(program.ends_with("claude.exe") && script.is_none());
        let (program, script) = shim_target(&node).unwrap();
        assert_eq!(program, std::path::PathBuf::from("node"));
        assert!(script.unwrap().ends_with("codex.js"));
        assert!(shim_target(&dir.join("claude.exe")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod live_tests {
    /// Every provider installed on this machine is found and starts.
    /// `cargo test -p monocode-core installed_providers_start -- --ignored --nocapture`
    #[test]
    #[ignore = "starts the installed agent CLIs"]
    fn installed_providers_start() {
        for kind in crate::provider::Kind::ALL {
            let Some(binary) = crate::provider::resolve_auto(kind) else {
                println!("{:<12} not installed", kind.id());
                continue;
            };
            let output = super::command(&binary)
                .arg("--version")
                .stdin(std::process::Stdio::null())
                .output();
            let line = output
                .as_ref()
                .map(|o| {
                    let text = String::from_utf8_lossy(&o.stdout).trim().to_owned();
                    let text = if text.is_empty() {
                        String::from_utf8_lossy(&o.stderr).trim().to_owned()
                    } else {
                        text
                    };
                    text.lines().next().unwrap_or_default().to_owned()
                })
                .unwrap_or_else(|error| format!("FAILED: {error}"));
            println!("{:<12} {} -> {line}", kind.id(), binary.display());
            assert!(output.is_ok(), "{} did not start", kind.id());
        }
    }
}
