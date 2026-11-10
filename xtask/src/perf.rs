//! `cargo xtask perf`: streaming profiles with a replayed Claude reply
//! (scripts/perf/fake-claude-stream.py). See scripts/perf/README.md.
use crate::util::{self, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

const USAGE: &str =
    "usage: cargo xtask perf <frame-run|stream-profile> [path/to/monocode-gpui] [seconds]";
const INSTALLED: &str = "/Applications/monoBOT.app/Contents/MacOS/monocode-gpui";

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_owned())
}

fn profiler() -> PathBuf {
    std::env::var_os("MONOCODE_PROFILER")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env_or("HOME", "~")).join(
                "code/mono-perf-chip/dist/Monocode Performance.app/Contents/MacOS/MonocodePerformance",
            )
        })
}

fn executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    path.is_file()
}

/// Closes the window this command started, however it ends.
struct AppGuard(Child);

impl Drop for AppGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl AppGuard {
    fn alive(&mut self) -> bool {
        matches!(self.0.try_wait(), Ok(None))
    }
}

struct Run {
    work: PathBuf,
    app: AppGuard,
}

/// A throwaway folder for one run; kept afterwards for its logs.
fn new_work(binary: &Path) -> Result<PathBuf> {
    if !executable(binary) {
        return Err(format!("No monoBOT binary at {}", binary.display()));
    }
    Ok(tempfile::Builder::new()
        .prefix("monocode-perf-")
        .tempdir()
        .map_err(|e| e.to_string())?
        .keep())
}

/// Starts a separate app window on throwaway data, with `claude` replaced by
/// the replay.
fn start(binary: &Path, work: PathBuf, env: &[(&str, String)]) -> Result<Run> {
    util::create_dir_all(&work.join("data"))?;
    util::create_dir_all(&work.join("project"))?;
    let log = fs::File::create(work.join("app.log")).map_err(|e| e.to_string())?;
    let replay = util::root().join("scripts/perf/fake-claude-stream.py");
    let child = util::command(binary)
        .args(["--dev-data", "--cwd"])
        .arg(work.join("project"))
        .env("MONOCODE_NATIVE_DATA_DIR", work.join("data"))
        .env("MONOCODE_CLAUDE_BINARY", replay)
        // A throwaway run adds nothing to the user's Claude Code and Codex
        // hooks; they would point at a folder that goes with the run.
        .env("MONOCODE_NOTCH_HOOKS", "off")
        .env("REPLAY_MARK", work.join("streaming"))
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log)
        .spawn()
        .map_err(|e| format!("{}: {e}", binary.display()))?;
    Ok(Run {
        work,
        app: AppGuard(child),
    })
}

/// Waits until a reply streams; `limit` in seconds, none to wait for good.
fn wait_for_stream(run: &mut Run, limit: Option<u64>) -> Result<()> {
    let mut waited = 0;
    while !run.work.join("streaming").exists() {
        if !run.app.alive() {
            return Err(format!(
                "monoBOT exited; see {}",
                run.work.join("app.log").display()
            ));
        }
        waited += 1;
        if limit.is_some_and(|limit| waited >= limit) {
            return Err(format!(
                "No reply streamed in {waited}s; see {}",
                run.work.join("app.log").display()
            ));
        }
        thread::sleep(Duration::from_secs(1));
    }
    Ok(())
}

fn sleep_secs(seconds: u64) {
    thread::sleep(Duration::from_secs(seconds));
}

fn profile(run: &Run, seconds: u64, output: &Path) -> Result<()> {
    util::create_dir_all(output)?;
    let summary = fs::File::create(output.join("summary.json")).map_err(|e| e.to_string())?;
    let status = Command::new(profiler())
        .args([
            "--profile",
            &seconds.to_string(),
            "--pid",
            &run.app.0.id().to_string(),
        ])
        .arg("--output")
        .arg(output)
        .stdout(Stdio::from(summary))
        .status()
        .map_err(|e| format!("profiler: {e}"))?;
    if !status.success() {
        return Err(format!("The profiler failed ({status})"));
    }
    Ok(())
}

fn profile_folder(output: &Path) -> Option<PathBuf> {
    fs::read_dir(output)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("profile-"))
        })
}

/// A number's leading digits, as awk reads `12.7%` or `3.35/5.10ms`.
fn leading_number(value: &str) -> f64 {
    let end = value
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .unwrap_or(value.len());
    value[..end].parse().unwrap_or(0.0)
}

