//! Claude Code's and Codex's own sessions in the notch, through their hooks.
//!
//! Each hook is a one-line shell command, so monoBOT ships no helper: it
//! writes the source and the hook's JSON to the notch's Unix socket with
//! `nc`, and always succeeds. When monoBOT is closed `nc` fails at once, and
//! the agent never waits on the notch. monoBOT's entries carry [`MARK`]; the
//! merge adds them beside everybody else's hooks and touches nothing else.
//! Claude Code reads `~/.claude/settings.json`; Codex reads
//! `~/.codex/hooks.json`, and runs a new hook only once the user trusts it
//! in Codex's `/hooks`.
//!
//! Windows has no `nc -U`: there the notch listens on a loopback port
//! ([`loopback`]) and the hook sends its JSON with `curl`, which Windows
//! ships. Claude Code runs its hooks in Git Bash there, so the command keeps
//! its shell form; Codex's shell on Windows is not known, so only Claude
//! Code's hooks are added.
use super::{state_word, Activity};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;

/// In every command monoBOT writes, so it can find its own entries.
pub const MARK: &str = "monobot-notch";

/// The Claude Code events the notch follows, with each hook's timeout.
pub const CLAUDE_EVENTS: &[(&str, u64)] = &[
    ("SessionStart", 5),
    ("SessionEnd", 5),
    ("UserPromptSubmit", 5),
    ("PreToolUse", 5),
    ("PostToolUse", 5),
    ("PermissionRequest", 5),
    ("Notification", 5),
    ("Stop", 5),
    ("StopFailure", 5),
];

/// Codex's: it has no Notification or StopFailure, and SessionEnd and
/// Interrupt allow at most 3 seconds.
pub const CODEX_EVENTS: &[(&str, u64)] = &[
    ("SessionStart", 5),
    ("SessionEnd", 3),
    ("UserPromptSubmit", 5),
    ("PreToolUse", 5),
    ("PostToolUse", 5),
    ("PermissionRequest", 5),
    ("Stop", 5),
    ("Interrupt", 3),
];

/// Where the notch listens on Windows: a loopback port of its own, written
/// where a socket's path goes elsewhere. The address carries [`MARK`].
pub fn loopback(port: u16) -> std::path::PathBuf {
    format!("http://127.0.0.1:{port}/{MARK}").into()
}

/// The loopback address `endpoint` names, if it is one.
fn loopback_url(endpoint: &Path) -> Option<String> {
    let text = endpoint.to_str()?;
    text.strip_prefix("http://127.0.0.1:")
        .and_then(|rest| rest.split_once('/'))
        .filter(|(port, path)| port.parse::<u16>().is_ok() && *path == MARK)
        .map(|_| text.to_owned())
}

/// The port a copy of monoBOT listens on, from its data folder: stable
/// across runs, so its hook command stays the same, and apart from another
/// copy's (a development build beside the installed app).
pub fn loopback_port(data: &str) -> u16 {
    47_000 + (external_id(data).unsigned_abs() % 1_000) as u16
}

/// The hook's shell command: the source on the first line, then the
/// hook's JSON, to the socket; always a success.
pub fn command(source: &str, socket: &Path) -> String {
    if let Some(url) = loopback_url(socket) {
        // Over loopback the source is the address's last part and the JSON
        // is the body; `curl` ships with Windows. Claude Code runs a hook in
        // Git Bash; Codex runs it as a plain command line (the other apps'
        // hooks there are a quoted program and its words), so its form has
        // no shell in it. The address carries MARK. monoBOT's own sessions
        // are told apart by the notch, which sees itself among the senders.
        return if source == "codex" {
            format!("curl.exe -s -m 2 --data-binary @- {url}/{source}")
        } else {
            format!(
                "[ -n \"$MONOCODE_HARNESS_PARENT\" ] || curl -s -m 2 --data-binary @- '{url}/{source}' >/dev/null 2>&1; true # {MARK}"
            )
        };
    }
    let socket = socket.to_string_lossy().replace('\'', r"'\''");
    // Sessions monoBOT runs itself carry MONOCODE_HARNESS_PARENT and are
    // already in the notch: their hooks stay quiet. The first line also
    // names the app the agent runs in (macOS passes its bundle id down), so
    // the notch can bring that app forward.
    format!(
        "[ -n \"$MONOCODE_HARNESS_PARENT\" ] || {{ printf '{source} %s\\n' \"$__CFBundleIdentifier\"; cat; }} | nc -U -w 1 '{socket}' >/dev/null 2>&1; true # {MARK}"
    )
}

/// The socket as the command quotes it, to find monoBOT's older commands
/// for it.
fn quoted(socket: &Path) -> String {
    if let Some(url) = loopback_url(socket) {
        return format!("{url}/");
    }
    format!("'{}'", socket.to_string_lossy().replace('\'', r"'\''"))
}

fn ours(handler: &Value) -> bool {
    handler["command"]
        .as_str()
        .is_some_and(|command| command.contains(MARK))
}

