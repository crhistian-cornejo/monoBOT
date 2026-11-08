use super::*;
use crate::orchestration::{Plan, Task as PlanTask};

fn plan(tasks: &[(&str, &str, &[&str])]) -> Plan {
    Plan {
        title: "Work".into(),
        summary: String::new(),
        tasks: tasks
            .iter()
            .map(|(id, file, deps)| PlanTask {
                id: (*id).into(),
                title: format!("Assignment {id}"),
                prompt: "Do it".into(),
                files: vec![(*file).into()],
                depends_on: deps.iter().map(|d| (*d).into()).collect(),
                ..Default::default()
            })
            .collect(),
    }
}
fn run(tasks: &[(&str, &str, &[&str])], workers: usize) -> Run {
    Run::from_plan(
        7,
        "/repo",
        "/repo",
        false,
        "Fix it",
        &plan(tasks),
        "claude",
        Some("sonnet"),
        vec![
            Choice {
                provider: "claude".into(),
                models: vec!["sonnet".into(), "opus".into()],
            },
            Choice {
                provider: "codex".into(),
                models: Vec::new(),
            },
        ],
        workers,
    )
    .unwrap()
}
fn none(_: &Task) -> Option<NeedsInput> {
    None
}

#[test]
fn approved_plans_map_ids_and_schedule_within_limits_scopes_and_reviews() {
    let mut run = run(
        &[
            ("a", "src", &[]),
            ("b", "src/main.rs", &[]),
            ("c", "tests", &[]),
            ("d", "docs", &["c"]),
        ],
        2,
    );
    assert_eq!(run.tasks[3].depends_on, [run.tasks[2].id.clone()]);
    assert_eq!(run.tasks[0].assignment.as_deref(), Some("a"));
    assert_eq!(run.tasks[0].model.as_deref(), Some("sonnet"));
    let started = run.schedule();
    assert_eq!(started, [run.tasks[0].id.clone(), run.tasks[2].id.clone()]);
    assert_eq!(
        run.waiting_for(&run.tasks[1]).unwrap(),
        "Waiting for files: Assignment a"
    );
    assert_eq!(
        run.waiting_for(&run.tasks[3]).unwrap(),
        "Waiting for review: Assignment c"
    );
    // A stale dispatch cannot settle a newer one.
    let c = run.tasks[2].id.clone();
    assert!(!run.settle(&c, 0, true, "old", None));
    assert!(run.settle(&c, 1, true, "done", None));
    assert!(run.schedule().is_empty(), "d waits for c's review");
    let (value, effect) = run.execute("review", &json!({"taskId": c}), &none).unwrap();
    assert_eq!(value["accepted"], true);
    assert_eq!(effect, Effect::None);
    assert_eq!(run.schedule(), [run.tasks[3].id.clone()]);
}

#[test]
fn actions_reject_unknown_fields_bad_harnesses_and_scope_escapes() {
    let mut run = run(&[("a", "src", &[])], 4);
    let error = run
        .execute(
            "delegate",
            &json!({"title":"x","harness":"claude","prompt":"p","files":["src"],"scope":"x"}),
            &none,
        )
        .unwrap_err();
    assert!(error.contains("Unknown delegate field: scope"), "{error}");
    let error = run
        .execute(
            "delegate",
            &json!({"title":"x","harness":"gemini","prompt":"p","files":["src"]}),
            &none,
        )
        .unwrap_err();
    assert!(error.contains("not allowed"), "{error}");
    let error = run
        .execute(
            "delegate",
            &json!({"title":"x","harness":"claude","model":"gpt","prompt":"p","files":["src"]}),
            &none,
        )
        .unwrap_err();
    assert!(error.contains("sonnet, opus"), "{error}");
    let error = run
        .execute(
            "delegate",
            &json!({"title":"x","harness":"claude","prompt":"p","files":["../out"]}),
            &none,
        )
        .unwrap_err();
    assert!(error.contains("project-relative"), "{error}");
    let error = run
        .execute("delegate", &json!({"title":"x","harness":"claude","prompt":"p","files":["src"],"dependsOn":["nope"]}), &none)
        .unwrap_err();
    assert!(error.contains("unknown or cancelled: nope"), "{error}");
    let (value, _) = run
        .execute(
            "delegate",
            &json!({"title":"x","harness":"codex","prompt":"p","files":["lib"]}),
            &none,
        )
        .unwrap();
    let task = run.task(value["taskId"].as_str().unwrap()).unwrap();
    assert_eq!(task.provider, "codex");
    assert_eq!(task.model, None);
    assert!(run
        .execute("bogus", &json!({}), &none)
        .unwrap_err()
        .contains("Unknown action"));
    assert!(run
        .execute("wait", &json!({"timeoutSeconds": 40}), &none)
        .is_err());
}

