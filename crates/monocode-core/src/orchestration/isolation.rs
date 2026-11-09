//! Each worker of a lead-supervised run edits its own detached worktree
//! (`createOrchestrationWorktree`, `session_checkpoint_apply`). The worktree
//! starts from a snapshot of the lead's checkout, uncommitted and untracked
//! files included, so a worker sees what the lead sees. Accepting the worker
//! applies only the difference from that snapshot back to the lead's checkout,
//! and refuses it when the worker wrote outside its scopes. Synchronous: run
//! these on a background executor.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Where a worker works, and the snapshot it started from.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Worker {
    pub checkout: String,
    /// Commit of the lead's files when the worker started.
    pub base: String,
}

fn git(cwd: &Path, index: Option<&Path>, args: &[&str]) -> Result<String, String> {
    if crate::files::remote::is_remote(cwd) {
        return Err("Worker worktrees are not available on another machine.".into());
    }
    let mut command =
        crate::git::safe::command(cwd, args.first().copied(), crate::git::safe::Hooks::Off)?;
    command.current_dir(cwd).args(args);
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    let out = command
        .output()
        .map_err(|e| format!("Could not run Git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    String::from_utf8(out.stdout).map_err(|e| e.to_string())
}

/// Whether workers can get their own worktree: a local Git checkout with a
/// first commit. Otherwise they share the lead's checkout.
pub fn available(checkout: &Path) -> bool {
    !crate::files::remote::is_remote(checkout)
        && git(
            checkout,
            None,
            &["rev-parse", "--verify", "--quiet", "HEAD"],
        )
        .is_ok()
}

/// The checkout's files as a tree, through a private index: `git add -A`
/// without touching the user's staging area.
fn tree(checkout: &Path) -> Result<String, String> {
    let scratch = tempfile::tempdir().map_err(|e| e.to_string())?;
    let index = scratch.path().join("index");
    git(checkout, Some(&index), &["read-tree", "HEAD"])?;
    git(checkout, Some(&index), &["add", "-A", "--", ":/"])?;
    Ok(git(checkout, Some(&index), &["write-tree"])?
        .trim()
        .to_owned())
}

/// A commit of the lead's current files, parented on its HEAD.
pub fn snapshot(checkout: &Path) -> Result<String, String> {
    let tree = tree(checkout)?;
    Ok(git(
        checkout,
        None,
        &[
            "-c",
            "user.name=MonoBOT",
            "-c",
            "user.email=orchestration@monobot.local",
            "commit-tree",
            &tree,
            "-p",
            "HEAD",
            "-m",
            "MonoBOT worker base",
        ],
    )?
    .trim()
    .to_owned())
}

/// Where the project sits inside its repository, as `sub/dir/` or empty.
fn prefix(checkout: &Path) -> Result<String, String> {
    Ok(git(checkout, None, &["rev-parse", "--show-prefix"])?
        .trim()
        .to_owned())
}

/// Repository-relative names the worker changed, split into project-relative
/// ones and those outside the project.
fn changes(worker: &Worker, now: &str) -> Result<(Vec<String>, Vec<String>), String> {
    let checkout = Path::new(&worker.checkout);
    let prefix = prefix(checkout)?;
    let names = git(
        checkout,
        None,
        &[
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            &format!("{}^{{tree}}", worker.base),
            now,
        ],
    )?;
    let mut inside = Vec::new();
    let mut outside = Vec::new();
    for name in names.split('\0').filter(|s| !s.is_empty()) {
        match name.strip_prefix(&prefix) {
            Some(relative) => inside.push(relative.to_owned()),
            None => outside.push(format!(":/{name}")),
        }
    }
    Ok((inside, outside))
}

fn root(lead: &Path) -> Result<PathBuf, String> {
    let main = crate::git::worktrees(lead)?
        .into_iter()
        .next()
        .ok_or("No working copies found")?;
    Ok(crate::worktree_management::default_root(Path::new(
        &main.path,
    )))
}

/// A detached worktree holding the lead's files as they are now.
pub fn create(lead: &Path, name: &str) -> Result<Worker, String> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("Invalid worker name".into());
    }
    let base = snapshot(lead)?;
    let parent = root(lead)?;
    std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    let path = parent.join(format!("mc-worker-{name}"));
    if path.exists() {
        return Err(format!(
            "A worker worktree already exists at {}",
            path.display()
        ));
    }
    git(
        lead,
        None,
        &[
            "worktree",
            "add",
            "--detach",
            "--",
            &path.to_string_lossy(),
            &base,
        ],
    )?;
    // A project opened at a subfolder works in the same subfolder.
    let checkout = path
        .canonicalize()
        .unwrap_or(path)
        .join(prefix(lead)?)
        .to_string_lossy()
        .trim_end_matches('/')
        .to_owned();
    Ok(Worker { checkout, base })
}

