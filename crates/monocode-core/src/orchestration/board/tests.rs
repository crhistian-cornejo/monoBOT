use super::*;
use crate::orchestration::control::{Run, Task};

fn run(tasks: Vec<Task>) -> Run {
    let mut run = Run::swarm(7, "/p", "/p", true, "Migrate auth", Vec::new(), 4);
    run.tasks = tasks;
    run
}
fn task(id: &str, handle: Option<&str>) -> Task {
    Task {
        id: id.into(),
        title: format!("Task {id}"),
        files: vec!["src".into()],
        handle: handle.map(Into::into),
        ..Default::default()
    }
}
const NONE: &dyn Fn(&Task) -> bool = &|_| false;

#[test]
fn mentions_skip_addresses_and_repeat_once() {
    assert_eq!(
        mention_words("@Worker-1 ping @all, mail a@b.com and @worker-1 again @x-"),
        ["worker-1", "all", "x"]
    );
    assert_eq!(handle_from("  Race Verifier! "), "race-verifier");
    assert_eq!(
        segments("Wired `Lookup(kid)` in `wt`"),
        [
            (false, "Wired "),
            (true, "Lookup(kid)"),
            (false, " in "),
            (true, "wt")
        ]
    );
    assert_eq!(segments("odd `tick"), [(false, "odd "), (false, "tick")]);
    assert_eq!(title("/swarm  Migrate auth\nmore"), "Migrate auth");
    assert_eq!(since(1_000, 109_000), "+1m 48s");
    assert_eq!(since(0, 5_000), "+5s");
    assert_eq!(
        report("Did things.\n\n## Result\nAll 28 tests pass."),
        "Result All 28 tests pass."
    );
}

#[test]
fn the_run_posts_assignments_reports_and_reviews_by_itself() {
    let mut board = Board::new("Migrate auth");
    let mut verifier = task("t2", Some("Verifier"));
    verifier.files = vec!["tests".into()];
    let mut run = run(vec![task("t1", None), verifier]);
    assert!(board.sync(&run, NONE, 1));
    let handles: Vec<_> = board.members.iter().map(|m| m.handle.as_str()).collect();
    assert_eq!(handles, ["coordinator", "worker-1", "verifier"]);
    assert!(board.posts.is_empty(), "queued tasks say nothing yet");
    assert_eq!(board.members[1].section(), Section::StandingBy);

    let started = run.schedule();
    assert_eq!(started.len(), 2);
    run.prepared("t1", 1, 11, None);
    board.sync(&run, &|t| t.id == "t2", 2);
    assert_eq!(board.posts.len(), 2);
    assert_eq!(board.posts[0].kind, Kind::Assign);
    assert_eq!(board.posts[0].from, COORDINATOR);
    assert_eq!(board.posts[0].to, ["worker-1"]);
    assert!(board.posts[0].text.contains("`src`"));
    assert_eq!(board.members[1].state, State::Working);
    assert_eq!(board.members[1].session, Some(11));
    assert_eq!(board.members[2].state, State::NeedsInput);
    assert_eq!(board.handle_of(11), Some("worker-1"));

    run.settle(
        "t1",
        1,
        true,
        "Edited.\n\nGuarded `Lookup` with RLock.",
        None,
    );
    board.sync(&run, NONE, 3);
    let last = board.posts.last().unwrap();
    assert_eq!((last.kind, last.from.as_str()), (Kind::Report, "worker-1"));
    assert_eq!(last.to, [COORDINATOR]);
    assert_eq!(last.text, "Guarded `Lookup` with RLock.");
    assert_eq!(board.members[1].state, State::StandingBy);
    assert_eq!(board.members[1].latest, "Guarded `Lookup` with RLock.");

    run.accept("t1", vec!["src/a.rs".into()]);
    board.sync(&run, NONE, 4);
    assert_eq!(
        board.posts.last().unwrap().text,
        "Accepted Task t1: 1 file applied."
    );
    assert_eq!(board.members[1].state, State::Done);
    assert_eq!(board.members[1].section(), Section::Finished);

    run.settle("t2", 1, false, "", Some("Tool crashed\nstack".into()));
    board.sync(&run, NONE, 5);
    assert_eq!(
        board.posts.last().unwrap().text,
        "Failed: Tool crashed stack"
    );
    // Nothing changed: nothing posted.
    let count = board.posts.len();
    assert!(!board.sync(&run, NONE, 6));
    assert_eq!(board.posts.len(), count);
}

