//! `cargo xtask ssh-smoke`: the native SSH transport against disposable
//! loopback services.
//!
//! Needs OpenSSH's sshd, a built host package for this platform and Cargo.
//! Does not touch HOME, SSH configuration, authorized_keys or OS services.
use crate::util::{self, Result};
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

const USAGE: &str = "usage: cargo xtask ssh-smoke [--desktop <monocode-gpui>] [--host <monocode-host>] [--profile <profile>]";

fn free_port() -> Result<u16> {
    let probe = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    Ok(probe.local_addr().map_err(|e| e.to_string())?.port())
}

fn user_name() -> Result<String> {
    // SAFETY: getpwuid returns a pointer into static storage or null; the
    // name is copied out before any other passwd call.
    unsafe {
        let entry = libc::getpwuid(libc::getuid());
        if entry.is_null() {
            return Err("Could not read the current user name".into());
        }
        Ok(std::ffi::CStr::from_ptr((*entry).pw_name)
            .to_string_lossy()
            .into_owned())
    }
}

fn random_hex(bytes: usize) -> Result<String> {
    let mut buffer = vec![0u8; bytes];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buffer))
        .map_err(|e| e.to_string())?;
    Ok(buffer.iter().map(|b| format!("{b:02x}")).collect())
}

/// Waits for `child` up to `timeout`, then kills it.
fn output_within(mut child: Child, timeout: Duration, what: &str) -> Result<Output> {
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            return child.wait_with_output().map_err(|e| e.to_string());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{what} timed out after {}s", timeout.as_secs()));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn check(status: std::process::ExitStatus, what: &str) -> Result<()> {
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what} failed ({status})"))
    }
}

fn default_launcher(root: &Path) -> Result<PathBuf> {
    let system = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        other => return Err(format!("ssh-smoke does not run on {other}")),
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => return Err(format!("ssh-smoke does not run on {other}")),
    };
    Ok(root
        .join("host/build/host-packages")
        .join(format!("{system}-{arch}"))
        .join("monocode-host"))
}