/// Whether `path` falls under one of the project-relative `scopes`.
pub fn in_scope(path: &str, scopes: &[String]) -> bool {
    let path = path.trim_start_matches("./");
    scopes.iter().any(|scope| {
        let scope = scope.trim_start_matches("./").trim_end_matches('/');
        scope == "." || scope.is_empty() || path == scope || {
            path.len() > scope.len()
                && path.starts_with(scope)
                && path.as_bytes()[scope.len()] == b'/'
        }
    })
}

/// Files the worker changed since it started, project-relative; files
/// outside the project are listed as `:/path`.
pub fn changed(worker: &Worker) -> Result<Vec<String>, String> {
    let now = tree(Path::new(&worker.checkout))?;
    let (mut inside, outside) = changes(worker, &now)?;
    inside.extend(outside);
    Ok(inside)
}

/// Apply the worker's changes to the lead's checkout, only when every changed
/// file is in scope. Idempotent: a patch already in the lead counts as applied.
/// Returns the files it carried over.
pub fn integrate(worker: &Worker, lead: &Path, scopes: &[String]) -> Result<Vec<String>, String> {
    let checkout = Path::new(&worker.checkout);
    if !checkout.is_dir() {
        return Err("This worker's worktree is missing. Its changes were not applied.".into());
    }
    let now = tree(checkout)?;
    let base = format!("{}^{{tree}}", worker.base);
    let (files, mut outside) = changes(worker, &now)?;
    outside.extend(files.iter().filter(|f| !in_scope(f, scopes)).cloned());
    if !outside.is_empty() {
        return Err(format!(
            "The worker changed files outside its scope ({}): {}. Its worktree was kept; retry with corrected files or message it to undo those edits.",
            scopes.join(", "),
            outside.join(", ")
        ));
    }
    if files.is_empty() {
        return Ok(files);
    }
    let patch = git(
        checkout,
        None,
        &["diff", "--binary", "--no-renames", &base, &now],
    )?;
    let scratch = tempfile::tempdir().map_err(|e| e.to_string())?;
    let file = scratch.path().join("worker.patch");
    std::fs::write(&file, patch).map_err(|e| e.to_string())?;
    let file = file.to_string_lossy();
    // The patch names files from the repository's root.
    let top = PathBuf::from(git(lead, None, &["rev-parse", "--show-toplevel"])?.trim());
    let lead = top.as_path();
    if git(lead, None, &["apply", "--check", "--", &file]).is_err() {
        if git(lead, None, &["apply", "--reverse", "--check", "--", &file]).is_ok() {
            return Ok(files);
        }
        let detail = git(lead, None, &["apply", "--check", "--", &file])
            .err()
            .unwrap_or_default();
        return Err(format!(
            "The worker's changes no longer apply to the lead's checkout: {detail}. Its worktree was kept for manual review."
        ));
    }
    git(lead, None, &["apply", "--", &file])?;
    Ok(files)
}