#[test]
fn posts_reach_the_members_they_name_once() {
    let mut board = Board::new("Work");
    let run = run(vec![task("t1", None), task("t2", None)]);
    board.sync(&run, NONE, 1);
    let error = board.post("worker-1", "@nobody look", 2).unwrap_err();
    assert!(
        error.contains("@nobody") && error.contains("@worker-2"),
        "{error}"
    );
    assert!(board.post("stranger", "hi", 2).is_err());
    let post = board
        .post("worker-1", "@Worker-2 use `Lookup(kid)`", 2)
        .unwrap();
    assert_eq!(post.to, ["worker-2"]);
    board.post("coordinator", "@all rebase on main", 3).unwrap();
    assert!(board.pending("worker-2"));
    assert!(board.pending("worker-1"), "@all reaches the sender's peers");
    let inbox = board.inbox("worker-2").unwrap();
    assert!(
        inbox.contains("@worker-1: @Worker-2 use `Lookup(kid)`"),
        "{inbox}"
    );
    assert!(
        inbox.contains("@coordinator: @all rebase on main"),
        "{inbox}"
    );
    assert!(board.inbox("worker-2").is_none());
    let inbox = board.inbox("worker-1").unwrap();
    assert!(
        !inbox.contains("use `Lookup"),
        "its own post is not news: {inbox}"
    );
    assert!(!board.pending(COORDINATOR));
    // A member that joins later starts with an empty inbox.
    let mut later = run.clone();
    later.tasks.push(task("t3", None));
    board.sync(&later, NONE, 4);
    assert!(board.inbox("worker-3").is_none());
}

#[test]
fn the_graph_counts_edges_up_to_the_cursor() {
    let mut board = Board::new("Work");
    board.sync(&run(vec![task("t1", None), task("t2", None)]), NONE, 1);
    board
        .post("coordinator", "@worker-1 @worker-2 go", 1)
        .unwrap();
    board.post("worker-1", "@coordinator done", 2).unwrap();
    board
        .post("worker-1", "@coordinator really done", 3)
        .unwrap();
    board.post("worker-2", "@all heads up", 4).unwrap();
    board.post("worker-2", "thinking aloud", 5).unwrap();

    let graph = board.graph(3, false);
    assert_eq!(graph.edges.len(), 3);
    let back = graph
        .edges
        .iter()
        .find(|e| e.from == 1 && e.to == 0)
        .unwrap();
    assert_eq!((back.count, back.last), (2, 3));

    let graph = board.graph(usize::MAX, false);
    assert_eq!(graph.all_posts, 1);
    assert_eq!(graph.edges.len(), 3, "@all draws nothing unless expanded");
    assert_eq!(graph.silent, [false, false, true]);

    let graph = board.graph(usize::MAX, true);
    let via: Vec<_> = graph.edges.iter().filter(|e| e.via_all).collect();
    assert_eq!(via.len(), 2);
    assert!(graph.involved.iter().all(|i| *i));

    let points = layout(5);
    assert!((points[0].0 - 0.5).abs() < 1e-5 && points[0].1 < 0.2);
    assert!(points[1].0 > 0.8 && points[4].0 < 0.2);
    assert_eq!(layout(1).len(), 1);
}

#[test]
fn old_posts_are_dropped_and_long_ones_refused() {
    let mut board = Board::new("Work");
    for n in 0..MAX_POSTS + 5 {
        board
            .post(COORDINATOR, &format!("note {n}"), n as u64)
            .unwrap();
    }
    assert_eq!(board.posts.len(), MAX_POSTS);
    assert_eq!(board.posts[0].text, "note 5");
    assert!(board
        .post(COORDINATOR, &"x".repeat(POST_CHARS + 1), 0)
        .is_err());
    let view = board.view(board.next_seq - 3, 50);
    assert_eq!(view["posts"].as_array().unwrap().len(), 2);
    assert_eq!(view["members"][0]["handle"], "coordinator");
}