/// The averages line for a frame log (`fps=39 layout=34 … main=14.8%`).
pub fn frame_averages(log: &str) -> Option<String> {
    let (mut n, mut fps, mut layout, mut paint, mut draw, mut busy, mut main) =
        (0u32, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for line in log.lines() {
        let fields: std::collections::HashMap<&str, &str> = line
            .split_whitespace()
            .filter_map(|field| field.split_once('='))
            .collect();
        let Some(frames) = fields.get("fps") else {
            continue;
        };
        let get = |key| fields.get(key).map_or(0.0, |v| leading_number(v));
        n += 1;
        fps += leading_number(frames);
        layout += get("layout");
        paint += get("paint");
        busy += get("busy");
        main += get("main");
        draw += get("draw");
    }
    let k = f64::from(n);
    (n > 0).then(|| {
        format!(
            "AVERAGE over {n}s: fps={:.0} layout={:.0} paint={:.0} draw={:.2}ms busy={:.1}% main={:.1}%",
            fps / k,
            layout / k,
            paint / k,
            draw / k,
            busy / k,
            main / k
        )
    })
}

/// Frames and main-thread CPU while a replayed reply streams, hands-off:
/// MONOCODE_PERF_PROMPT makes the first chat send a message by itself and
/// MONOCODE_FRAME_LOG records one line per second. `MODE=scroll` scrolls a
/// finished reply instead; `PROFILE=1` also samples stacks.
fn frame_run(binary: &Path, seconds: u64) -> Result<()> {
    let scroll = env_or("MODE", "stream") == "scroll";
    let reply_seconds: u64 = if scroll {
        env_or("SCROLL_REPLY_SECONDS", "25").parse().unwrap_or(25)
    } else {
        seconds + 15
    };
    let mut env = vec![("REPLAY_SECONDS", reply_seconds.to_string())];
    if scroll {
        env.push(("MONOCODE_PERF_SCROLL", env_or("SCROLL_SPEED", "1500")));
    }
    let work = new_work(binary)?;
    let log_path = work.join("frames.log");
    env.push(("MONOCODE_PERF_PROMPT", "stream".into()));
    env.push((
        "MONOCODE_FRAME_LOG",
        log_path.to_string_lossy().into_owned(),
    ));
    let mut run = start(binary, work, &env)?;
    wait_for_stream(&mut run, Some(60))?;
    if scroll {
        // Let the reply finish and the scroll get going.
        sleep_secs(reply_seconds + 4);
    } else {
        sleep_secs(env_or("PROFILE_DELAY", "3").parse().unwrap_or(3));
    }
    util::write(&log_path, "")?;
    let output = run.work.join("profile");
    if env_or("PROFILE", "0") == "1" && executable(&profiler()) {
        profile(&run, seconds, &output)?;
    } else {
        sleep_secs(seconds);
    }
    let window = run.work.join("window.log");
    util::copy(&log_path, &window)?;
    let text = fs::read_to_string(&window).map_err(|e| e.to_string())?;
    print!("{text}");
    if let Some(averages) = frame_averages(&text) {
        println!("\n{averages}");
    }
    println!("Log: {}", window.display());
    if let Some(folder) = profile_folder(&output) {
        println!("Profile: {}", folder.display());
    }
    Ok(())
}

/// Opens a window with the replay as Claude, waits for you to send a message,
/// then profiles that window while the reply streams.
fn stream_profile(binary: &Path, seconds: u64) -> Result<()> {
    if !executable(&profiler()) {
        return Err(format!(
            "No profiler at {} (set MONOCODE_PROFILER)",
            profiler().display()
        ));
    }
    let env = [("REPLAY_SECONDS", (seconds + 15).to_string())];
    let mut run = start(binary, new_work(binary)?, &env)?;
    println!(
        "Started monoBOT (PID {}) on {}.",
        run.app.0.id(),
        run.work.join("data").display()
    );
    println!("Send any message in a Claude chat there; profiling starts when the reply streams.");
    wait_for_stream(&mut run, None)?;
    // Let the first frames and the transcript settle before sampling.
    sleep_secs(env_or("PROFILE_DELAY", "3").parse().unwrap_or(3));
    let output = run.work.join("profile");
    profile(&run, seconds, &output)?;
    match profile_folder(&output) {
        Some(folder) => println!("Report: {}", folder.join("report.md").display()),
        None => println!("Report: {}", output.display()),
    }
    Ok(())
}

pub fn main(argv: &[String]) -> Result<()> {
    let (command, rest) = argv.split_first().ok_or(USAGE)?;
    let binary = PathBuf::from(rest.first().map_or(INSTALLED, String::as_str));
    let seconds = |default: u64| -> Result<u64> {
        rest.get(1)
            .map_or(Ok(default), |s| s.parse().map_err(|_| USAGE.to_owned()))
    };
    match command.as_str() {
        "frame-run" => frame_run(&binary, seconds(20)?),
        "stream-profile" => stream_profile(&binary, seconds(30)?),
        _ => Err(USAGE.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn averages_frame_log_lines() {
        let log = "fps=40 layout=30 paint=10 refresh=0 draw=3.00/5.10ms busy=10.0% main=20.0% views=330/30\n  by: x=1 | measure:markdown=7417\nfps=20 layout=10 paint=10 refresh=0 draw=1.00/2.00ms busy=20.0% main=10.0%\n";
        assert_eq!(
            frame_averages(log).unwrap(),
            "AVERAGE over 2s: fps=30 layout=20 paint=10 draw=2.00ms busy=15.0% main=15.0%"
        );
        assert_eq!(frame_averages(""), None);
    }
}
