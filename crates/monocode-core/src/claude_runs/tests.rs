use super::*;
use serde_json::json;

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn lines(values: &[Value]) -> String {
    values.iter().map(|v| format!("{v}\n")).collect::<String>()
}

fn assistant(model: &str, content: Value) -> Value {
    json!({"type":"assistant","timestamp":"2026-11-11T14:18:51.354Z",
        "message":{"role":"assistant","model":model,"content":content}})
}

#[test]
fn finds_the_session_folder_by_its_transcript() {
    let home = tempfile::tempdir().unwrap();
    write(&home.path().join("projects/D--app/abc.jsonl"), "");
    write(&home.path().join("projects/D--other/zzz.jsonl"), "");
    assert_eq!(
        session_dir(home.path(), "abc"),
        Some(home.path().join("projects/D--app/abc"))
    );
    assert_eq!(session_dir(home.path(), "missing"), None);
    assert_eq!(session_dir(home.path(), "../abc"), None);
}

#[test]
fn a_workflow_reads_as_phases_with_their_agents_and_latest_steps() {
    let session = tempfile::tempdir().unwrap();
    let session = session.path();
    write(
        &session.join("workflows/scripts/review-wf_1.js"),
        "export const meta = {\n  name: 'review',\n  description: \"Review the diff\",\n  phases: [{ title: 'Find', detail: 'look: here' }, { title: 'Verify' }, { title: 'Report' }],\n}\nconst x = { title: 'not a phase' }\n",
    );
    let run = session.join("subagents/workflows/wf_1");
    write(
        &run.join("journal.jsonl"),
        &lines(&[
            json!({"type":"launched"}),
            json!({"type":"started","agentId":"a1","label":"find: bugs","phase":"Find"}),
            json!({"type":"started","agentId":"a2","label":"find: perf","phase":"Find"}),
            json!({"type":"result","agentId":"a1","result":{"summary":"## Done\nFound two bugs."}}),
            json!({"type":"started","agentId":"a3","label":"verify","phase":"Verify"}),
            json!({"type":"result","agentId":"a2","result":{"findings":[1,2,3]}}),
        ]),
    );
    write(
        &run.join("agent-a3.jsonl"),
        &lines(&[
            json!({"type":"user","timestamp":"2026-11-11T14:00:00.000Z"}),
            assistant(
                "claude-opus-5-5",
                json!([{"type":"text","text":"Checking the first finding"},
                       {"type":"tool_use","name":"Bash","input":{"command":"cargo test -p core\nmore","description":"Run tests"}}]),
            ),
            json!({"type":"api-request","assistant":"ignored"}),
        ]),
    );
    write(
        &run.join("agent-a1.jsonl"),
        &lines(&[assistant(
            "m",
            json!([{"type":"tool_use","name":"Read","input":{"file_path":"/repo/src/lib.rs"}}]),
        )]),
    );
    let now = modified(&run.join("journal.jsonl")).unwrap();
    let snapshot = Scanner::default().scan(session, now);
    assert!(snapshot.agents.is_empty());
    let run = &snapshot.runs[0];
    assert_eq!(run.name, "review");
    assert_eq!(run.description.as_deref(), Some("Review the diff"));
    assert_eq!(
        run.phases
            .iter()
            .map(|p| p.title.as_str())
            .collect::<Vec<_>>(),
        ["Find", "Verify", "Report"]
    );
    let find = &run.phases[0];
    assert_eq!(find.state(), Some(State::Done));
    assert_eq!(find.agents[0].summary.as_deref(), Some("Done"));
    assert_eq!(find.agents[1].summary.as_deref(), Some("3 findings"));
    assert_eq!(
        find.agents[0].last,
        Some(Step {
            tool: Some("Read".into()),
            text: "lib.rs".into()
        })
    );
    let verify = &run.phases[1].agents[0];
    assert_eq!(verify.state, State::Running);
    assert_eq!(verify.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(
        verify.last,
        Some(Step {
            tool: Some("Bash".into()),
            text: "cargo test -p core".into()
        })
    );
    assert_eq!(verify.started_at, iso_ms("2026-11-11T14:00:00.000Z"));
    assert_eq!(run.phases[2].state(), None);
    assert_eq!(run.state(), State::Running);
    assert_eq!(snapshot.running(), 1);

    // Long after its last write, an unfinished agent has stopped.
    let later = Scanner::default().scan(session, now + STALE_MS + 60_000);
    assert_eq!(later.runs[0].phases[1].agents[0].state, State::Stopped);
    assert_eq!(later.runs[0].state(), State::Stopped);
}

#[test]
fn plain_subagents_finish_by_their_meta_and_rescans_reuse_unchanged_files() {
    let session = tempfile::tempdir().unwrap();
    let dir = session.path().join("subagents");
    write(
        &dir.join("agent-x.meta.json"),
        &json!({"agentType":"general-purpose","description":"Port readers","model":"sonnet","finished":true}).to_string(),
    );
    write(
        &dir.join("agent-x.jsonl"),
        &lines(&[assistant(
            "m",
            json!([{"type":"text","text":"\n  All ported.\nDetails"}]),
        )]),
    );
    write(
        &dir.join("agent-y.meta.json"),
        &json!({"agentType":"Explore"}).to_string(),
    );
    write(&dir.join("agent-y.jsonl"), "");
    let now = modified(&dir.join("agent-y.jsonl")).unwrap();
    let mut scanner = Scanner::default();
    let snapshot = scanner.scan(session.path(), now);
    let x = snapshot.agents.iter().find(|a| a.id == "x").unwrap();
    assert_eq!(x.label, "Port readers");
    assert_eq!(x.state, State::Done);
    assert_eq!(x.model.as_deref(), Some("sonnet"));
    assert_eq!(
        x.last,
        Some(Step {
            tool: None,
            text: "All ported.".into()
        })
    );
    let y = snapshot.agents.iter().find(|a| a.id == "y").unwrap();
    assert_eq!((y.label.as_str(), y.state), ("Explore", State::Running));
    assert_eq!(scanner.scan(session.path(), now), snapshot);
    assert!(!snapshot.is_empty());
    assert!(Scanner::default()
        .scan(&session.path().join("none"), now)
        .is_empty());
}

#[test]
fn timestamps_and_tool_inputs_read_as_one_line() {
    assert_eq!(iso_ms("1970-01-01T00:00:01.500Z"), Some(1_500));
    assert_eq!(iso_ms("2000-03-01T00:00:00Z"), Some(951_868_800_000));
    assert_eq!(tool_text("Grep", &json!({"pattern":"fn main"})), "fn main");
    assert_eq!(
        tool_text("Agent", &json!({"description":"Explore"})),
        "Explore"
    );
    assert_eq!(tool_text("Other", &json!({})), "");
    assert_eq!(clip("abcdef", 4), "abc…");
}
