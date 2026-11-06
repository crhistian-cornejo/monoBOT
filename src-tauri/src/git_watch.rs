//! Watches open checkouts so the Git panel reads status again only when
//! something changed, instead of spawning `git diff` every two seconds.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{mpsc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

const CHANGED_EVENT: &str = "git-worktree-changed";
/// Saves, checkouts and builds arrive as bursts; one read per burst.
const DEBOUNCE: Duration = Duration::from_millis(300);
/// Paths checked against .gitignore per burst. Past this, assume a change.
const MAX_CHECKED: usize = 256;
/// Build output that is ignored almost everywhere, skipped without asking git.
const SKIPPED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    ".next",
    ".turbo",
    "__pycache__",
    ".venv",
];

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct WorktreeChanged {
    cwd: String,
}

struct Watch {
    _watcher: RecommendedWatcher,
    users: usize,
}

#[derive(Default)]
pub struct GitWatchHost {
    watches: Mutex<HashMap<String, Watch>>,
}

/// Starts (or shares) a watch on `cwd`. False when this platform or folder
/// can't be watched, and the caller keeps polling.
#[tauri::command(async)]
pub fn git_watch_start(app: AppHandle, host: State<'_, GitWatchHost>, cwd: String) -> bool {
    // inotify needs one watch per folder, too many for a large checkout.
    if cfg!(target_os = "linux") {
        return false;
    }
    let mut watches = host.watches.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(watch) = watches.get_mut(&cwd) {
        watch.users += 1;
        return true;
    }
    let root = crate::fs::expand_home(&cwd);
    let changed = cwd.clone();
    let notify_ui = move || {
        let _ = app.emit(
            CHANGED_EVENT,
            WorktreeChanged {
                cwd: changed.clone(),
            },
        );
    };
    let Some(watcher) = watch(root, notify_ui) else {
        return false;
    };
    watches.insert(
        cwd,
        Watch {
            _watcher: watcher,
            users: 1,
        },
    );
    true
}

#[tauri::command(async)]
pub fn git_watch_stop(host: State<'_, GitWatchHost>, cwd: String) {
    let mut watches = host.watches.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(watch) = watches.get_mut(&cwd) {
        watch.users = watch.users.saturating_sub(1);
        if watch.users == 0 {
            watches.remove(&cwd);
        }
    }
}

fn watch(root: PathBuf, on_change: impl Fn() + Send + 'static) -> Option<RecommendedWatcher> {
    let (tx, rx) = mpsc::channel::<PathBuf>();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        let Ok(event) = result else { return };
        if event.kind.is_access() {
            return;
        }
        for path in event.paths {
            let _ = tx.send(path);
        }
    })
    .ok()?;
    watcher.watch(&root, RecursiveMode::Recursive).ok()?;
    // Dropping the watcher drops `tx`, which ends this thread.
    thread::spawn(move || {
        while let Ok(first) = rx.recv() {
            let mut paths = vec![first];
            let deadline = Instant::now() + DEBOUNCE;
            while let Some(wait) = deadline.checked_duration_since(Instant::now()) {
                match rx.recv_timeout(wait) {
                    Ok(path) => paths.push(path),
                    Err(mpsc::RecvTimeoutError::Timeout) => break,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
            if status_may_have_changed(&root, &paths) {
                on_change();
            }
        }
    });
    Some(watcher)
}

enum PathKind {
    /// HEAD, the index or a ref moved: status changed.
    GitState,
    /// Git's own objects, logs and locks, or skipped build output.
    Noise,
    /// A working-tree file, relevant unless .gitignore covers it.
    File(String),
}

fn classify(root: &Path, path: &Path) -> PathKind {
    let Ok(relative) = path.strip_prefix(root) else {
        return PathKind::Noise;
    };
    let parts: Vec<&str> = relative
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect();
    match parts.as_slice() {
        [] => PathKind::Noise,
        [".git", "HEAD" | "index" | "packed-refs"] => PathKind::GitState,
        [".git", "refs", ..] => PathKind::GitState,
        [".git", ..] => PathKind::Noise,
        _ if parts.iter().any(|part| SKIPPED_DIRS.contains(part)) => PathKind::Noise,
        _ => PathKind::File(parts.join("/")),
    }
}

fn status_may_have_changed(root: &Path, paths: &[PathBuf]) -> bool {
    let mut files = HashSet::new();
    for path in paths {
        match classify(root, path) {
            PathKind::GitState => return true,
            PathKind::Noise => {}
            PathKind::File(name) => {
                files.insert(name);
            }
        }
    }
    if files.is_empty() {
        return false;
    }
    if files.len() > MAX_CHECKED {
        return true;
    }
    let names: Vec<&str> = files.iter().map(String::as_str).collect();
    match crate::fs::git_ignored_names(root, &names) {
        Some(ignored) => files.iter().any(|name| !ignored.contains(name)),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(path: &str) -> &'static str {
        let root = Path::new("/repo");
        match classify(root, &root.join(path)) {
            PathKind::GitState => "git",
            PathKind::Noise => "noise",
            PathKind::File(_) => "file",
        }
    }

    #[test]
    fn git_state_files_count_and_git_internals_do_not() {
        assert_eq!(kind(".git/HEAD"), "git");
        assert_eq!(kind(".git/index"), "git");
        assert_eq!(kind(".git/refs/heads/main"), "git");
        assert_eq!(kind(".git/index.lock"), "noise");
        assert_eq!(kind(".git/objects/ab/cdef"), "noise");
        assert_eq!(kind(".git/logs/HEAD"), "noise");
    }

    #[test]
    fn build_output_is_skipped_and_sources_are_checked() {
        assert_eq!(kind("node_modules/react/index.js"), "noise");
        assert_eq!(kind("src-tauri/target/debug/app"), "noise");
        assert_eq!(kind("src/main.ts"), "file");
        assert_eq!(kind("README.md"), "file");
    }

    #[test]
    fn paths_outside_the_checkout_are_ignored() {
        let root = Path::new("/repo");
        assert!(matches!(
            classify(root, Path::new("/other/file")),
            PathKind::Noise
        ));
        assert!(matches!(classify(root, root), PathKind::Noise));
    }

    #[test]
    fn a_saved_file_is_reported_once_per_burst() {
        let dir = std::env::temp_dir().join(format!("git-watch-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let (tx, rx) = mpsc::channel();
        let watcher = watch(dir.clone(), move || {
            let _ = tx.send(());
        })
        .expect("watch starts");
        // Let the platform watcher arm before the write.
        thread::sleep(Duration::from_millis(200));
        std::fs::write(dir.join("src").join("main.ts"), "one").unwrap();
        std::fs::write(dir.join("src").join("main.ts"), "two").unwrap();
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok());
        assert!(rx.recv_timeout(DEBOUNCE * 2).is_err());
        drop(watcher);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_path_uses_forward_slashes() {
        let root = Path::new("/repo");
        match classify(root, &root.join("src").join("main.ts")) {
            PathKind::File(name) => assert_eq!(name, "src/main.ts"),
            _ => panic!("expected a file"),
        }
    }
}