/// The hook file with monoBOT's entry for `socket` on every event, beside
/// whatever else it holds. `None` when it already has them all. A file that
/// is not a JSON object is refused rather than overwritten.
pub fn merge(
    file: &str,
    source: &str,
    events: &[(&str, u64)],
    socket: &Path,
) -> Result<Option<String>, String> {
    let mut root: Value = if file.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(file).map_err(|error| format!("not JSON: {error}"))?
    };
    let Some(object) = root.as_object_mut() else {
        return Err("the hook file is not a JSON object".into());
    };
    let hooks = object.entry("hooks").or_insert_with(|| json!({}));
    let Some(hooks) = hooks.as_object_mut() else {
        return Err("\"hooks\" is not an object".into());
    };
    let command = command(source, socket);
    let socket = quoted(socket);
    let mut changed = false;
    for (event, timeout) in events {
        let groups = hooks.entry(*event).or_insert_with(|| json!([]));
        let Some(groups) = groups.as_array_mut() else {
            return Err(format!("\"hooks.{event}\" is not a list"));
        };
        // An older monoBOT command for this same socket gives way to this one.
        let before: usize = groups
            .iter()
            .map(|group| group["hooks"].as_array().map_or(0, Vec::len))
            .sum();
        for group in groups.iter_mut() {
            if let Some(handlers) = group["hooks"].as_array_mut() {
                handlers.retain(|handler| {
                    let stale = handler["command"]
                        .as_str()
                        .is_some_and(|old| old != command && old.contains(&socket));
                    !(ours(handler) && stale)
                });
            }
        }
        groups.retain(|group| {
            group["hooks"]
                .as_array()
                .is_none_or(|handlers| !handlers.is_empty())
        });
        let after: usize = groups
            .iter()
            .map(|group| group["hooks"].as_array().map_or(0, Vec::len))
            .sum();
        changed |= after != before;
        let present = groups.iter().any(|group| {
            group["hooks"]
                .as_array()
                .is_some_and(|handlers| handlers.iter().any(|h| h["command"] == command.as_str()))
        });
        if !present {
            groups.push(json!({
                "hooks": [{ "type": "command", "command": command, "timeout": timeout }]
            }));
            changed = true;
        }
    }
    if !changed {
        return Ok(None);
    }
    serde_json::to_string_pretty(&root)
        .map(Some)
        .map_err(|error| error.to_string())
}

/// The socket one of monoBOT's commands writes to.
fn socket_of(command: &str) -> Option<std::path::PathBuf> {
    let (_, rest) = command.split_once("nc -U -w 1 '")?;
    let (socket, _) = rest.rsplit_once("' >/dev/null")?;
    Some(socket.replace(r"'\''", "'").into())
}

/// The hook file without monoBOT's entries for sockets that are `gone`:
/// another copy's (a development or profiling run), whose data folder was
/// deleted. Left in, each would run on every hook for nothing. `keep` is
/// this copy's own socket and always stays. `None` when nothing leaves.
pub fn forget(
    file: &str,
    keep: &Path,
    gone: impl Fn(&Path) -> bool,
) -> Result<Option<String>, String> {
    let mut root: Value =
        serde_json::from_str(file).map_err(|error| format!("not JSON: {error}"))?;
    let mut changed = false;
    if let Some(hooks) = root["hooks"].as_object_mut() {
        for groups in hooks.values_mut() {
            let Some(groups) = groups.as_array_mut() else {
                continue;
            };
            groups.retain_mut(|group| {
                let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                    return true;
                };
                let before = handlers.len();
                handlers.retain(|handler| {
                    let stale = handler["command"]
                        .as_str()
                        .and_then(socket_of)
                        .is_some_and(|socket| socket != keep && gone(&socket));
                    !(ours(handler) && stale)
                });
                changed |= handlers.len() != before;
                // Only a group this emptied goes; the others are not ours.
                handlers.len() == before || !handlers.is_empty()
            });
        }
    }
    if !changed {
        return Ok(None);
    }
    serde_json::to_string_pretty(&root)
        .map(Some)
        .map_err(|error| error.to_string())
}

/// The hook file without any of monoBOT's entries; everything else stays.
pub fn remove(file: &str) -> Result<String, String> {
    let mut root: Value = serde_json::from_str(file).map_err(|error| error.to_string())?;
    if let Some(hooks) = root["hooks"].as_object_mut() {
        for groups in hooks.values_mut() {
            if let Some(groups) = groups.as_array_mut() {
                for group in groups.iter_mut() {
                    if let Some(handlers) = group["hooks"].as_array_mut() {
                        handlers.retain(|handler| !ours(handler));
                    }
                }
                groups.retain(|group| {
                    group["hooks"]
                        .as_array()
                        .is_none_or(|handlers| !handlers.is_empty())
                });
            }
        }
    }
    serde_json::to_string_pretty(&root).map_err(|error| error.to_string())
}

/// Adds monoBOT's hooks for `socket` to Claude Code's and Codex's files
/// under `home`, where each agent is installed (its folder exists). The
/// first change of a file keeps a copy beside it (`.monobot-backup`), and
/// every write goes through a temporary file. Returns what was done, by
/// file, for the log.
pub fn install(home: &Path, socket: &Path) -> Vec<(std::path::PathBuf, Result<bool, String>)> {
    let codex_home = std::env::var_os("CODEX_HOME")
        .filter(|dir| !dir.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    let targets = [
        (
            home.join(".claude"),
            "settings.json",
            "claude",
            CLAUDE_EVENTS,
        ),
        (codex_home, "hooks.json", "codex", CODEX_EVENTS),
    ];
    targets
        .into_iter()
        .filter(|(dir, ..)| dir.is_dir())
        .map(|(dir, name, source, events)| {
            let path = dir.join(name);
            let result = install_file(&path, source, events, socket);
            (path, result)
        })
        .collect()
}

fn install_file(
    path: &Path,
    source: &str,
    events: &[(&str, u64)],
    socket: &Path,
) -> Result<bool, String> {
    let current = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.to_string()),
    };
    // Entries for sockets whose folder is gone leave with the same write.
    let pruned = if current.trim().is_empty() {
        None
    } else {
        forget(&current, socket, |socket| !socket.exists())?
    };
    let merged = match merge(
        pruned.as_deref().unwrap_or(&current),
        source,
        events,
        socket,
    )? {
        Some(merged) => merged,
        None => match pruned {
            Some(pruned) => pruned,
            None => return Ok(false),
        },
    };
    if !current.is_empty() {
        let backup = path.with_extension("json.monobot-backup");
        if !backup.exists() {
            std::fs::write(&backup, &current).map_err(|error| error.to_string())?;
        }
    }
    let temporary = path.with_extension("json.monobot-tmp");
    std::fs::write(&temporary, merged + "\n").map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, path).map_err(|error| error.to_string())?;
    Ok(true)
}

