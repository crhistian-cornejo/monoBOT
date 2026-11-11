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
        ..Default::default()
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

#[test]
fn swarm_roles_pin_a_harness_and_model_to_a_handle() {
    let (roles, rest) =
        swarm_roles("@Verifier=codex @tester=claude/opus  Migrate auth @x=y").unwrap();
    assert_eq!(rest, "Migrate auth @x=y", "only leading words are roles");
    assert_eq!(
        roles,
        [
            Role {
                handle: "verifier".into(),
                provider: "codex".into(),
                model: None
            },
            Role {
                handle: "tester".into(),
                provider: "claude".into(),
                model: Some("opus".into())
            },
        ]
    );
    assert_eq!(
        swarm_roles("Plain task").unwrap(),
        (Vec::new(), "Plain task".into())
    );
    assert!(swarm_roles("@all=codex task").is_err());
    assert!(swarm_roles("@v=codex @v=claude task")
        .unwrap_err()
        .contains("twice"));
    assert!(swarm_roles("@v=claude/ task").is_err());

    let mut run = run(&[("a", "lib", &[])], 4);
    run.roles = roles;
    assert!(run.check_roles().is_ok());
    run.roles.push(Role {
        handle: "docs".into(),
        provider: "gemini".into(),
        model: None,
    });
    assert!(run.check_roles().unwrap_err().contains("gemini"));
    run.roles.pop();

    // A pinned handle needs no harness, and refuses another one.
    let (value, _) = run
        .execute(
            "delegate",
            &json!({"title":"t","prompt":"p","files":["tests"],"handle":"tester"}),
            &none,
        )
        .unwrap();
    let id = value["taskId"].as_str().unwrap();
    let task = run.task(id).unwrap();
    assert_eq!(
        (task.provider.as_str(), task.model.as_deref()),
        ("claude", Some("opus"))
    );
    let error = run
        .execute(
            "delegate",
            &json!({"title":"v","harness":"claude","prompt":"p","files":["src"],"handle":"verifier"}),
            &none,
        )
        .unwrap_err();
    assert!(error.contains("pinned @verifier to codex"), "{error}");
    let error = run
        .execute(
            "delegate",
            &json!({"title":"t2","prompt":"p","files":["docs"],"handle":"tester","model":"sonnet"}),
            &none,
        )
        .unwrap_err();
    assert!(error.contains("pinned @tester to opus"), "{error}");
    // The prompt names the pins; the started payload carries the rules.
    assert!(swarm_prompt("Do it", "/bin/m", &run.roles).contains("@tester on claude with opus"));
    let started = swarm_started("Do it", "/bin/m", &[]);
    assert_eq!(started["handle"], "coordinator");
    assert!(started["instructions"]
        .as_str()
        .unwrap()
        .contains("control --help"));
    assert!(swarm_offer("/bin/m").contains("control swarm"));
}

fn chief_run() -> Run {
    Run::chief(
        9,
        3,
        "/repo",
        "/repo",
        true,
        "Migrate auth",
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
        Some(Defaults {
            provider: "claude".into(),
            model: Some("opus".into()),
            effort: Some("medium".into()),
        }),
        vec![CrewMember {
            id: "mono-a".into(),
            name: "Ada".into(),
            specialty: "Rust backends".into(),
        }],
    )
}

#[test]
fn chief_delegates_with_opus_at_medium_unless_told_otherwise() {
    let mut run = chief_run();
    assert_eq!((run.lead, run.requester), (9, Some(3)));
    let (task, _) = run
        .execute(
            "delegate",
            &json!({"title":"Keys","prompt":"Rotate keys","files":["src/keys"]}),
            &none,
        )
        .unwrap();
    let task = run.task(task["taskId"].as_str().unwrap()).unwrap();
    assert_eq!(task.provider, "claude");
    assert_eq!(task.model.as_deref(), Some("opus"));
    assert_eq!(task.effort.as_deref(), Some("medium"));

    // Another model keeps its own default effort.
    let (task, _) = run
        .execute(
            "delegate",
            &json!({"title":"Docs","prompt":"Write docs","files":["docs"],"model":"sonnet"}),
            &none,
        )
        .unwrap();
    let task = run.task(task["taskId"].as_str().unwrap()).unwrap();
    assert_eq!(
        (task.model.as_deref(), task.effort.as_deref()),
        (Some("sonnet"), None)
    );
    let (task, _) = run
        .execute(
            "delegate",
            &json!({"title":"Tests","prompt":"Test","files":["tests"],"harness":"codex"}),
            &none,
        )
        .unwrap();
    let task = run.task(task["taskId"].as_str().unwrap()).unwrap();
    assert_eq!(
        (task.model.as_deref(), task.effort.as_deref()),
        (None, None)
    );
}

