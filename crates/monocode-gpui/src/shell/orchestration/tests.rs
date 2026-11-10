use super::*;
use gpui::{TestAppContext, WindowHandle};
use monocode_core::{agent::Approval, orchestration::Task as PlanTask, store::Store};
use std::sync::{Arc, Mutex, mpsc};

/// A scratch folder removed with the test.
struct TempDir(std::path::PathBuf);
impl TempDir {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A lead in a real Git checkout. Its provider is unknown, so a turn fails
/// at once instead of starting an agent.
fn shell(cx: &mut TestAppContext) -> (WindowHandle<Shell>, i64, TempDir) {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = TempDir(std::env::temp_dir().join(format!(
        "monocode-orchestration-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )));
    let _ = std::fs::remove_dir_all(dir.path());
    let repo = dir.path().join("project");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .current_dir(&repo)
                .args(args)
                .status()
                .unwrap()
                .success()
        )
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "fixture@example.invalid"]);
    git(&["config", "user.name", "Fixture"]);
    std::fs::write(repo.join("README"), "readme\n").unwrap();
    std::fs::write(repo.join("src/lib.rs"), "one\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "Initial"]);
    let repo = repo.canonicalize().unwrap().to_string_lossy().into_owned();
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let mut lead = store.create("codex", &repo).unwrap();
    lead.provider = "fixture".into();
    let id = lead.id;
    let mut state = Workspace::default();
    state.add_project(&repo);
    state.open(id);
    let saved = serde_json::to_string(&state).unwrap();
    let store = Arc::new(Mutex::new(store));
    let window = cx.update(|cx| {
        crate::input::bind(cx);
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| {
                let mut shell = Shell::new(store, vec![lead], repo.clone(), Some(saved), cx);
                shell.ensure_session(id, cx);
                shell
            })
        })
        .unwrap()
    });
    cx.run_until_parked();
    (window, id, dir)
}

fn run(lead: i64, repo: &str, isolated: bool, files: &[&str]) -> Run {
    let plan = orchestration::Plan {
        title: "Work".into(),
        summary: String::new(),
        tasks: files
            .iter()
            .enumerate()
            .map(|(ix, file)| PlanTask {
                id: format!("task-{ix}"),
                title: format!("Assignment {ix}"),
                prompt: "Edit and report".into(),
                files: vec![(*file).into()],
                ..Default::default()
            })
            .collect(),
    };
    Run::from_plan(
        lead,
        repo,
        repo,
        isolated,
        "Improve the project",
        &plan,
        "fixture",
        None,
        vec![Choice {
            provider: "fixture".into(),
            models: Vec::new(),
        }],
        4,
    )
    .unwrap()
}

/// One `monocode-gpui control` call, as the bridge delivers it.
fn call(
    shell: &mut Shell,
    lead: i64,
    action: &str,
    request: &str,
    input: Value,
    cx: &mut Context<Shell>,
) -> mpsc::Receiver<Result<Value, String>> {
    let (reply, result) = mpsc::channel();
    shell.handle_control_call(
        Call {
            session: lead,
            request_id: request.into(),
            action: format!("control.{action}"),
            input,
            reply,
        },
        cx,
    );
    result
}

/// The lead keeps a draft, so it is never handed a continuation turn.
fn hold_lead(shell: &mut Shell, lead: i64, cx: &mut Context<Shell>) {
    shell.sessions[&lead].update(cx, |v, cx| {
        v.input
            .update(cx, |input, cx| input.set_text("Draft".into(), cx))
    });
}