/// Listens on `socket` on a thread of its own and hands every hook call to
/// `on_event`. A connection that sends nothing readable is dropped.
#[cfg(unix)]
pub fn serve(socket: &Path, on_event: impl Fn(HookEvent) + Send + 'static) -> std::io::Result<()> {
    use std::io::Read;
    use std::os::unix::net::UnixListener;
    // A socket left by an earlier run would refuse the bind.
    let _ = std::fs::remove_file(socket);
    let listener = UnixListener::bind(socket)?;
    std::thread::Builder::new()
        .name("notch-hooks".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                let mut payload = String::new();
                // Hooks are small; a runaway sender stops at 256 KB.
                let read = (&mut stream).take(256 * 1024).read_to_string(&mut payload);
                if let Some(event) = read.ok().and_then(|_| parse(&payload)) {
                    on_event(event);
                }
            }
        })?;
    Ok(())
}

/// Listens on the loopback `port` (Windows' stand-in for the socket) on a
/// thread of its own. Each hook is one `POST /monobot-notch/<source>` with
/// its JSON as the body. A request from a web page (it carries `Origin`) or
/// to any other address is refused.
pub fn serve_loopback(
    port: u16,
    on_event: impl Fn(HookEvent) + Send + 'static,
) -> std::io::Result<()> {
    serve_loopback_for(port, Some(std::process::id()), on_event)
}

/// [`serve_loopback`], leaving out what comes from below the program `own`.
fn serve_loopback_for(
    port: u16,
    own: Option<u32>,
    on_event: impl Fn(HookEvent) + Send + 'static,
) -> std::io::Result<()> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))?;
    std::thread::Builder::new()
        .name("notch-hooks".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                // Who sent it, while the sender still waits for the answer:
                // the programs above it, so a click can bring forward the
                // app the agent runs in (a terminal, Claude, an editor).
                let senders = stream
                    .peer_addr()
                    .ok()
                    .map(|peer| senders(peer.port()))
                    .unwrap_or_default();
                let event = read_request(&mut stream).map(|mut event| {
                    if event.app.is_none() && !senders.is_empty() {
                        event.app = Some(pids_app(&senders));
                    }
                    event
                });
                // A session monoBOT runs itself is already in the notch: its
                // hooks come from below this very program.
                let own = own.is_some_and(|own| senders.contains(&own));
                let answer: &[u8] = if event.is_some() {
                    b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n"
                } else {
                    b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                };
                let _ = std::io::Write::write_all(&mut stream, answer);
                if let Some(event) = event.filter(|_| !own) {
                    on_event(event);
                }
            }
        })?;
    Ok(())
}

/// How many programs up from the hook's sender are kept: `curl`, its shell,
/// the agent, the shell or app that started it, and a few more.
#[cfg(windows)]
const SENDERS: usize = 12;

/// A hook's app as the programs it came from: `pids-<sender>-<parent>-…`,
/// nearest first (the characters a hook's app may hold).
fn pids_app(pids: &[u32]) -> String {
    let list: Vec<String> = pids.iter().map(u32::to_string).collect();
    format!("pids-{}", list.join("-"))
}

/// The programs a hook came from, by `pids_app`'s form; none for an app
/// named otherwise (a macOS bundle id).
pub fn app_pids(app: &str) -> Vec<u32> {
    app.strip_prefix("pids-")
        .map(|list| list.split('-').filter_map(|pid| pid.parse().ok()).collect())
        .unwrap_or_default()
}

/// The program on this computer whose connection comes from `port` on the
/// loopback, then its parents. Windows only; elsewhere the hook names its
/// app itself.
#[cfg(windows)]
fn senders(port: u16) -> Vec<u32> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
    };
    use windows_sys::Win32::Networking::WinSock::AF_INET;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    // The connection's owner, from the system's table of TCP connections.
    let owner = (|| {
        let mut size = 0u32;
        // SAFETY: a null table asks for the size it needs.
        unsafe {
            GetExtendedTcpTable(
                std::ptr::null_mut(),
                &mut size,
                0,
                u32::from(AF_INET),
                TCP_TABLE_OWNER_PID_ALL,
                0,
            )
        };
        let mut buffer = vec![0u32; (size as usize).div_ceil(4) + 64];
        size = (buffer.len() * 4) as u32;
        // SAFETY: the buffer holds `size` bytes, aligned for the table.
        let status = unsafe {
            GetExtendedTcpTable(
                buffer.as_mut_ptr().cast(),
                &mut size,
                0,
                u32::from(AF_INET),
                TCP_TABLE_OWNER_PID_ALL,
                0,
            )
        };
        if status != 0 {
            return None;
        }
        let count = buffer[0] as usize;
        // SAFETY: the table is its count and then `count` rows.
        let rows = unsafe {
            std::slice::from_raw_parts(
                buffer.as_ptr().add(1).cast::<MIB_TCPROW_OWNER_PID>(),
                count.min((buffer.len() - 1) / 6),
            )
        };
        // Ports are in network order, in the low 16 bits.
        rows.iter()
            .find(|row| u16::from_be(row.dwLocalPort as u16) == port)
            .map(|row| row.dwOwningPid)
    })();
    let Some(owner) = owner.filter(|pid| *pid != 0) else {
        return Vec::new();
    };
    let mut parents = HashMap::new();
    let mut names = HashMap::new();
    // SAFETY: a snapshot of the process list, walked and closed here.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return vec![owner];
        }
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                parents.insert(entry.th32ProcessID, entry.th32ParentProcessID);
                let name = &entry.szExeFile;
                let len = name.iter().position(|c| *c == 0).unwrap_or(name.len());
                names.insert(entry.th32ProcessID, String::from_utf16_lossy(&name[..len]));
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
    }
    let mut chain = vec![owner];
    while chain.len() < SENDERS {
        match parents.get(chain.last().unwrap_or(&0)) {
            Some(&parent) if parent != 0 && !chain.contains(&parent) => chain.push(parent),
            _ => break,
        }
    }
    let named: Vec<(u32, String)> = chain
        .into_iter()
        .map(|pid| (pid, names.get(&pid).cloned().unwrap_or_default()))
        .collect();
    lasting(&named)
}

