//! Git that a repository cannot turn against the user. An agent works in the
//! repository and can write its `.git/config` and `.git/hooks`; MonoBOT then
//! runs Git there in the background (status, diffs, checkpoints), with no
//! sandbox. Every call goes through here: settings that run nothing, no
//! hooks unless the user asked for a commit or a push, none of the app's
//! own Git variables, and every program the repository's own configuration
//! names (filters, fsmonitor, credential helpers, signers, ssh) replaced by
//! the user's own or by nothing. Ported from IVY's `code/git.rs`.
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Settings every call has: they run nothing.
const ALWAYS: [(&str, &str); 5] = [
    ("core.quotepath", "false"),
    ("core.fsmonitor", "false"),
    ("core.pager", "cat"),
    ("log.showSignature", "false"),
    ("protocol.ext.allow", "never"),
];

/// The environment of every call: no question on a terminal that is not
/// there, no editor, no pager, none of the system's attributes.
const ENV: [(&str, &str); 5] = [
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_ATTR_NOSYSTEM", "1"),
    ("GIT_EDITOR", ":"),
    ("GIT_SEQUENCE_EDITOR", ":"),
    ("GIT_PAGER", "cat"),
];

/// What the app's own environment might carry that would point Git
/// somewhere else or run something. A caller that needs one (its own index)
/// sets it again after.
const ENV_REMOVED: [&str; 9] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_EXTERNAL_DIFF",
    "GIT_NAMESPACE",
    "GIT_CONFIG",
];

/// Subcommands that run no program a repository's configuration names
/// (no filter, driver, helper or signer): they skip reading it first.
const PLAIN: [&str; 10] = [
    "rev-parse",
    "rev-list",
    "branch",
    "symbolic-ref",
    "for-each-ref",
    "update-ref",
    "diff-tree",
    "remote",
    "config",
    "merge-base",
];

/// Whether the repository's hooks may run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hooks {
    /// Never: everything MonoBOT runs on its own.
    Off,
    /// A commit or a push the user asked for, as in their terminal.
    Repo,
}

/// Where hooks are looked for when they are off: nowhere.
fn no_hooks() -> PathBuf {
    #[cfg(unix)]
    {
        PathBuf::from("/dev/null")
    }
    #[cfg(not(unix))]
    {
        static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        let dir = DIR.get_or_init(|| {
            crate::store::monocode_data_dir()
                .join("git")
                .join("no-hooks")
        });
        let _ = std::fs::create_dir_all(dir);
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
        dir.clone()
    }
}

/// `program` with the settings and environment every call has, without
/// reading the repository's configuration: for a call with no folder yet.
pub fn base(program: impl AsRef<std::ffi::OsStr>, hooks: Hooks) -> Command {
    let mut command = Command::new(program);
    crate::process::configure_child(&mut command);
    for (key, value) in ALWAYS {
        command.arg("-c").arg(format!("{key}={value}"));
    }
    if hooks == Hooks::Off {
        let mut path = OsString::from("core.hooksPath=");
        path.push(no_hooks());
        command.arg("-c").arg(path);
    }
    for key in ENV_REMOVED {
        command.env_remove(key);
    }
    command.envs(ENV);
    command
}

/// Git for a call in `dir` whose subcommand is `subcommand`: hardened, with
/// the repository's programs replaced. The caller adds the subcommand and
/// its arguments (and its own directory or `-C`). Fails when the
/// repository's configuration cannot be read or names a program in a way
/// that cannot be put out of the way.
pub fn command(dir: &Path, subcommand: Option<&str>, hooks: Hooks) -> Result<Command, String> {
    command_with(Path::new("git"), dir, subcommand, hooks)
}

/// [`command`] with Git at `program` (a resolved path).
pub fn command_with(
    program: &Path,
    dir: &Path,
    subcommand: Option<&str>,
    hooks: Hooks,
) -> Result<Command, String> {
    // A commit or a push the user asked for runs as in their terminal; what
    // MonoBOT runs on its own never runs the repository's programs.
    let guard = match subcommand {
        _ if hooks == Hooks::Repo => Guard::default(),
        Some(sub) if PLAIN.contains(&sub) => Guard::default(),
        _ => cached_guard(program, dir)?,
    };
    let mut command = base(program, hooks);
    for (key, value) in &guard.config {
        command.arg("-c").arg(format!("{key}={value}"));
    }
    if let Some(tree) = &guard.work_tree {
        command.arg("--work-tree").arg(tree);
    }
    Ok(command)
}