pub fn main(argv: &[String]) -> Result<()> {
    let root = util::root();
    let mut desktop = None;
    let mut launcher = None;
    let mut profile = "native-dev".to_owned();
    let mut args = argv.iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().cloned().ok_or_else(|| USAGE.to_owned());
        match arg.as_str() {
            "--desktop" => desktop = Some(PathBuf::from(value()?)),
            "--host" => launcher = Some(PathBuf::from(value()?)),
            "--profile" => profile = value()?,
            _ => return Err(USAGE.into()),
        }
    }
    let launcher = match launcher {
        Some(path) => path,
        None => default_launcher(&root)?,
    };
    let profile_directory = if profile == "dev" { "debug" } else { &profile };
    let metadata = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(&root)
        .output()
        .map_err(|e| e.to_string())?;
    check(metadata.status, "cargo metadata")?;
    let metadata: Value = serde_json::from_slice(&metadata.stdout).map_err(|e| e.to_string())?;
    let mut target = PathBuf::from(
        metadata["target_directory"]
            .as_str()
            .ok_or("cargo metadata has no target_directory")?,
    );
    if let Ok(triple) = std::env::var("CARGO_BUILD_TARGET") {
        if !triple.is_empty() {
            target = target.join(triple);
        }
    }
    let desktop = desktop.unwrap_or_else(|| target.join(profile_directory).join("monocode-gpui"));
    let desktop = std::path::absolute(&desktop).map_err(|e| e.to_string())?;
    if !desktop.is_file() {
        return Err(format!(
            "Run cargo build -p monocode-gpui --profile {profile} first, or pass --desktop with an existing build"
        ));
    }
    if !launcher.is_file() {
        return Err(format!(
            "No host package at {}: build the host package first, or pass --host",
            launcher.display()
        ));
    }
    let sshd = ["/usr/sbin/sshd", "/usr/bin/sshd", "/usr/local/sbin/sshd"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("sshd"));
    let temp = tempfile::Builder::new()
        .prefix("monocode-ssh-integration-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let folder = temp.path();
    for key in ["host", "client"] {
        let status = Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-f"])
            .arg(folder.join(key))
            .status()
            .map_err(|e| format!("ssh-keygen: {e}"))?;
        check(status, "ssh-keygen")?;
    }
    let (ssh_port, host_port) = (free_port()?, free_port()?);
    let config = folder.join("sshd_config");
    let dir = folder.display();
    util::write(
        &config,
        format!("Port {ssh_port}\nListenAddress 127.0.0.1\nHostKey {dir}/host\nPidFile {dir}/pid\nAuthorizedKeysFile {dir}/client.pub\nStrictModes no\nUsePAM no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nLogLevel ERROR\n"),
    )?;
    let host_key = std::fs::read_to_string(folder.join("host.pub")).map_err(|e| e.to_string())?;
    util::write(
        &folder.join("known_hosts"),
        format!("[127.0.0.1]:{ssh_port} {host_key}"),
    )?;
    let mut daemon = Command::new(&sshd)
        .args(["-D", "-e", "-f"])
        .arg(&config)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{}: {e}", sshd.display()))?;
    let data = folder.join("data");
    let host = |args: &[&str]| -> Result<String> {
        let child = Command::new(&launcher)
            .args(args)
            .arg("--data-dir")
            .arg(&data)
            .args(["--port", &host_port.to_string()])
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{}: {e}", launcher.display()))?;
        let output = output_within(child, Duration::from_secs(15), "monocode-host")?;
        check(output.status, &format!("monocode-host {}", args.join(" ")))?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    };
    let result = run(
        &RunContext {
            folder,
            desktop: &desktop,
            root: &root,
            profile: &profile,
            ssh_port,
            host_port,
            host: &host,
        },
        &mut daemon,
    );
    if data.join("running.json").exists() {
        let _ = host(&["stop"]);
        for _ in 0..50 {
            if !data.join("running.json").exists() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    // SAFETY: SIGTERM to our own sshd child.
    unsafe {
        libc::kill(daemon.id() as libc::pid_t, libc::SIGTERM);
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while matches!(daemon.try_wait(), Ok(None)) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    if matches!(daemon.try_wait(), Ok(None)) {
        let _ = daemon.kill();
        let _ = daemon.wait();
    }
    result
}

struct RunContext<'a> {
    folder: &'a Path,
    desktop: &'a Path,
    root: &'a Path,
    profile: &'a str,
    ssh_port: u16,
    host_port: u16,
    host: &'a dyn Fn(&[&str]) -> Result<String>,
}

fn run(context: &RunContext, daemon: &mut Child) -> Result<()> {
    let RunContext {
        folder,
        desktop,
        root,
        profile,
        ssh_port,
        host_port,
        host: _,
    } = *context;
    thread::sleep(Duration::from_millis(300));
    if daemon.try_wait().map_err(|e| e.to_string())?.is_some() {
        let mut message = String::new();
        if let Some(mut stderr) = daemon.stderr.take() {
            let _ = stderr.read_to_string(&mut message);
        }
        return Err(format!("sshd did not start: {message}"));
    }
    let user = user_name()?;
    // Exercise the desktop executable's non-GUI askpass mode. Only trust the
    // fingerprint of the disposable key created above.
    let listing = Command::new("ssh-keygen")
        .arg("-lf")
        .arg(folder.join("host.pub"))
        .output()
        .map_err(|e| e.to_string())?;
    let fingerprint = String::from_utf8_lossy(&listing.stdout)
        .split_whitespace()
        .nth(1)
        .ok_or("ssh-keygen printed no fingerprint")?
        .to_owned();
    let prompts = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let prompt_port = prompts.local_addr().map_err(|e| e.to_string())?.port();
    let secret = random_hex(32)?;
    let results: Arc<Mutex<Vec<Value>>> = Arc::default();
    let worker = {
        let (results, secret, fingerprint) = (results.clone(), secret.clone(), fingerprint.clone());
        thread::spawn(move || {
            let answer = || -> Result<Value> {
                prompts.set_nonblocking(true).map_err(|e| e.to_string())?;
                let deadline = Instant::now() + Duration::from_secs(20);
                let mut stream = loop {
                    match prompts.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            if Instant::now() >= deadline {
                                return Err("timed out".into());
                            }
                            thread::sleep(Duration::from_millis(20));
                        }
                        Err(e) => return Err(e.to_string()),
                    }
                };
                stream.set_nonblocking(false).map_err(|e| e.to_string())?;
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .map_err(|e| e.to_string())?;
                let mut line = String::new();
                BufReader::new((&stream).take(16384))
                    .read_line(&mut line)
                    .map_err(|e| e.to_string())?;
                let request: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
                let prompt = request["prompt"].as_str().unwrap_or_default();
                let valid = request["secret"].as_str() == Some(&secret)
                    && request["confirm"].as_bool() == Some(true)
                    && prompt.contains(&fingerprint);
                let reply = if valid { "\"yes\"\n" } else { "null\n" };
                stream
                    .write_all(reply.as_bytes())
                    .map_err(|e| e.to_string())?;
                Ok(if valid {
                    Value::Bool(true)
                } else {
                    serde_json::json!({
                        "confirm": request["confirm"],
                        "prompt": prompt,
                        "expectedFingerprint": fingerprint,
                    })
                })
            };
            let result = answer().unwrap_or_else(Value::String);
            results.lock().unwrap().push(result);
        })
    };
    let ssh = Command::new("ssh")
        .args(["-F", "/dev/null", "-i"])
        .arg(folder.join("client"))
        .args(["-p", &ssh_port.to_string()])
        .args([
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "StrictHostKeyChecking=ask",
            "-o",
        ])
        .arg(format!(
            "UserKnownHostsFile={}/prompt_known_hosts",
            folder.display()
        ))
        .arg(format!("{user}@127.0.0.1"))
        .args(["printf", "askpass-ok"])
        .env("SSH_ASKPASS", desktop)
        .env("SSH_ASKPASS_REQUIRE", "force")
        .env("DISPLAY", "monocode:0")
        .env(
            "MONOCODE_SSH_ASKPASS_ADDRESS",
            format!("127.0.0.1:{prompt_port}"),
        )
        .env("MONOCODE_SSH_ASKPASS_SECRET", &secret)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ssh: {e}"))?;
    let ssh = output_within(ssh, Duration::from_secs(25), "ssh")?;
    let deadline = Instant::now() + Duration::from_secs(1);
    while !worker.is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    let results = results.lock().unwrap().clone();
    if results != [Value::Bool(true)] {
        return Err(format!(
            "The askpass prompt was not answered as expected: {results:?}"
        ));
    }
    if !ssh.status.success() || ssh.stdout != b"askpass-ok" {
        return Err(format!(
            "ssh through the native askpass failed: {}",
            String::from_utf8_lossy(&ssh.stderr)
        ));
    }
    println!("Native askpass verified the disposable SSH host fingerprint.");

    (context.host)(&["start"])?;
    let device: Value = serde_json::from_str(&(context.host)(&[
        "pair",
        "--name",
        "Loopback test",
        "--json",
    ])?)
    .map_err(|e| e.to_string())?;
    let field = |key: &str| {
        device[key]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("The paired device has no {key}"))
    };
    let status = Command::new("cargo")
        .args([
            "test",
            "--locked",
            "--offline",
            "-j",
            "1",
            "-p",
            "monocode-core",
            "--profile",
            profile,
            "--lib",
        ])
        .arg("remote::ssh::tests::loopback_transport_preserves_host_and_reconnects")
        .args(["--", "--ignored"])
        .current_dir(root)
        .env("MONOCODE_TEST_SSH_TARGET", format!("{user}@127.0.0.1"))
        .env("MONOCODE_TEST_SSH_PORT", ssh_port.to_string())
        .env("MONOCODE_TEST_HOST_PORT", host_port.to_string())
        .env("MONOCODE_TEST_SSH_KEY", folder.join("client"))
        .env("MONOCODE_TEST_KNOWN_HOSTS", folder.join("known_hosts"))
        .env("MONOCODE_TEST_TOKEN", field("token")?)
        .env("MONOCODE_TEST_ENVIRONMENT", field("environmentId")?)
        .status()
        .map_err(|e| e.to_string())?;
    check(status, "The loopback transport test")?;
    let info: Value =
        serde_json::from_str(&(context.host)(&["connection-info"])?).map_err(|e| e.to_string())?;
    if info["port"].as_u64() != Some(u64::from(host_port)) {
        return Err(format!("The host moved off port {host_port}: {info}"));
    }
    println!("Packaged host survived both native SSH tunnel closures.");
    Ok(())
}