/// The programs a hook runs through that end with it: `curl` and the shell
/// that started it.
const PASSING: &[&str] = &["curl.exe", "bash.exe", "sh.exe", "dash.exe", "cmd.exe"];

/// A hook's senders, nearest first, without the programs that ended with it
/// (`curl`, its shell). Windows soon hands their ids to other programs, so
/// a click could bring forward whatever got them; the agent and what runs it
/// stay.
pub fn lasting(chain: &[(u32, String)]) -> Vec<u32> {
    let first = chain
        .iter()
        .position(|(_, name)| {
            !PASSING
                .iter()
                .any(|passing| name.eq_ignore_ascii_case(passing))
        })
        .unwrap_or(chain.len());
    chain[first..].iter().map(|(pid, _)| *pid).collect()
}

/// Which window on screen to bring forward for a session: one of the
/// nearest of `pids` that shows any, and among its windows the one whose
/// title names the session (what it was asked, its folder); otherwise its
/// first, the one in front. `windows` is each window's program and title,
/// front first.
pub fn pick_window(windows: &[(u32, String)], pids: &[u32], hints: &[String]) -> Option<usize> {
    let pid = pids
        .iter()
        .find(|pid| windows.iter().any(|(owner, _)| owner == *pid))?;
    let own: Vec<usize> = (0..windows.len())
        .filter(|at| windows[*at].0 == *pid)
        .collect();
    let named = hints
        .iter()
        .map(|hint| hint.trim().trim_end_matches('…').to_lowercase())
        .filter(|hint| hint.chars().count() >= 3)
        .find_map(|hint| {
            own.iter()
                .copied()
                .find(|at| windows[*at].1.to_lowercase().contains(&hint))
        });
    named.or(own.first().copied())
}

#[cfg(not(windows))]
fn senders(_: u16) -> Vec<u32> {
    Vec::new()
}

/// One hook's request, read off a loopback connection.
fn read_request(stream: &mut impl std::io::Read) -> Option<HookEvent> {
    use std::io::Read;
    // Hooks are small; a runaway sender stops at 256 KB.
    let mut reader = stream.take(256 * 1024);
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        if let Some(at) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        let read = reader.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..read]);
    };
    let head = std::str::from_utf8(&bytes[..head_end]).ok()?;
    let mut lines = head.split("\r\n");
    let mut request = lines.next()?.split_whitespace();
    let (method, path) = (request.next()?, request.next()?);
    let source = path.strip_prefix(&format!("/{MARK}/")).unwrap_or("");
    let mut length = 0usize;
    let mut from_a_page = false;
    for line in lines {
        let (name, value) = line.split_once(':')?;
        match name.trim().to_ascii_lowercase().as_str() {
            "origin" => from_a_page = true,
            "content-length" => length = value.trim().parse().ok()?,
            _ => {}
        }
    }
    // The body is read even when refused: closing on unread bytes resets
    // the connection and the sender never hears the answer.
    let mut body = bytes[head_end + 4..].to_vec();
    while body.len() < length {
        let read = reader.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(length);
    if method != "POST" || from_a_page {
        return None;
    }
    parse(&format!("{source}\n{}", String::from_utf8_lossy(&body)))
}

/// One hook call, as the socket received it.
#[derive(Clone, Debug, PartialEq)]
pub struct HookEvent {
    /// "claude" or "codex".
    pub source: String,
    /// The app the agent runs in, by bundle id ("com.apple.Terminal").
    pub app: Option<String>,
    pub session: String,
    pub cwd: String,
    pub event: String,
    pub tool: Option<String>,
    pub message: Option<String>,
    /// What the user asked, on `UserPromptSubmit`.
    pub prompt: Option<String>,
}

/// Reads what a hook sent: its source and app, a newline, then its JSON.
pub fn parse(payload: &str) -> Option<HookEvent> {
    let (head, json) = payload.split_once('\n')?;
    let mut head = head.split_whitespace();
    let source = match head.next()? {
        "claude" => "claude",
        "codex" => "codex",
        _ => return None,
    };
    // Only what a bundle id can hold: it is handed to `open -b`.
    let app = head.next().filter(|app| {
        app.len() <= 255
            && app
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    });
    let value: Value = serde_json::from_str(json.trim()).ok()?;
    let text = |key: &str| value[key].as_str().map(str::to_owned);
    Some(HookEvent {
        source: source.into(),
        app: app.map(str::to_owned),
        session: text("session_id")?,
        cwd: text("cwd").unwrap_or_default(),
        event: text("hook_event_name")?,
        tool: text("tool_name"),
        message: text("message"),
        prompt: text("prompt"),
    })
}

/// A session the agent runs on its own, as its hooks tell it.
#[derive(Clone, Debug, PartialEq)]
struct External {
    source: String,
    app: Option<String>,
    cwd: String,
    /// What it was last asked, in a few words: tells two sessions in the
    /// same folder apart.
    topic: Option<String>,
    step: String,
    working: bool,
    needs_you: bool,
    done: bool,
    started_at: Option<u64>,
    seen_at: u64,
}

/// The sessions outside monoBOT, by their agent's session id.
#[derive(Debug, Default)]
pub struct Externals {
    sessions: HashMap<String, External>,
}

/// A finished or idle session leaves after this long without news.
const IDLE_MS: u64 = 10 * 60_000;
/// A session said to work but silent this long is gone (killed, closed).
const SILENT_MS: u64 = 30 * 60_000;