/// What a call takes from the user's configuration in place of the
/// repository's own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Guard {
    /// `-c key=value`, in order.
    config: Vec<(String, String)>,
    /// The repository moved its working tree (`core.worktree`): this one.
    work_tree: Option<PathBuf>,
}

/// The guard for `dir`, read again at most every few seconds: background
/// calls come in bursts, and reading the configuration is a process too.
fn cached_guard(program: &Path, dir: &Path) -> Result<Guard, String> {
    const FRESH: Duration = Duration::from_secs(3);
    static CACHE: Mutex<Option<HashMap<PathBuf, (Instant, Guard)>>> = Mutex::new(None);
    {
        let cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        let fresh = cache
            .as_ref()
            .and_then(|cache| cache.get(dir))
            .filter(|(at, _)| at.elapsed() < FRESH);
        if let Some((_, guard)) = fresh {
            return Ok(guard.clone());
        }
    }
    let guard = read_guard(program, dir)?;
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = cache.get_or_insert_with(HashMap::new);
    if cache.len() > 256 {
        cache.clear();
    }
    cache.insert(dir.to_path_buf(), (Instant::now(), guard.clone()));
    Ok(guard)
}

fn read_guard(program: &Path, dir: &Path) -> Result<Guard, String> {
    let out = base(program, Hooks::Off)
        .arg("-C")
        .arg(dir)
        .args(["config", "--list", "--show-scope", "-z"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Could not run Git: {e}"))?;
    if !out.status.success() {
        // Outside a repository there is no repository configuration.
        let error = String::from_utf8_lossy(&out.stderr);
        if error.contains("not a git repository") {
            return Ok(Guard::default());
        }
        return Err(error.trim().to_owned());
    }
    guard_from(&out.stdout, dir)
}

/// How a setting that names a program is put out of the way when the
/// repository set it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Program {
    /// The user's own value, else this one (which runs nothing, or Git's
    /// usual program).
    Single(&'static str),
    /// A filter's command: the user's own, else none, and no longer required.
    Filter,
    /// The credential helpers: the list starts again with the user's own.
    Helper,
    /// A signer: the call then signs nothing.
    Signing,
    /// `core.worktree`.
    WorkTree,
    /// A program with no inert stand-in (an external diff): the call is
    /// refused.
    Refuse,
}

/// Whether `key` (as `git config --list` writes it, section and name in
/// lower case) names a program Git would run, and how it is put out of the way.
fn program_key(key: &str) -> Option<Program> {
    let (section, rest) = key.split_once('.')?;
    let (sub, name) = rest.rsplit_once('.').unwrap_or(("", rest));
    Some(match (section, sub, name) {
        ("core", "", "sshcommand") => Program::Single("ssh"),
        ("core", "", "askpass") => Program::Single(""),
        ("core", "", "editor") => Program::Single(":"),
        ("core", "", "fsmonitor") => Program::Single("false"),
        ("core", "", "hookspath") => Program::Single(""),
        ("core", "", "worktree") => Program::WorkTree,
        ("credential", _, "helper") => Program::Helper,
        ("gpg", _, "program") | ("gpg", "ssh", "defaultkeycommand") => Program::Signing,
        ("filter", sub, "clean" | "smudge" | "process") if !sub.is_empty() => Program::Filter,
        ("diff", "", "external") => Program::Refuse,
        ("diff", sub, "command") if !sub.is_empty() => Program::Refuse,
        // `cat` shows the file as it is: no conversion runs.
        ("diff", sub, "textconv") if !sub.is_empty() => Program::Single("cat"),
        // A merge driver that always reports a conflict: a merge stops.
        ("merge", sub, "driver") if !sub.is_empty() => Program::Single("false"),
        ("remote", sub, "receivepack") if !sub.is_empty() => Program::Single("git-receive-pack"),
        ("remote", sub, "uploadpack") if !sub.is_empty() => Program::Single("git-upload-pack"),
        ("sequence", "", "editor") => Program::Single(":"),
        _ => return None,
    })
}

/// The scopes the user owns; everything else (`local`, `worktree`) is the
/// repository's, which an agent may have written.
fn users(scope: &str) -> bool {
    matches!(scope, "system" | "global" | "command")
}

/// `git config --list --show-scope -z` (`scope\0key\nvalue\0`…) as what to
/// replace for a call in `dir`.
fn guard_from(listing: &[u8], dir: &Path) -> Result<Guard, String> {
    let text = String::from_utf8_lossy(listing);
    let mut parts = text.split('\0');
    let mut entries: Vec<(bool, String, String)> = Vec::new();
    while let (Some(scope), Some(entry)) = (parts.next(), parts.next()) {
        let (key, value) = entry.split_once('\n').unwrap_or((entry, ""));
        if !key.is_empty() {
            entries.push((users(scope), key.to_owned(), value.to_owned()));
        }
    }
    fn set(config: &mut Vec<(String, String)>, key: &str, value: &str) -> Result<(), String> {
        // `-c` takes the name up to the first `=`: such a name cannot be said.
        if key.contains('=') || key.contains('\n') {
            return Err(
                "This repository's Git settings name a program MonoBOT cannot set aside, so Git was not run."
                    .into(),
            );
        }
        if !config.iter().any(|(k, v)| k == key && v == value) {
            config.push((key.to_owned(), value.to_owned()));
        }
        Ok(())
    }
    let mut guard = Guard::default();
    let mut done: Vec<&str> = Vec::new();
    let mut helpers = false;
    for (_, key, _) in entries.iter().filter(|(user, ..)| !user) {
        let Some(kind) = program_key(key) else {
            continue;
        };
        if done.contains(&key.as_str()) {
            continue;
        }
        done.push(key);
        let own = entries
            .iter()
            .rev()
            .find(|(user, k, _)| *user && k == key)
            .map(|(_, _, v)| v.as_str());
        match kind {
            Program::Single(inert) => set(&mut guard.config, key, own.unwrap_or(inert))?,
            Program::Filter => {
                set(&mut guard.config, key, own.unwrap_or(""))?;
                if own.is_none() {
                    let driver = &key[..key.rfind('.').unwrap_or(key.len())];
                    set(&mut guard.config, &format!("{driver}.required"), "false")?;
                }
            }
            Program::Helper => helpers = true,
            Program::Signing => {
                for key in ["commit.gpgSign", "tag.gpgSign", "push.gpgSign"] {
                    set(&mut guard.config, key, "false")?;
                }
            }
            Program::Refuse => match own {
                // The user's own wins over the repository's, which would
                // otherwise override it.
                Some(own) => set(&mut guard.config, key, own)?,
                None => {
                    return Err(format!(
                        "This repository's Git settings run a program ({key}), so MonoBOT did not run Git here."
                    ));
                }
            },
            Program::WorkTree => {
                guard.work_tree = Some(own_top(dir).ok_or(
                    "This repository moves its working tree elsewhere (core.worktree) and MonoBOT cannot find its own, so Git was not run.",
                )?)
            }
        }
    }
    if helpers {
        // An empty helper empties the list; then the user's own, in order.
        set(&mut guard.config, "credential.helper", "")?;
        for (_, key, value) in entries
            .iter()
            .filter(|(user, key, _)| *user && program_key(key) == Some(Program::Helper))
        {
            if !key.contains('=') {
                guard.config.push((key.clone(), value.clone()));
            }
        }
    }
    Ok(guard)
}

/// The top of the working tree `dir` is in, by where its `.git` is.
fn own_top(dir: &Path) -> Option<PathBuf> {
    let dir = std::fs::canonicalize(dir).ok()?;
    dir.ancestors()
        .find(|folder| folder.join(".git").exists())
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(entries: &[(&str, &str, &str)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (scope, key, value) in entries {
            out.extend_from_slice(format!("{scope}\0{key}\n{value}\0").as_bytes());
        }
        out
    }

    #[test]
    fn a_repositorys_programs_give_way_to_the_users_or_to_nothing() {
        let guard = guard_from(
            &listing(&[
                ("global", "core.sshcommand", "ssh -i ~/.ssh/work"),
                ("local", "core.sshcommand", "touch /tmp/pwned"),
                ("local", "core.fsmonitor", "./steal.sh"),
                ("local", "filter.evil.clean", "./steal.sh"),
                ("local", "diff.evil.textconv", "./steal.sh"),
                ("local", "gpg.program", "./steal.sh"),
                ("global", "credential.helper", "osxkeychain"),
                ("local", "credential.helper", "!./steal.sh"),
                ("local", "user.name", "Anyone"),
            ]),
            Path::new("/tmp"),
        )
        .unwrap();
        let has = |key: &str, value: &str| guard.config.iter().any(|(k, v)| k == key && v == value);
        assert!(
            has("core.sshcommand", "ssh -i ~/.ssh/work"),
            "the user's own"
        );
        assert!(has("core.fsmonitor", "false"));
        assert!(has("filter.evil.clean", "") && has("filter.evil.required", "false"));
        assert!(has("diff.evil.textconv", "cat"));
        assert!(has("commit.gpgSign", "false"));
        assert!(has("credential.helper", "") && has("credential.helper", "osxkeychain"));
        assert!(
            !guard.config.iter().any(|(k, _)| k == "user.name"),
            "plain settings stay"
        );
    }

    #[test]
    fn what_the_user_set_is_left_alone() {
        let guard = guard_from(
            &listing(&[
                ("global", "core.sshcommand", "ssh"),
                ("system", "filter.lfs.clean", "git-lfs clean -- %f"),
            ]),
            Path::new("/tmp"),
        )
        .unwrap();
        assert_eq!(guard, Guard::default());
    }

    #[test]
    fn an_external_diff_from_the_repository_stops_the_call() {
        assert!(guard_from(
            &listing(&[("local", "diff.external", "./x")]),
            Path::new("/tmp")
        )
        .is_err());
        assert!(guard_from(
            &listing(&[("local", "diff.x.command", "./x")]),
            Path::new("/tmp")
        )
        .is_err());
        // The user's own is theirs.
        let users = listing(&[
            ("global", "diff.external", "difft"),
            ("local", "diff.external", "./x"),
        ]);
        let guard = guard_from(&users, Path::new("/tmp")).unwrap();
        assert_eq!(
            guard.config,
            [("diff.external".to_owned(), "difft".to_owned())]
        );
    }

    #[test]
    fn a_name_that_cannot_be_said_stops_the_call() {
        assert!(guard_from(
            &listing(&[("local", "filter.a=b.clean", "x")]),
            Path::new("/tmp")
        )
        .is_err());
    }

    /// A real repository whose configuration runs a program on `git status`
    /// (fsmonitor) and on every commit (a hook): neither runs through here.
    #[cfg(unix)]
    #[test]
    fn a_hostile_repository_runs_nothing_on_status() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        let run = |args: &[&str]| {
            assert!(Command::new("git")
                .arg("-C")
                .arg(repo)
                .args(args)
                .output()
                .unwrap()
                .status
                .success());
        };
        run(&["init", "-q"]);
        let marker = repo.join("pwned");
        let script = repo.join("evil.sh");
        std::fs::write(
            &script,
            format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        run(&["config", "core.fsmonitor", &script.to_string_lossy()]);
        std::fs::create_dir_all(repo.join(".git/hooks")).unwrap();
        let hook = repo.join(".git/hooks/post-index-change");
        std::fs::copy(&script, &hook).unwrap();
        let out = command(repo, Some("status"), Hooks::Off)
            .unwrap()
            .arg("-C")
            .arg(repo)
            .args(["status", "--porcelain"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!marker.exists(), "fsmonitor or a hook ran");
        // Plain Git would have run it.
        let _ = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["status"])
            .output();
        assert!(marker.exists(), "the test's trap works");
    }
}
