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

/// One `control` call from a worker's own session.
fn worker_call(
    shell: &mut Shell,
    session: i64,
    action: &str,
    request: &str,
    input: Value,
    cx: &mut Context<Shell>,
) -> mpsc::Receiver<Result<Value, String>> {
    call(shell, session, action, request, input, cx)
}

#[gpui::test]
fn the_board_follows_the_run_and_workers_post_on_it(cx: &mut TestAppContext) {
    let (window, lead, dir) = shell(cx);
    let repo = dir.path().join("project").canonicalize().unwrap();
    let repo = repo.to_string_lossy().into_owned();
    window
        .update(cx, |shell, window, cx| {
            hold_lead(shell, lead, cx);
            shell.ensure_right_panel(window, cx);
            shell
                .orchestrations
                .insert(lead, Live::new(run(lead, &repo, false, &["src", "README"])));
            shell.pump_orchestration(lead, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let (worker, replies) = window
        .update(cx, |shell, _, cx| {
            let live = &shell.orchestrations[&lead];
            let handles: Vec<_> = live
                .board
                .members
                .iter()
                .map(|m| m.handle.clone())
                .collect();
            assert_eq!(handles, ["coordinator", "worker-1", "worker-2"]);
            // Both were assigned; the fixture provider failed both turns.
            let kinds: Vec<_> = live.board.posts.iter().map(|p| p.kind).collect();
            assert_eq!(
                kinds.iter().filter(|k| **k == board::Kind::Assign).count(),
                2
            );
            assert_eq!(kinds.iter().filter(|k| **k == board::Kind::Fail).count(), 2);
            let worker = live.run.tasks[0].session.unwrap();
            let post = worker_call(
                shell,
                worker,
                "post",
                "p1",
                json!({"text": "@worker-2 the `README` is mine"}),
                cx,
            );
            let again = worker_call(
                shell,
                worker,
                "post",
                "p1",
                json!({"text": "@worker-2 the `README` is mine"}),
                cx,
            );
            let steer = worker_call(shell, worker, "delegate", "d0", json!({}), cx);
            let unknown = call(shell, lead, "post", "p2", json!({"text": "@ghost hi"}), cx);
            let read = worker_call(shell, worker, "board", "b1", json!({"since": 0}), cx);
            let delegated = call(
                shell,
                lead,
                "delegate",
                "d1",
                json!({"title": "Verify", "harness": "fixture", "prompt": "Run the tests",
                       "files": ["tests"], "handle": "Verifier"}),
                cx,
            );
            (worker, (post, again, steer, unknown, read, delegated))
        })
        .unwrap();
    cx.run_until_parked();
    let (post, again, steer, unknown, read, delegated) = replies;
    let post = post.try_recv().unwrap().unwrap();
    assert_eq!(post["to"], json!(["worker-2"]));
    assert_eq!(
        again.try_recv().unwrap().unwrap(),
        post,
        "a retried post lands once"
    );
    assert!(
        steer
            .try_recv()
            .unwrap()
            .unwrap_err()
            .contains("only use post and board")
    );
    assert!(unknown.try_recv().unwrap().unwrap_err().contains("@ghost"));
    let read = read.try_recv().unwrap().unwrap();
    assert!(
        read["posts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["from"] == "worker-1")
    );
    assert_eq!(delegated.try_recv().unwrap().unwrap()["handle"], "verifier");
    window
        .update(cx, |shell, _, cx| {
            let live = &shell.orchestrations[&lead];
            assert_eq!(
                live.board
                    .posts
                    .iter()
                    .filter(|p| p.kind == board::Kind::Message)
                    .count(),
                1
            );
            // worker-2 stopped: the post waits for its next turn.
            assert!(live.board.pending("worker-2"));
            // The Swarm tab shows the lead's board, and a worker's too.
            let panel = shell.right_panel.clone().unwrap();
            assert!(panel.read(cx).swarm_tab());
            let swarm = panel.read(cx).swarm.clone();
            assert_eq!(swarm.read(cx).lead(), Some(lead));
            assert_eq!(swarm.read(cx).board().posts.len(), live.board.posts.len());
            assert_eq!(shell.swarm_lead(worker), Some(lead));
        })
        .unwrap();
    // The board is saved under the lead.
    let saved = window
        .update(cx, |shell, _, _| {
            locked(&shell.store)
                .unwrap()
                .setting(&board::setting_key(lead))
                .unwrap()
        })
        .unwrap()
        .expect("a saved board");
    let saved: Board = serde_json::from_str(&saved).unwrap();
    assert_eq!(saved.members.len(), 4);
}

#[gpui::test]
fn swarm_opens_a_run_without_a_card_and_shows_its_tab(cx: &mut TestAppContext) {
    let (window, lead, _dir) = shell(cx);
    window
        .update(cx, |shell, window, cx| {
            shell.ensure_right_panel(window, cx);
            let message = monocode_core::follow_up::Message {
                id: "m1".into(),
                text: "/swarm Migrate auth".into(),
                ..Default::default()
            };
            shell.start_swarm(lead, "Migrate auth".into(), Vec::new(), message, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |shell, _, cx| {
            let live = shell.orchestrations.get(&lead).expect("a run");
            assert!(live.run.swarm);
            assert_eq!(live.board.title, "Migrate auth");
            let panel = shell.right_panel.clone().unwrap();
            assert!(panel.read(cx).swarm_tab());
            assert_eq!(panel.read(cx).tab(), crate::right_panel::RightTab::Swarm);
            // A second /swarm on the same chat is refused.
            let message = monocode_core::follow_up::Message {
                text: "/swarm Again".into(),
                ..Default::default()
            };
            shell.start_swarm(lead, "Again".into(), Vec::new(), message, cx);
            // It goes back to the composer.
            let view = shell.sessions[&lead].read(cx);
            assert_eq!(view.input.read(cx).content, "/swarm Again");
        })
        .unwrap();
}

#[gpui::test]
fn classic_shows_the_swarm_as_a_sidebar_tab_until_its_board_goes(cx: &mut TestAppContext) {
    let (window, lead, _dir) = shell(cx);
    window
        .update(cx, |shell, window, cx| {
            shell.workspace.classic_layout = true;
            shell.ensure_right_panel(window, cx);
            let message = monocode_core::follow_up::Message {
                id: "m1".into(),
                text: "/swarm Migrate auth".into(),
                ..Default::default()
            };
            shell.start_swarm(lead, "Migrate auth".into(), Vec::new(), message, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |shell, _, cx| {
            assert!(shell.classic_swarm_available(cx));
            assert_eq!(
                shell.classic_sidebar_tab,
                classic_sidebar::ClassicSidebarTab::Swarm,
                "a new swarm opens its tab"
            );
            // The tab is never saved as the project's tab.
            assert_ne!(
                shell
                    .workspace
                    .classic_sidebar_tabs
                    .get(&shell.workspace.project)
                    .map(String::as_str),
                Some("swarm")
            );
            shell.orchestrations.remove(&lead);
            shell.orchestration_summaries.remove(&lead);
            shell.orchestration_control.boards.remove(&lead);
            shell.sync_swarm(cx);
            assert!(!shell.classic_swarm_available(cx));
            assert_eq!(
                shell.classic_sidebar_tab,
                classic_sidebar::ClassicSidebarTab::Sessions
            );
        })
        .unwrap();
}

#[gpui::test]
fn an_agent_starts_a_swarm_only_when_the_user_lets_it(cx: &mut TestAppContext) {
    let (window, lead, _dir) = shell(cx);
    let refused = window
        .update(cx, |shell, window, cx| {
            shell.ensure_right_panel(window, cx);
            call(shell, lead, "swarm", "s1", json!({"title": "Big task"}), cx)
        })
        .unwrap();
    let error = refused.try_recv().unwrap().unwrap_err();
    assert!(error.contains("Settings > Chat"), "{error}");
    cx.update(|cx| {
        cx.set_global(crate::settings::Preferences {
            swarm_offer: true,
            ..Default::default()
        })
    });
    let missing = window
        .update(cx, |shell, _, cx| {
            call(shell, lead, "swarm", "s2", json!({}), cx)
        })
        .unwrap();
    assert!(missing.try_recv().unwrap().unwrap_err().contains("title"));
    let started = window
        .update(cx, |shell, _, cx| {
            call(shell, lead, "swarm", "s3", json!({"title": "Big task"}), cx)
        })
        .unwrap();
    cx.run_until_parked();
    let value = started.try_recv().unwrap().unwrap();
    assert_eq!(value["handle"], "coordinator");
    assert!(
        value["instructions"]
            .as_str()
            .unwrap()
            .contains("control --help")
    );
    window
        .update(cx, |shell, _, cx| {
            let live = shell.orchestrations.get(&lead).expect("a run");
            assert!(live.run.swarm && live.lead_busy);
            assert_eq!(live.board.title, "Big task");
            assert!(shell.right_panel.clone().unwrap().read(cx).swarm_tab());
            // Asked again, it answers with the swarm it already opened.
            let again = call(shell, lead, "swarm", "s4", json!({"title": "Other"}), cx);
            assert_eq!(again.try_recv().unwrap().unwrap()["title"], "Big task");
        })
        .unwrap();
}

#[gpui::test]
fn a_role_pinned_to_a_harness_the_run_lacks_sends_swarm_back(cx: &mut TestAppContext) {
    let (window, lead, _dir) = shell(cx);
    window
        .update(cx, |shell, window, cx| {
            shell.ensure_right_panel(window, cx);
            let message = monocode_core::follow_up::Message {
                id: "m1".into(),
                text: "/swarm @verifier=nowhere Migrate auth".into(),
                ..Default::default()
            };
            let roles = vec![control::Role {
                handle: "verifier".into(),
                provider: "nowhere".into(),
                model: None,
            }];
            shell.start_swarm(lead, "Migrate auth".into(), roles, message, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |shell, _, cx| {
            assert!(!shell.orchestrations.contains_key(&lead));
            let view = shell.sessions[&lead].read(cx);
            assert_eq!(
                view.input.read(cx).content,
                "/swarm @verifier=nowhere Migrate auth"
            );
            assert!(
                view.status_line().contains("nowhere"),
                "{}",
                view.status_line()
            );
        })
        .unwrap();
}

#[gpui::test]
fn a_chat_hands_a_large_task_to_chief_whose_monos_report_back_once(cx: &mut TestAppContext) {
    let (window, requester, _dir) = shell(cx);
    cx.update(|cx| cx.set_global(crate::settings::Preferences::default()));
    let missing = window
        .update(cx, |shell, window, cx| {
            shell.ensure_right_panel(window, cx);
            call(
                shell,
                requester,
                "chief",
                "c1",
                json!({"title": "Auth"}),
                cx,
            )
        })
        .unwrap();
    assert!(missing.try_recv().unwrap().unwrap_err().contains("brief"));
    let started = window
        .update(cx, |shell, _, cx| {
            call(
                shell,
                requester,
                "chief",
                "c2",
                json!({"title": "Auth", "brief": "Rotate the keys in src"}),
                cx,
            )
        })
        .unwrap();
    cx.run_until_parked();
    let value = started.try_recv().unwrap().unwrap();
    let chief = value["chief"].as_i64().unwrap();
    assert!(
        value["instructions"]
            .as_str()
            .unwrap()
            .contains("End your turn")
    );
    window
        .update(cx, |shell, _, cx| {
            let live = &shell.orchestrations[&chief];
            assert_eq!(live.run.requester, Some(requester));
            assert_eq!(live.run.request, "Rotate the keys in src");
            assert_eq!(shell.started_by[&chief], requester);
            assert!(
                shell.sessions[&chief]
                    .read(cx)
                    .summary
                    .title
                    .starts_with("Chief · ")
            );
            let monos = shell.monos.as_ref().unwrap().read(cx);
            assert!(
                monos
                    .roster
                    .monos
                    .iter()
                    .any(monocode_core::mono::crew::is_chief),
                "Chief stays as one of the user's agents"
            );
            // The chat that asked shows Chief's board.
            assert_eq!(shell.swarm_lead(requester), Some(chief));
            // Asked again, it answers with the run it opened.
            let again = call(
                shell,
                requester,
                "chief",
                "c3",
                json!({"title": "Auth", "brief": "Again"}),
                cx,
            );
            assert_eq!(again.try_recv().unwrap().unwrap()["chief"], chief);
            // The fixture harness ends Chief's first turn at once; keep the
            // run going and Chief idle under a draft, as a real turn would.
            let live = shell.orchestrations.get_mut(&chief).unwrap();
            live.run.status = RunStatus::Active;
            live.run.error = None;
            live.lead_busy = false;
            hold_lead(shell, chief, cx);
        })
        .unwrap();
    let delegated = window
        .update(cx, |shell, _, cx| {
            call(
                shell,
                chief,
                "delegate",
                "d1",
                json!({"title": "Keys", "prompt": "Rotate them", "files": ["src"], "handle": "tester"}),
                cx,
            )
        })
        .unwrap();
    cx.run_until_parked();
    let task = delegated.try_recv().unwrap().unwrap()["taskId"]
        .as_str()
        .unwrap()
        .to_owned();
    window
        .update(cx, |shell, _, cx| {
            let live = &shell.orchestrations[&chief];
            let mono = live.run.task(&task).unwrap().mono.clone().expect("a mono");
            assert_eq!(live.new_monos, 1);
            assert!(
                live.run
                    .crew
                    .iter()
                    .any(|m| m.id == mono && m.name == "Tester")
            );
            let monos = shell.monos.as_ref().unwrap().read(cx);
            let made = monos.roster.monos.iter().find(|m| m.id == mono).unwrap();
            assert!(monocode_core::mono::crew::is_coder(made));
            let worker = live.run.task(&task).unwrap().session.expect("a session");
            assert!(
                shell.sessions[&worker]
                    .read(cx)
                    .summary
                    .title
                    .starts_with("Tester · ")
            );
        })
        .unwrap();
    // Chief drops the task and finishes: the chat that asked gets one report.
    let finished = window
        .update(cx, |shell, _, cx| {
            call(shell, chief, "cancel", "x1", json!({"taskId": task}), cx);
            call(
                shell,
                chief,
                "finish",
                "f1",
                json!({"summary": "Nothing changed; the keys task was dropped."}),
                cx,
            )
        })
        .unwrap();
    cx.run_until_parked();
    assert!(finished.try_recv().unwrap().is_ok());
    window
        .update(cx, |shell, _, _| {
            assert!(!shell.orchestrations.contains_key(&chief));
            assert!(
                !shell.chief_reports.contains_key(&requester),
                "the idle chat that asked got the report at once"
            );
        })
        .unwrap();
}

#[gpui::test]
fn a_swarm_card_opens_its_board_before_any_agent_and_after_the_panel_closed(
    cx: &mut TestAppContext,
) {
    let (window, lead, _dir) = shell(cx);
    window
        .update(cx, |shell, window, cx| {
            shell.ensure_right_panel(window, cx);
            let message = monocode_core::follow_up::Message {
                id: "m1".into(),
                text: "/swarm Migrate auth".into(),
                ..Default::default()
            };
            shell.start_swarm(lead, "Migrate auth".into(), Vec::new(), message, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
    visual.refresh().unwrap();
    window
        .update(visual, |shell, window, cx| {
            assert!(
                shell.workspace.right_panel_open,
                "a new swarm opens the panel"
            );
            assert!(
                shell.sessions[&lead].read(cx).orchestration_is_swarm(),
                "the card says Swarm, not Orchestrator"
            );
            // The user closes the panel and moves to another tab.
            shell.set_right_panel_open(false, window, cx);
            let panel = shell.right_panel.clone().unwrap();
            panel.update(cx, |p, cx| {
                p.select(crate::right_panel::RightTab::Files, cx)
            });
            // No agent yet: the card's button still shows the board.
            shell.sessions[&lead].update(cx, |_, cx| {
                cx.emit(crate::conversation::composer_features::OpenOrchestrationAgents)
            });
        })
        .unwrap();
    visual.run_until_parked();
    visual.refresh().unwrap();
    window
        .update(visual, |shell, _, cx| {
            assert!(shell.workspace.right_panel_open);
            let panel = shell.right_panel.clone().unwrap();
            assert_eq!(panel.read(cx).tab(), crate::right_panel::RightTab::Swarm);
        })
        .unwrap();
}