impl Externals {
    pub fn apply(&mut self, event: HookEvent, now: u64) {
        if event.event == "SessionEnd" {
            self.sessions.remove(&event.session);
            return;
        }
        let session = self
            .sessions
            .entry(event.session)
            .or_insert_with(|| External {
                source: event.source.clone(),
                app: None,
                cwd: event.cwd.clone(),
                topic: None,
                step: "Thinking".into(),
                working: false,
                needs_you: false,
                done: false,
                started_at: None,
                seen_at: now,
            });
        session.seen_at = now;
        if event.app.is_some() {
            session.app = event.app;
        }
        if !event.cwd.is_empty() {
            session.cwd = event.cwd;
        }
        let start = |session: &mut External| {
            if !session.working {
                session.started_at = Some(now);
            }
            session.working = true;
            session.done = false;
        };
        match event.event.as_str() {
            "UserPromptSubmit" => {
                start(session);
                if let Some(topic) = event.prompt.as_deref().and_then(topic) {
                    session.topic = Some(topic);
                }
                session.needs_you = false;
                session.step = "Thinking".into();
            }
            "PreToolUse" => {
                start(session);
                session.needs_you = false;
                session.step = event.tool.unwrap_or_else(|| "Working".into());
            }
            "PostToolUse" => {
                // The tool ran, so any approval it waited on was given. The
                // step stays the last action until the next one, as the
                // agent's own window shows it; "Thinking" only before the
                // turn's first tool.
                start(session);
                session.needs_you = false;
            }
            "PermissionRequest" => {
                start(session);
                session.needs_you = true;
            }
            "Notification" => {
                // Claude Code says it waits for a permission, or for input.
                let message = event.message.unwrap_or_default().to_lowercase();
                if message.contains("permission") {
                    session.needs_you = true;
                }
            }
            "Stop" | "StopFailure" => {
                session.working = false;
                session.needs_you = false;
                session.done = true;
            }
            "Interrupt" => {
                session.working = false;
                session.needs_you = false;
            }
            _ => {}
        }
    }

    /// Lets go of sessions with no news for a while. Returns whether any
    /// went.
    pub fn prune(&mut self, now: u64) -> bool {
        let before = self.sessions.len();
        self.sessions.retain(|_, session| {
            let quiet = now.saturating_sub(session.seen_at);
            if session.working {
                quiet < SILENT_MS
            } else {
                quiet < IDLE_MS
            }
        });
        self.sessions.len() != before
    }

    /// The app a session runs in, by its notch id, to bring forward.
    pub fn app_for(&self, id: i64) -> Option<String> {
        self.target(id).map(|target| target.app)
    }

    /// What a click on a session brings forward: its app, and the words in
    /// the title of a window that shows this very session (what it was
    /// asked, its folder).
    pub fn target(&self, id: i64) -> Option<Target> {
        let session = self
            .sessions
            .iter()
            .find(|(session, _)| external_id(session) == id)
            .map(|(_, session)| session)?;
        let mut hints: Vec<String> = session.topic.iter().cloned().collect();
        hints.extend(folder(&session.cwd));
        Some(Target {
            app: session.app.clone()?,
            hints,
        })
    }

    /// The sessions at work, waiting or just finished, for the notch. Each
    /// gets a negative id of its own, so it never meets one of monoBOT's.
    pub fn activities(&self) -> Vec<Activity> {
        let mut activities: Vec<Activity> = self
            .sessions
            .iter()
            .filter(|(_, session)| session.working || session.done || session.needs_you)
            .map(|(id, session)| {
                let folder = folder(&session.cwd).unwrap_or_default();
                let who = if session.source == "codex" {
                    "Codex"
                } else {
                    "Claude Code"
                };
                // Two sessions in one folder read apart by what each was
                // asked; until then, by their agent.
                let what = session.topic.as_deref().unwrap_or(who);
                Activity {
                    id: external_id(id),
                    provider: session.source.clone(),
                    title: if folder.is_empty() {
                        what.into()
                    } else {
                        format!("{folder} · {what}")
                    },
                    step: if session.done {
                        "Done".into()
                    } else if session.needs_you {
                        "Need approval".into()
                    } else {
                        state_word(&session.step).into()
                    },
                    started_at: session.started_at,
                    needs_you: session.needs_you,
                    done: session.done && !session.working,
                    // A coding session, not one of the user's agents: the
                    // notch lists it with the coding chats.
                    agent: None,
                    steps: None,
                    face: None,
                }
            })
            .collect();
        activities.sort_by_key(|activity| activity.id);
        activities
    }
}

/// What a click on an outside session brings forward.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// The app it runs in: a macOS bundle id, or on Windows the programs
    /// it came from (see [`app_pids`]).
    pub app: String,
    /// Words in the title of a window showing this session, best first.
    pub hints: Vec<String>,
}

/// Whether a notch id is a session outside monoBOT (see [`external_id`]).
pub fn is_external(id: i64) -> bool {
    id < 0
}

/// A folder's last part, which names a session. Either separator: a
/// Windows path reads the same on any system.
fn folder(cwd: &str) -> Option<String> {
    let trimmed = cwd.trim_end_matches(['/', '\\']);
    let name = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    (!name.is_empty()).then(|| name.to_owned())
}

/// A prompt's first line, in a few words, for a session's title.
fn topic(prompt: &str) -> Option<String> {
    const LETTERS: usize = 48;
    let line = prompt
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    if line.chars().count() <= LETTERS {
        return Some(line.to_owned());
    }
    // Cut at the last word that fits, or mid-word if one word is longer.
    let cut: String = line.chars().take(LETTERS + 1).collect();
    let cut = cut.rsplit_once(char::is_whitespace).map_or_else(
        || cut.chars().take(LETTERS).collect(),
        |(head, _)| head.to_owned(),
    );
    Some(format!("{}…", cut.trim_end()))
}