#[test]
fn chief_gives_a_task_to_a_named_mono_once_at_a_time() {
    let mut run = chief_run();
    let (task, _) = run
        .execute(
            "delegate",
            &json!({"title":"Keys","prompt":"Rotate","files":["src/keys"],"mono":"ada"}),
            &none,
        )
        .unwrap();
    let id = task["taskId"].as_str().unwrap().to_owned();
    assert_eq!(run.task(&id).unwrap().mono.as_deref(), Some("mono-a"));
    assert!(run.mono_busy("mono-a"));
    let busy = run
        .execute(
            "delegate",
            &json!({"title":"More","prompt":"More","files":["src/more"],"mono":"Ada"}),
            &none,
        )
        .unwrap_err();
    assert!(busy.contains("already working"), "{busy}");
    let unknown = run
        .execute(
            "delegate",
            &json!({"title":"More","prompt":"More","files":["src/more"],"mono":"Zed"}),
            &none,
        )
        .unwrap_err();
    assert!(unknown.contains("Crew: Ada"), "{unknown}");
    let (list, _) = run.execute("list", &json!({}), &none).unwrap();
    assert_eq!(list["crew"][0]["mono"], "Ada");
    assert_eq!(list["crew"][0]["busy"], true);
    assert_eq!(list["defaults"]["effort"], "medium");

    // An ordinary swarm has no crew.
    let mut swarm = Run::swarm(7, "/repo", "/repo", true, "Go", run.choices.clone(), 2);
    let refused = swarm
        .execute(
            "delegate",
            &json!({"title":"A","prompt":"A","files":["a"],"harness":"claude","mono":"Ada"}),
            &none,
        )
        .unwrap_err();
    assert!(refused.contains("only for Chief"), "{refused}");
}

#[test]
fn chief_finishes_with_a_summary_for_the_chat_that_asked() {
    let mut run = chief_run();
    assert_eq!(run.chief_report(), None, "nothing to report while it runs");
    let missing = run.execute("finish", &json!({}), &none).unwrap_err();
    assert!(missing.contains("summary"), "{missing}");
    run.execute(
        "finish",
        &json!({"summary":"Keys rotate on login; tests pass."}),
        &none,
    )
    .unwrap();
    let report = run.chief_report().unwrap();
    assert!(report.contains("Request: Migrate auth"), "{report}");
    assert!(
        report.contains("Keys rotate on login; tests pass."),
        "{report}"
    );

    // An ordinary run reports nothing and needs no summary.
    let mut swarm = Run::swarm(7, "/repo", "/repo", true, "Go", run.choices.clone(), 2);
    swarm.execute("finish", &json!({}), &none).unwrap();
    assert_eq!(swarm.chief_report(), None);

    let mut stopped = chief_run();
    stopped.stop();
    assert!(stopped
        .chief_report()
        .unwrap()
        .contains("stopped before it finished"));
}

#[test]
fn saved_runs_from_before_chief_still_load() {
    let old = json!({"version":1,"lead":7,"project":"/repo","checkout":"/repo","status":"active","tasks":[{"id":"t1","title":"A","provider":"claude"}]});
    let run: Run = serde_json::from_value(old).unwrap();
    assert_eq!(run.requester, None);
    assert!(run.crew.is_empty());
    assert_eq!(run.tasks[0].mono, None);
}

#[test]
fn the_chief_prompts_carry_the_crew_and_the_rules() {
    let crew = [CrewMember {
        id: "mono-a".into(),
        name: "Ada".into(),
        specialty: "Rust backends".into(),
    }];
    let prompt = chief_prompt("Migrate auth", "/bin/monobot", &crew);
    assert!(prompt.starts_with("Migrate auth"));
    assert!(prompt.contains("Ada (Rust backends)"));
    assert!(prompt.contains("finish with {\"summary\""));
    assert!(chief_prompt("X", "/bin/monobot", &[]).contains("no monos yet"));
    assert!(chief_offer("/bin/monobot").contains("control chief"));
    assert_eq!(chief_started("Auth", 12)["chief"], 12);
    check_fields("chief", &json!({"title":"A","brief":"B"})).unwrap();
    assert!(check_fields("chief", &json!({"title":"A","model":"x"})).is_err());
}

#[test]
fn chief_defaults_to_opus_at_medium_from_the_catalog() {
    let model = |id: &str, alias: Option<&str>, efforts: &[&str]| crate::catalog::Model {
        id: id.into(),
        name: id.into(),
        alias_target: alias.map(Into::into),
        efforts: efforts.iter().map(|e| (*e).into()).collect(),
        default_effort: efforts.iter().find(|e| **e == "high").map(|e| (*e).into()),
        fast: false,
        thinking: false,
        default: false,
        tiers: Vec::new(),
        controls: Vec::new(),
    };
    let catalog = [
        model("claude-sonnet-5", None, &["low", "medium", "high"]),
        model(
            "opus",
            Some("claude-opus-5-5"),
            &["low", "medium", "high", "max"],
        ),
    ];
    let defaults = Defaults::chief(&catalog);
    assert_eq!(defaults.provider, "claude");
    assert_eq!(defaults.model.as_deref(), Some("opus"));
    assert_eq!(defaults.effort.as_deref(), Some("medium"));
    // Without medium, the model's own default effort.
    let defaults = Defaults::chief(&[model("opus", None, &["low", "high", "max"])]);
    assert_eq!(defaults.effort.as_deref(), Some("high"));
    // No Opus at all: Claude's default model.
    let defaults = Defaults::chief(&[model("claude-sonnet-5", None, &["medium"])]);
    assert_eq!((defaults.model, defaults.effort), (None, None));
}