#[gpui::test]
fn workers_run_in_their_own_worktree_and_review_applies_their_changes(cx: &mut TestAppContext) {
    let (window, lead, dir) = shell(cx);
    let repo = dir.path().join("project").canonicalize().unwrap();
    let repo_text = repo.to_string_lossy().into_owned();
    window
        .update(cx, |shell, _, cx| {
            hold_lead(shell, lead, cx);
            shell
                .orchestrations
                .insert(lead, Live::new(run(lead, &repo_text, true, &["src"])));
            shell.pump_orchestration(lead, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let (worker, checkout) = window
        .update(cx, |shell, _, cx| {
            let task = &shell.orchestrations[&lead].run.tasks[0];
            let worker = task.session.expect("a worker session");
            let checkout = task.workspace.clone().expect("its own worktree").checkout;
            assert_ne!(checkout, repo_text);
            let view = shell.sessions[&worker].read(cx);
            assert_eq!(view.managed_by(), Some(lead), "the lead manages it");
            assert_eq!(
                view.settings.worktree_cwd.as_deref(),
                Some(checkout.as_str())
            );
            // The fixture provider fails, so the turn settles as failed.
            assert_eq!(task.status, Status::Failed, "{:?}", task.error);
            assert!(shell.orchestration_worker(worker), "listed under the lead");
            (worker, checkout)
        })
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(std::path::Path::new(&checkout).join("README")).unwrap(),
        "readme\n"
    );
    let replies = window
        .update(cx, |shell, _, cx| {
            let get = call(shell, lead, "get", "g1", json!({"taskId":"t1"}), cx);
            let early = call(shell, lead, "review", "r0", json!({"taskId":"t1"}), cx);
            // The worker's next turn edits its scope; it completes.
            std::fs::write(std::path::Path::new(&checkout).join("src/new.rs"), "new\n").unwrap();
            let live = shell.orchestrations.get_mut(&lead).unwrap();
            live.run.tasks[0].status = Status::Completed;
            let review = call(shell, lead, "review", "r1", json!({"taskId":"t1"}), cx);
            (get, early, review)
        })
        .unwrap();
    cx.run_until_parked();
    let (get, early, review) = replies;
    let get = get.try_recv().unwrap().unwrap();
    assert_eq!(get["status"], "failed");
    assert_eq!(get["workspace"], checkout.as_str());
    assert!(
        early
            .try_recv()
            .unwrap()
            .unwrap_err()
            .contains("Only a completed result")
    );
    let accepted = review.try_recv().unwrap().unwrap();
    assert_eq!(accepted["accepted"], true);
    assert_eq!(accepted["files"], json!(["src/new.rs"]));
    assert_eq!(accepted["cleaned"], true);
    assert_eq!(
        std::fs::read_to_string(repo.join("src/new.rs")).unwrap(),
        "new\n"
    );
    assert!(
        !std::path::Path::new(&checkout).exists(),
        "its worktree is removed"
    );
    let (again, finish) = window
        .update(cx, |shell, _, cx| {
            let again = call(shell, lead, "review", "r1", json!({"taskId":"t1"}), cx);
            let finish = call(shell, lead, "finish", "f1", json!({}), cx);
            (again, finish)
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(
        again.try_recv().unwrap().unwrap(),
        accepted,
        "a retried ID repeats its result"
    );
    assert_eq!(finish.try_recv().unwrap().unwrap()["finished"], true);
    window
        .update(cx, |shell, _, cx| {
            assert!(!shell.orchestrations.contains_key(&lead), "the run ended");
            assert_eq!(
                shell.orchestration_summaries[&lead].tasks[0].status,
                "completed"
            );
            assert_eq!(shell.sessions[&worker].read(cx).managed_by(), None);
            let late = call(shell, lead, "list", "l1", json!({}), cx);
            assert!(
                late.try_recv()
                    .unwrap()
                    .unwrap_err()
                    .contains("No orchestration run")
            );
        })
        .unwrap();
}

#[gpui::test]
fn worker_approvals_go_to_the_lead_and_wait_wakes_when_a_worker_settles(cx: &mut TestAppContext) {
    let (window, lead, dir) = shell(cx);
    let repo = dir.path().join("project").canonicalize().unwrap();
    let repo = repo.to_string_lossy().into_owned();
    let (controls, list, blocked_wait) = window
        .update(cx, |shell, _, cx| {
            hold_lead(shell, lead, cx);
            // A worker whose turn is running and asks to run a command.
            let mut worker = locked(&shell.store)
                .unwrap()
                .create("codex", &repo)
                .unwrap();
            worker.provider = "fixture".into();
            let worker_id = worker.id;
            shell.add(worker, cx);
            let mut run = run(lead, &repo, false, &["src"]);
            run.schedule();
            run.prepared("t1", 1, worker_id, None);
            let mut live = Live::new(run);
            live.turns.insert("t1".into(), (1, true));
            shell.orchestrations.insert(lead, live);
            shell.sessions[&worker_id].update(cx, |v, cx| v.test_turn(true, "Working…", cx));
            let controls = shell.sessions[&worker_id].update(cx, |v, cx| {
                v.wait_on_approval(
                    Approval {
                        id: "approval-1".into(),
                        title: "Run cargo test".into(),
                        detail: "cargo test -p core".into(),
                        can_remember: false,
                    },
                    cx,
                )
            });
            let list = call(shell, lead, "list", "l1", json!({}), cx);
            let wait = call(shell, lead, "wait", "w1", json!({"timeoutSeconds": 20}), cx);
            (controls, list, wait)
        })
        .unwrap();
    cx.run_until_parked();
    let list = list.try_recv().unwrap().unwrap();
    let needs = &list["run"]["tasks"][0]["needsInput"];
    assert_eq!(needs["kind"], "approval");
    assert_eq!(needs["requestId"], "approval-1");
    assert_eq!(needs["detail"], "cargo test -p core");
    assert!(
        blocked_wait.try_recv().unwrap().is_ok(),
        "a worker waiting on the lead wakes wait at once"
    );
    let (respond, wait) = window
        .update(cx, |shell, _, cx| {
            let stale = call(
                shell,
                lead,
                "respond",
                "p0",
                json!({"taskId":"t1","requestId":"approval-0","decision":"allow"}),
                cx,
            );
            assert!(
                stale
                    .try_recv()
                    .unwrap()
                    .unwrap_err()
                    .contains("Stale requestId")
            );
            let respond = call(
                shell,
                lead,
                "respond",
                "p1",
                json!({"taskId":"t1","requestId":"approval-1","decision":"allow"}),
                cx,
            );
            let wait = call(shell, lead, "wait", "w2", json!({"timeoutSeconds": 20}), cx);
            (respond, wait)
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(respond.try_recv().unwrap().unwrap()["decision"], "allow");
    let decision = controls.try_recv().unwrap();
    assert_eq!(decision.id, "approval-1");
    assert!(matches!(
        decision.decision,
        monocode_core::agent::Decision::Allow
    ));
    assert!(
        wait.try_recv().is_err(),
        "wait blocks while the worker works"
    );
    // The worker's turn ends: wait returns with its result.
    window
        .update(cx, |shell, _, cx| {
            let worker = shell.orchestrations[&lead].run.tasks[0].session.unwrap();
            shell.sessions[&worker].update(cx, |v, cx| v.test_turn(false, "Ready", cx));
        })
        .unwrap();
    cx.run_until_parked();
    let woke = wait.try_recv().unwrap().unwrap();
    assert_eq!(woke["tasks"][0]["status"], "completed");
    window
        .update(cx, |shell, _, cx| {
            let steer = call(
                shell,
                lead,
                "steer",
                "s1",
                json!({"taskId":"t1","text":"x"}),
                cx,
            );
            assert!(
                steer
                    .try_recv()
                    .unwrap()
                    .unwrap_err()
                    .contains("Only a running agent")
            );
            let unknown = call(shell, lead, "list", "l2", json!({"verbose":true}), cx);
            assert!(
                unknown
                    .try_recv()
                    .unwrap()
                    .unwrap_err()
                    .contains("Unknown list field")
            );
            // Saved for a restart.
            cx.background_executor().run_until_parked();
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |shell, _, _| {
            let saved = locked(&shell.store).unwrap().orchestration_runs().unwrap();
            assert_eq!(saved[&lead].tasks[0].status, Status::Completed);
        })
        .unwrap();
}

#[gpui::test]
fn an_interrupted_run_comes_back_paused_and_resumes_its_worker_in_place(cx: &mut TestAppContext) {
    let (window, lead, dir) = shell(cx);
    let repo = dir.path().join("project").canonicalize().unwrap();
    let repo = repo.to_string_lossy().into_owned();
    window
        .update(cx, |shell, _, cx| {
            hold_lead(shell, lead, cx);
            let mut worker = locked(&shell.store)
                .unwrap()
                .create("codex", &repo)
                .unwrap();
            worker.provider = "fixture".into();
            let worker_id = worker.id;
            shell.add(worker, cx);
            let mut run = run(lead, &repo, false, &["src"]);
            run.schedule();
            run.prepared("t1", 1, worker_id, None);
            // Saved while active, then the app closed.
            let restored = restore(HashMap::from([(lead, run)]));
            shell.orchestrations.extend(restored);
            shell.publish_orchestration(lead, cx);
            let live = &shell.orchestrations[&lead];
            assert_eq!(live.run.status, RunStatus::Paused);
            assert_eq!(live.run.tasks[0].status, Status::Interrupted);
            let view = shell.sessions[&lead].read(cx);
            assert!(view.orchestration_paused(), "the card offers Resume");
            assert!(
                view.orchestration_progress
                    .as_deref()
                    .unwrap()
                    .contains("monoBOT closed")
            );
            let paused = call(
                shell,
                lead,
                "delegate",
                "d1",
                json!({"title":"x","harness":"fixture","prompt":"p","files":["docs"]}),
                cx,
            );
            assert!(paused.try_recv().unwrap().unwrap_err().contains("paused"));
            // Resume waits for the user's draft.
            shell.resume_orchestration(lead, cx);
            assert_eq!(shell.orchestrations[&lead].run.status, RunStatus::Paused);
            shell.error = None;
            shell.sessions[&lead].update(cx, |v, cx| {
                v.input
                    .update(cx, |input, cx| input.set_text(String::new(), cx))
            });
            shell.resume_orchestration(lead, cx);
            let task = &shell.orchestrations[&lead].run.tasks[0];
            assert_eq!(task.status, Status::Running);
            assert_eq!(task.session, Some(worker_id), "the same worker continues");
            assert_eq!(task.dispatch, 2);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |shell, _, _| {
            // The fixture lead's turn fails: an interrupted lead pauses the run.
            let live = &shell.orchestrations[&lead];
            assert_eq!(live.run.status, RunStatus::Paused, "{:?}", live.run.error);
            assert!(
                live.run
                    .error
                    .as_deref()
                    .unwrap()
                    .contains("lead was interrupted")
            );
        })
        .unwrap();
}