/// A stable negative id from the agent's session id.
pub fn external_id(session: &str) -> i64 {
    // FNV-1a: stable across runs, unlike the standard hasher.
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in session.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    -((hash >> 1) as i64).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn socket() -> &'static Path {
        Path::new("/tmp/monobot test/notch.sock")
    }

    #[test]
    fn hooks_join_everybody_elses_and_only_once() {
        let file =
            r#"{"model":"x","hooks":{"Stop":[{"hooks":[{"type":"command","command":"other"}]}]}}"#;
        let merged = merge(file, "claude", CLAUDE_EVENTS, socket())
            .unwrap()
            .unwrap();
        let value: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(value["model"], "x", "other settings stay");
        let stop = value["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2, "the other tool's hook stays");
        assert_eq!(stop[0]["hooks"][0]["command"], "other");
        let ours = stop[1]["hooks"][0]["command"].as_str().unwrap();
        assert!(ours.contains(MARK) && ours.contains("'/tmp/monobot test/notch.sock'"));
        assert_eq!(
            value["hooks"].as_object().unwrap().len(),
            CLAUDE_EVENTS.len()
        );
        // Already there: nothing to write.
        assert_eq!(
            merge(&merged, "claude", CLAUDE_EVENTS, socket()).unwrap(),
            None
        );
        // Removing takes ours and leaves the rest.
        let removed: Value = serde_json::from_str(&remove(&merged).unwrap()).unwrap();
        assert_eq!(removed["hooks"]["Stop"].as_array().unwrap().len(), 1);
        assert!(removed["hooks"]["PreToolUse"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_file_that_is_not_hooks_is_refused() {
        assert!(merge("[1]", "codex", CODEX_EVENTS, socket()).is_err());
        assert!(merge(r#"{"hooks":3}"#, "codex", CODEX_EVENTS, socket()).is_err());
        assert!(merge("not json", "codex", CODEX_EVENTS, socket()).is_err());
        assert!(merge("", "codex", CODEX_EVENTS, socket())
            .unwrap()
            .is_some());
    }

    #[test]
    fn the_command_never_fails_and_quotes_the_socket() {
        let command = command("codex", Path::new("/tmp/it's/notch.sock"));
        assert!(command.starts_with(
            "[ -n \"$MONOCODE_HARNESS_PARENT\" ] || { printf 'codex %s\\n' \"$__CFBundleIdentifier\"; cat; } | nc -U -w 1 "
        ));
        assert!(command.contains(r"'/tmp/it'\''s/notch.sock'"));
        assert!(command.contains("; true #"));
    }

    #[test]
    fn an_older_command_for_the_same_socket_gives_way() {
        let old = format!("old {MARK} '/tmp/monobot test/notch.sock'");
        let other_app = format!("other {MARK} '/tmp/elsewhere.sock'");
        let file = json!({ "hooks": { "Stop": [
            { "hooks": [{ "type": "command", "command": old }] },
            { "hooks": [{ "type": "command", "command": other_app }] },
        ]}})
        .to_string();
        let merged = merge(&file, "claude", CLAUDE_EVENTS, socket())
            .unwrap()
            .unwrap();
        let value: Value = serde_json::from_str(&merged).unwrap();
        let commands: Vec<String> = value["hooks"]["Stop"]
            .as_array()
            .unwrap()
            .iter()
            .map(|group| group["hooks"][0]["command"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            commands.len(),
            2,
            "the old one replaced, the other app's kept"
        );
        assert!(commands.contains(&other_app));
        assert!(commands.contains(&command("claude", socket())));
    }

    #[test]
    fn a_bundle_id_is_read_and_anything_else_dropped() {
        let read = |head: &str| {
            parse(&format!(
                "{head}\n{{\"session_id\":\"s\",\"hook_event_name\":\"Stop\"}}"
            ))
            .unwrap()
            .app
        };
        assert_eq!(
            read("codex com.openai.codex").as_deref(),
            Some("com.openai.codex")
        );
        assert_eq!(read("claude"), None);
        assert_eq!(read("claude $(rm -rf ~)"), None);
    }

    fn event(name: &str, tool: Option<&str>) -> String {
        let tool = tool.map_or(String::new(), |tool| format!(r#","tool_name":"{tool}""#));
        format!(
            "claude com.apple.Terminal\n{{\"session_id\":\"s1\",\"cwd\":\"/Users/me/monoBOT\",\"hook_event_name\":\"{name}\"{tool}}}"
        )
    }

    #[test]
    fn a_session_works_waits_and_finishes_as_its_hooks_say() {
        let mut externals = Externals::default();
        for payload in [event("SessionStart", None), event("UserPromptSubmit", None)] {
            externals.apply(parse(&payload).unwrap(), 1_000);
        }
        let working = externals.activities();
        assert_eq!(working.len(), 1);
        assert_eq!(working[0].title, "monoBOT · Claude Code");
        assert_eq!(working[0].step, "Thinking");
        assert!(working[0].id < 0);
        assert_eq!(
            externals.app_for(working[0].id).as_deref(),
            Some("com.apple.Terminal")
        );
        externals.apply(parse(&event("PreToolUse", Some("Edit"))).unwrap(), 2_000);
        assert_eq!(externals.activities()[0].step, "Editing");
        externals.apply(
            parse(&event("PermissionRequest", Some("Bash"))).unwrap(),
            3_000,
        );
        assert!(externals.activities()[0].needs_you);
        // Approved: the tool ran, and the notch keeps its last action.
        externals.apply(parse(&event("PostToolUse", Some("Bash"))).unwrap(), 3_500);
        let after = &externals.activities()[0];
        assert!(!after.needs_you);
        assert_eq!(after.step, "Editing");
        externals.apply(parse(&event("Stop", None)).unwrap(), 4_000);
        let done = &externals.activities()[0];
        assert!(done.done && !done.needs_you);
        // A finished session leaves after a while; SessionEnd at once.
        assert!(!externals.prune(5_000));
        assert!(externals.prune(4_000 + IDLE_MS));
        externals.apply(parse(&event("UserPromptSubmit", None)).unwrap(), 1);
        externals.apply(parse(&event("SessionEnd", None)).unwrap(), 2);
        assert!(externals.activities().is_empty());
    }

    #[test]
    fn two_sessions_in_one_folder_read_apart_and_open_their_own_window() {
        let prompt = |session: &str, prompt: &str| {
            parse(&format!(
                "claude\n{}",
                json!({
                    "session_id": session,
                    "cwd": r"D:\dev\monoBOT",
                    "hook_event_name": "UserPromptSubmit",
                    "prompt": prompt,
                })
            ))
            .unwrap()
        };
        let mut externals = Externals::default();
        let mut first = prompt("a", "\n  Fix the notch's chats on Windows\nmore");
        first.app = Some("pids-30-20".into());
        externals.apply(first, 1);
        let long = "Make the tray menu open every Mono's floating chat at once";
        let mut second = prompt("b", long);
        second.app = Some("pids-31-20".into());
        externals.apply(second, 2);
        let titles: Vec<String> = externals
            .activities()
            .into_iter()
            .map(|activity| {
                // Coding sessions, listed with the coding chats.
                assert_eq!(activity.agent, None);
                assert_eq!(activity.who(), "Claude Code");
                activity.title
            })
            .collect();
        assert!(titles.contains(&"monoBOT · Fix the notch's chats on Windows".to_owned()));
        assert!(
            titles.contains(&"monoBOT · Make the tray menu open every Mono's floating…".to_owned())
        );
        // A turn without a prompt keeps the last words it was asked.
        let mut tool = prompt("a", "");
        tool.event = "PreToolUse".into();
        tool.prompt = None;
        externals.apply(tool, 3);
        let target = externals.target(external_id("a")).unwrap();
        assert_eq!(target.app, "pids-30-20");
        assert_eq!(
            target.hints,
            [
                "Fix the notch's chats on Windows".to_owned(),
                "monoBOT".to_owned()
            ]
        );
        // A window that names the session wins over the app's front one.
        let windows = [
            (9, "Other app".to_owned()),
            (20, "✳ Tray menu · Windows Terminal".to_owned()),
            (20, "✳ Fix the notch's chats on Windows".to_owned()),
        ];
        assert_eq!(
            pick_window(&windows, &app_pids(&target.app), &target.hints),
            Some(2)
        );
        assert_eq!(pick_window(&windows, &[30, 20], &[]), Some(1));
        assert_eq!(pick_window(&windows, &[30], &target.hints), None);
        assert!(is_external(external_id("a")) && !is_external(4));
    }

    #[test]
    fn curl_and_its_shell_are_not_kept_as_the_hooks_app() {
        let chain = |names: &[&str]| -> Vec<(u32, String)> {
            names
                .iter()
                .enumerate()
                .map(|(at, name)| (at as u32 + 1, (*name).to_owned()))
                .collect()
        };
        assert_eq!(
            lasting(&chain(&[
                "curl.exe",
                "BASH.EXE",
                "claude.exe",
                "WindowsTerminal.exe"
            ])),
            [3, 4]
        );
        assert_eq!(lasting(&chain(&["claude.exe", "bash.exe"])), [1, 2]);
        assert!(lasting(&chain(&["curl.exe"])).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn the_socket_hands_over_what_a_hook_sends() {
        let dir = std::env::temp_dir().join(format!("monobot-hooks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("n.sock");
        let (sent, received) = std::sync::mpsc::channel();
        serve(&socket, move |event| {
            let _ = sent.send(event);
        })
        .unwrap();
        let mut stream = std::os::unix::net::UnixStream::connect(&socket).unwrap();
        std::io::Write::write_all(&mut stream, event("Stop", None).as_bytes()).unwrap();
        drop(stream);
        let event = received
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert_eq!(
            (event.session.as_str(), event.event.as_str()),
            ("s1", "Stop")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn on_windows_a_hook_posts_to_the_notchs_own_port() {
        let endpoint = loopback(47_123);
        let command = command("claude", &endpoint);
        assert!(command.contains(
            "curl -s -m 2 --data-binary @- 'http://127.0.0.1:47123/monobot-notch/claude'"
        ));
        assert!(command.starts_with("[ -n \"$MONOCODE_HARNESS_PARENT\" ] ||"));
        assert!(command.ends_with(&format!("; true # {MARK}")));
        // An address is never read as a socket, nor a socket as an address.
        assert_eq!(socket_of(&command), None);
        assert!(loopback_url(Path::new("/tmp/notch.sock")).is_none());
        let merged = merge("", "claude", CLAUDE_EVENTS, &endpoint)
            .unwrap()
            .unwrap();
        assert_eq!(
            merge(&merged, "claude", CLAUDE_EVENTS, &endpoint).unwrap(),
            None
        );
        // The port follows the data folder and stays in its range.
        let data = r"C:\Users\me\AppData\Local\MonoBOT Development";
        assert_eq!(loopback_port(data), loopback_port(data));
        assert!((47_000..48_000).contains(&loopback_port(data)));
        // Codex runs its hook as a plain command line: no shell in it, and
        // the address still carries the mark that finds monoBOT's entries.
        let codex = super::command("codex", &endpoint);
        assert_eq!(
            codex,
            "curl.exe -s -m 2 --data-binary @- http://127.0.0.1:47123/monobot-notch/codex"
        );
        assert!(ours(&json!({ "command": codex })));
        let merged = merge("", "codex", CODEX_EVENTS, &endpoint)
            .unwrap()
            .unwrap();
        assert_eq!(
            merge(&merged, "codex", CODEX_EVENTS, &endpoint).unwrap(),
            None
        );
        assert!(remove(&merged).is_ok_and(|file| !file.contains("curl")));
        let home = std::env::temp_dir().join(format!("monobot-loop-{}", std::process::id()));
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        let done: Vec<_> = install(&home, &endpoint)
            .into_iter()
            .map(|(path, result)| (path.file_name().unwrap().to_owned(), result))
            .collect();
        assert_eq!(done, [("settings.json".into(), Ok(true))]);
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn the_notchs_port_hands_over_a_hooks_post_and_nothing_else() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let (sent, received) = std::sync::mpsc::channel();
        // This test is the sender: nothing of its own is left out here.
        serve_loopback_for(port, None, move |event| {
            let _ = sent.send(event);
        })
        .unwrap();
        let post = |head: &str| {
            let body = r#"{"session_id":"s1","hook_event_name":"Stop","cwd":"D:\\dev"}"#;
            let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(
                stream,
                "{head}\r\nHost: x\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            let mut answer = String::new();
            let _ = stream.read_to_string(&mut answer);
            answer
        };
        assert!(post(&format!("POST /{MARK}/claude HTTP/1.1")).starts_with("HTTP/1.1 204"));
        let event = received
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert_eq!(
            (
                event.source.as_str(),
                event.session.as_str(),
                event.event.as_str()
            ),
            ("claude", "s1", "Stop")
        );
        // On Windows the sender is found: this test, then its parents.
        #[cfg(windows)]
        assert_eq!(
            event
                .app
                .as_deref()
                .map(app_pids)
                .and_then(|pids| pids.first().copied()),
            Some(std::process::id())
        );
        assert_eq!(app_pids("pids-12-7-1"), [12, 7, 1]);
        assert!(app_pids("com.apple.Terminal").is_empty());
        // A web page, another address, an unknown source: refused.
        let page = format!("POST /{MARK}/claude HTTP/1.1\r\nOrigin: https://a.b");
        assert!(post(&page).starts_with("HTTP/1.1 400"));
        assert!(post("POST /other/claude HTTP/1.1").starts_with("HTTP/1.1 400"));
        assert!(post(&format!("POST /{MARK}/vim HTTP/1.1")).starts_with("HTTP/1.1 400"));
        assert!(received.try_recv().is_err());
        // What comes from below monoBOT itself (its own sessions' hooks) is
        // answered and left out.
        #[cfg(windows)]
        {
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            drop(listener);
            let (sent, received) = std::sync::mpsc::channel();
            serve_loopback(port, move |event| {
                let _ = sent.send(event);
            })
            .unwrap();
            let body = r#"{"session_id":"s2","hook_event_name":"Stop"}"#;
            let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(
                stream,
                "POST /{MARK}/codex HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            let mut answer = String::new();
            let _ = stream.read_to_string(&mut answer);
            assert!(answer.starts_with("HTTP/1.1 204"));
            assert!(received
                .recv_timeout(std::time::Duration::from_millis(300))
                .is_err());
        }
    }

    #[test]
    fn installing_writes_once_and_keeps_a_backup() {
        let home = std::env::temp_dir().join(format!("monobot-home-{}", std::process::id()));
        let claude = home.join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(claude.join("settings.json"), r#"{"model":"x"}"#).unwrap();
        let first = install_file(
            &claude.join("settings.json"),
            "claude",
            CLAUDE_EVENTS,
            socket(),
        );
        assert_eq!(first, Ok(true));
        let second = install_file(
            &claude.join("settings.json"),
            "claude",
            CLAUDE_EVENTS,
            socket(),
        );
        assert_eq!(second, Ok(false), "already there");
        let backup = std::fs::read_to_string(claude.join("settings.json.monobot-backup")).unwrap();
        assert_eq!(backup, r#"{"model":"x"}"#);
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn entries_for_sockets_that_are_gone_leave_and_everything_else_stays() {
        let gone = Path::new("/tmp/monocode-perf-gone/it's data/notch.sock");
        let other = Path::new("/tmp/another copy/notch.sock");
        let mut file =
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"say done"}]}]}}"#
                .to_owned();
        for socket in [gone, other, socket()] {
            file = merge(&file, "claude", CLAUDE_EVENTS, socket)
                .unwrap()
                .unwrap();
        }
        assert_eq!(socket_of(&command("claude", gone)).as_deref(), Some(gone));
        let kept = forget(&file, socket(), |socket| socket != other)
            .unwrap()
            .expect("the gone socket's entries leave");
        // This copy's own socket stays even though it is "gone" to the check.
        assert!(!kept.contains("monocode-perf-gone"));
        assert!(kept.contains("another copy") && kept.contains("say done"));
        let root: Value = serde_json::from_str(&kept).unwrap();
        assert_eq!(root["hooks"]["Stop"].as_array().unwrap().len(), 3);
        assert_eq!(root["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
        // Nothing gone: nothing to write.
        assert_eq!(forget(&kept, socket(), |socket| socket != other), Ok(None));
    }

    #[test]
    fn installing_forgets_a_deleted_copys_hooks() {
        let home = std::env::temp_dir().join(format!("monobot-forget-{}", std::process::id()));
        let claude = home.join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        let settings = claude.join("settings.json");
        let deleted = home.join("deleted/notch.sock");
        let stale = merge("", "claude", CLAUDE_EVENTS, &deleted)
            .unwrap()
            .unwrap();
        std::fs::write(&settings, stale).unwrap();
        assert_eq!(
            install_file(&settings, "claude", CLAUDE_EVENTS, socket()),
            Ok(true)
        );
        let written = std::fs::read_to_string(&settings).unwrap();
        assert!(!written.contains("deleted/notch.sock"));
        let root: Value = serde_json::from_str(&written).unwrap();
        assert_eq!(root["hooks"]["Stop"].as_array().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn only_hooks_from_claude_or_codex_are_read() {
        assert!(parse("other\n{}").is_none());
        assert!(parse("claude\nnot json").is_none());
        assert!(
            parse("codex\n{\"hook_event_name\":\"Stop\"}").is_none(),
            "no session"
        );
        assert_eq!(external_id("a"), external_id("a"));
        assert_ne!(external_id("a"), external_id("b"));
    }
}