#[test]
fn steer_respond_and_answer_reach_the_right_worker_request() {
    let mut run = run(&[("a", "src", &[])], 4);
    let id = run.tasks[0].id.clone();
    let error = run
        .execute("steer", &json!({"taskId": id, "text": "focus"}), &none)
        .unwrap_err();
    assert!(error.contains("not started yet"), "{error}");
    run.schedule();
    assert!(run.prepared(&id, 1, 42, None));
    let (_, effect) = run
        .execute("steer", &json!({"taskId": id, "text": "focus"}), &none)
        .unwrap();
    assert_eq!(
        effect,
        Effect::Steer {
            session: 42,
            text: "focus".into()
        }
    );

    let approval = |_: &Task| {
        Some(NeedsInput {
            kind: "approval",
            request_id: "r1".into(),
            label: "Run tests".into(),
            ..Default::default()
        })
    };
    assert!(run
        .execute(
            "answer",
            &json!({"taskId": id, "requestId": "r1", "skip": true}),
            &approval
        )
        .is_err());
    assert!(run
        .execute(
            "respond",
            &json!({"taskId": id, "requestId": "r0", "decision": "allow"}),
            &approval
        )
        .unwrap_err()
        .contains("Stale requestId"));
    let (_, effect) = run
        .execute(
            "respond",
            &json!({"taskId": id, "requestId": "r1", "decision": "deny"}),
            &approval,
        )
        .unwrap();
    assert_eq!(
        effect,
        Effect::Respond {
            session: 42,
            request_id: "r1".into(),
            allow: false
        }
    );
    assert!(
        !run.waitable(&approval),
        "a blocked worker wakes wait at once"
    );
    assert!(run.waitable(&none));

    let question = |_: &Task| {
        Some(NeedsInput {
            kind: "question",
            request_id: "q1".into(),
            label: "Which?".into(),
            questions: vec![Question {
                id: "0".into(),
                prompt: "Which database?".into(),
                options: vec![
                    QuestionOption {
                        id: "0".into(),
                        label: "Postgres".into(),
                    },
                    QuestionOption {
                        id: "1".into(),
                        label: "SQLite".into(),
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        })
    };
    let (_, effect) = run
        .execute(
            "answer",
            &json!({"taskId": id, "requestId": "q1", "answers": {"0": ["SQLite"]}}),
            &question,
        )
        .unwrap();
    assert_eq!(
        effect,
        Effect::Answer {
            session: 42,
            request_id: "q1".into(),
            answers: Some(vec![vec!["SQLite".into()]])
        }
    );
    assert!(run
        .execute(
            "answer",
            &json!({"taskId": id, "requestId": "q1", "answers": {"0": ["MySQL"]}}),
            &question
        )
        .unwrap_err()
        .contains("Unknown option"));
}

#[test]
fn isolated_reviews_integrate_and_finish_needs_every_task_settled() {
    let mut run = run(&[("a", "src", &[]), ("b", "docs", &[])], 4);
    let (a, b) = (run.tasks[0].id.clone(), run.tasks[1].id.clone());
    run.schedule();
    let worker = Worker {
        checkout: "/repo-worktrees/mc-worker-a".into(),
        base: "abc".into(),
    };
    run.prepared(&a, 1, 1, Some(worker.clone()));
    run.prepared(&b, 1, 2, None);
    run.settle(&a, 1, true, "edited src", None);
    run.settle(&b, 1, false, "", Some("crashed".into()));
    let error = run.execute("finish", &json!({}), &none).unwrap_err();
    assert!(error.contains("Assignment a (completed)"), "{error}");
    let (_, effect) = run.execute("review", &json!({"taskId": a}), &none).unwrap();
    assert_eq!(effect, Effect::Integrate { task: a.clone() });
    assert!(run
        .execute("review", &json!({"taskId": b}), &none)
        .unwrap_err()
        .contains("failed"));
    run.accept(&a, vec!["src/lib.rs".into()]);
    // message re-queues the failed worker with its session kept.
    run.execute("message", &json!({"taskId": b, "text": "try again"}), &none)
        .unwrap();
    assert_eq!(run.task(&b).unwrap().status, Status::Queued);
    assert_eq!(run.task(&b).unwrap().session, Some(2));
    run.execute("cancel", &json!({"taskId": b}), &none).unwrap();
    let (value, effect) = run.execute("finish", &json!({}), &none).unwrap();
    assert_eq!(value["finished"], true);
    assert_eq!(
        effect,
        Effect::Cleanup {
            accepted: vec![worker],
            unreviewed: vec![]
        }
    );
    assert_eq!(run.status, RunStatus::Finished);
    let error = run
        .execute(
            "delegate",
            &json!({"title":"x","harness":"claude","prompt":"p","files":["src"]}),
            &none,
        )
        .unwrap_err();
    assert!(error.contains("finished"), "{error}");
    assert!(run.execute("list", &json!({}), &none).is_ok());
}

#[test]
fn interrupted_runs_pause_resume_from_saved_state_and_deliver_results_once() {
    let mut run = run(&[("a", "src", &[]), ("b", "docs", &[])], 4);
    let (a, b) = (run.tasks[0].id.clone(), run.tasks[1].id.clone());
    run.schedule();
    run.prepared(&a, 1, 1, None);
    run.prepared(&b, 1, 2, None);
    run.settle(&b, 1, true, "docs done", None);
    let prompt = run.continuation(&none).unwrap().unwrap();
    assert!(prompt.contains("docs done"), "{prompt}");
    assert!(run.continuation(&none).unwrap().is_none(), "delivered once");

    assert_eq!(run.pause("The app quit."), [1]);
    assert_eq!(run.status, RunStatus::Paused);
    assert!(run
        .execute("message", &json!({"taskId": b, "text": "x"}), &none)
        .unwrap_err()
        .contains("paused"));
    // Saved and loaded across a restart.
    let saved = serde_json::to_string(&run).unwrap();
    let mut run: Run = serde_json::from_str(&saved).unwrap();
    run.resume().unwrap();
    let task = run.task(&a).unwrap();
    assert_eq!(task.status, Status::Queued);
    assert!(task.recovery.as_deref().unwrap().contains("The app quit."));
    assert_eq!(run.schedule(), std::slice::from_ref(&a));
    assert_eq!(run.task(&a).unwrap().dispatch, 2);
    assert!(run.settle(&a, 2, true, "resumed", None));
    let prompt = run.continuation(&none).unwrap().unwrap();
    assert!(
        prompt.contains("Previous interruption: The app quit."),
        "{prompt}"
    );
    assert_eq!(run.summary().tasks[0].status, "review");
}

#[test]
fn receipts_return_the_first_result_and_reject_reused_ids() {
    let mut run = run(&[("a", "src", &[])], 4);
    run.record("req-1", "hash", &json!({"taskId":"t9"}));
    assert_eq!(
        run.receipt("req-1", "hash").unwrap().unwrap()["taskId"],
        "t9"
    );
    assert!(run.receipt("req-1", "other").unwrap().is_err());
    assert!(run.receipt("req-2", "hash").is_none());
    assert_eq!(
        shell_path("/Applications/MonoBOT.app/Contents/MacOS/monocode-gpui"),
        "/Applications/MonoBOT.app/Contents/MacOS/monocode-gpui"
    );
    assert_eq!(shell_path("/Users/a b/app"), "'/Users/a b/app'");
    assert!(client(&["--help".into()]).unwrap()["help"]
        .as_str()
        .unwrap()
        .contains("finish"));
}