/// Remove the worker's worktree. A missing one is pruned.
pub fn remove(lead: &Path, worker: &Worker) -> Result<(), String> {
    let path = Path::new(&worker.checkout);
    if !path.exists() {
        let _ = git(lead, None, &["worktree", "prune"]);
        return Ok(());
    }
    // The worker may work in a subfolder of its worktree.
    let top = git(path, None, &["rev-parse", "--show-toplevel"])
        .map(|top| top.trim().to_owned())
        .unwrap_or_else(|_| worker.checkout.clone());
    crate::provider::discard_worktree(Path::new(&top));
    git(lead, None, &["worktree", "remove", "--force", "--", &top]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project");
        std::fs::create_dir(&path).unwrap();
        let git = |args: &[&str]| super::git(&path, None, args).unwrap();
        git(&["init", "-b", "main"]);
        git(&["config", "user.email", "fixture@example.invalid"]);
        git(&["config", "user.name", "Fixture"]);
        std::fs::create_dir(path.join("src")).unwrap();
        std::fs::write(path.join("src/lib.rs"), "one\n").unwrap();
        std::fs::write(path.join("README"), "readme\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "Initial"]);
        dir
    }

    #[test]
    fn scopes_cover_descendants_only() {
        let scopes = vec!["src".to_owned()];
        assert!(in_scope("src", &scopes));
        assert!(in_scope("src/lib.rs", &scopes));
        assert!(!in_scope("src2/lib.rs", &scopes));
        assert!(in_scope("anything", &[".".to_owned()]));
        assert!(in_scope("src/a.rs", &["./src/".to_owned()]));
    }

    #[test]
    fn workers_start_from_uncommitted_files_and_apply_only_their_changes() {
        let dir = repo();
        let lead = dir.path().join("project");
        assert!(available(&lead));
        // Uncommitted and untracked lead files reach the worker.
        std::fs::write(lead.join("src/lib.rs"), "one\ntwo\n").unwrap();
        std::fs::write(lead.join("notes.txt"), "draft\n").unwrap();
        let worker = create(&lead, "w1").unwrap();
        let checkout = Path::new(&worker.checkout);
        assert_eq!(
            std::fs::read_to_string(checkout.join("src/lib.rs")).unwrap(),
            "one\ntwo\n"
        );
        assert!(checkout.join("notes.txt").exists());
        // The user's staging area is untouched.
        assert_eq!(
            super::git(&lead, None, &["diff", "--cached", "--name-only"]).unwrap(),
            ""
        );
        assert!(changed(&worker).unwrap().is_empty());

        std::fs::write(checkout.join("src/lib.rs"), "one\ntwo\nthree\n").unwrap();
        std::fs::write(checkout.join("src/new.rs"), "new\n").unwrap();
        assert_eq!(changed(&worker).unwrap(), ["src/lib.rs", "src/new.rs"]);
        // The lead keeps editing elsewhere meanwhile.
        std::fs::write(lead.join("README"), "edited\n").unwrap();

        let files = integrate(&worker, &lead, &["src".into()]).unwrap();
        assert_eq!(files, ["src/lib.rs", "src/new.rs"]);
        assert_eq!(
            std::fs::read_to_string(lead.join("src/lib.rs")).unwrap(),
            "one\ntwo\nthree\n"
        );
        assert_eq!(
            std::fs::read_to_string(lead.join("README")).unwrap(),
            "edited\n"
        );
        // Applying again is a no-op rather than an error.
        assert_eq!(integrate(&worker, &lead, &["src".into()]).unwrap().len(), 2);

        remove(&lead, &worker).unwrap();
        assert!(!checkout.exists());
    }

    #[test]
    fn a_project_in_a_subfolder_works_and_applies_there() {
        let dir = repo();
        let root = dir.path().join("project");
        std::fs::create_dir_all(root.join("app/src")).unwrap();
        std::fs::write(root.join("app/src/main.rs"), "main\n").unwrap();
        super::git(&root, None, &["add", "."]).unwrap();
        super::git(&root, None, &["commit", "-q", "-m", "App"]).unwrap();
        let lead = root.join("app");
        let worker = create(&lead, "sub").unwrap();
        let checkout = Path::new(&worker.checkout);
        assert!(checkout.ends_with("app"), "{}", worker.checkout);
        std::fs::write(checkout.join("src/main.rs"), "edited\n").unwrap();
        assert_eq!(changed(&worker).unwrap(), ["src/main.rs"]);
        assert_eq!(
            integrate(&worker, &lead, &["src".into()]).unwrap(),
            ["src/main.rs"]
        );
        assert_eq!(
            std::fs::read_to_string(lead.join("src/main.rs")).unwrap(),
            "edited\n"
        );
        // "." covers the project, not the rest of the repository.
        std::fs::write(checkout.join("../README"), "outside\n").unwrap();
        let error = integrate(&worker, &lead, &[".".into()]).unwrap_err();
        assert!(error.contains(":/README"), "{error}");
        remove(&lead, &worker).unwrap();
        assert!(
            !checkout.parent().unwrap().exists(),
            "the whole worktree goes"
        );
    }

    #[test]
    fn writes_outside_the_scope_are_refused_and_the_worktree_kept() {
        let dir = repo();
        let lead = dir.path().join("project");
        let worker = create(&lead, "w2").unwrap();
        let checkout = Path::new(&worker.checkout);
        std::fs::write(checkout.join("README"), "worker\n").unwrap();
        std::fs::write(checkout.join("src/lib.rs"), "worker\n").unwrap();
        let error = integrate(&worker, &lead, &["src".into()]).unwrap_err();
        assert!(error.contains("outside its scope"), "{error}");
        assert!(error.contains("README"), "{error}");
        assert_eq!(
            std::fs::read_to_string(lead.join("src/lib.rs")).unwrap(),
            "one\n"
        );
        assert!(checkout.exists());
        // A conflicting lead edit is reported, not merged.
        std::fs::write(checkout.join("README"), "readme\n").unwrap();
        std::fs::write(lead.join("src/lib.rs"), "lead\n").unwrap();
        let error = integrate(&worker, &lead, &["src".into()]).unwrap_err();
        assert!(error.contains("no longer apply"), "{error}");
        remove(&lead, &worker).unwrap();
        assert!(create(&lead, "../bad").is_err());
    }
}
