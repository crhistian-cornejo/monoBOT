use super::*;
use gpui::{TestAppContext, VisualTestContext};
use monocode_core::orchestration::{
    board::COORDINATOR,
    control::{Run, Task},
};
use std::rc::Rc;

/// A board with two workers: assignments, a peer post, a report.
fn board() -> Arc<Board> {
    let mut run = Run::swarm(1, "/p", "/p", true, "Migrate auth v2", Vec::new(), 4);
    for (id, handle, files) in [("t1", None, "src"), ("t2", Some("verifier"), "tests")] {
        run.tasks.push(Task {
            id: id.into(),
            title: format!("Task {id}"),
            files: vec![files.into()],
            handle: handle.map(Into::into),
            session: Some(if id == "t1" { 11 } else { 12 }),
            ..Default::default()
        });
    }
    let mut board = Board::new(&run.request);
    run.schedule();
    board.sync(&run, &|_| false, 1_000);
    board
        .post("worker-1", "@verifier wired `KeyRing.Lookup(kid)`", 2_000)
        .unwrap();
    run.settle("t1", 1, true, "Guarded `Lookup` with RLock.", None);
    board.sync(&run, &|_| false, 3_000);
    Arc::new(board)
}

fn open(cx: &mut TestAppContext) -> gpui::WindowHandle<SwarmPanel> {
    cx.update(|cx| {
        crate::input::bind(cx);
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| {
                let mut panel = SwarmPanel::new(cx);
                panel.set_shown(true, cx);
                panel.set_board(1, board(), cx);
                panel
            })
        })
        .unwrap()
    })
}

#[gpui::test]
fn the_board_lists_posts_and_filters_them(cx: &mut TestAppContext) {
    let window = open(cx);
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    visual.refresh().unwrap();
    // Two assignments, the peer post and the report.
    assert!(visual.debug_bounds("swarm-post-1").is_some());
    assert!(visual.debug_bounds("swarm-post-4").is_some());
    let filter = visual.debug_bounds("swarm-filter").unwrap();
    visual.simulate_click(filter.center(), Default::default());
    visual.refresh().unwrap();
    window
        .update(visual, |panel, _, _| {
            assert_eq!(panel.rows.len(), 1, "only the agent's own post");
            assert_eq!(panel.board().posts[panel.rows[0]].from, "worker-1");
        })
        .unwrap();
    assert!(visual.debug_bounds("swarm-post-1").is_none());
    window
        .update(visual, |panel, _, cx| {
            panel.agents_only = false;
            panel
                .query
                .update(cx, |input, cx| input.set_text("@verifier".into(), cx));
        })
        .unwrap();
    visual.run_until_parked();
    window
        .update(visual, |panel, _, _| {
            // The verifier's assignment and the post that names it.
            assert_eq!(panel.rows.len(), 2);
        })
        .unwrap();
}

#[gpui::test]
fn members_are_grouped_and_open_their_chat(cx: &mut TestAppContext) {
    let window = open(cx);
    let opened = Rc::new(std::cell::Cell::new(None));
    let seen = opened.clone();
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    window
        .update(visual, |_, _, cx| {
            let entity = cx.entity();
            cx.subscribe(&entity, move |_, _, event: &OpenMember, _| {
                seen.set(Some(event.0))
            })
            .detach();
        })
        .unwrap();
    visual.refresh().unwrap();
    let tab = visual.debug_bounds("swarm-tab-Members").unwrap();
    visual.simulate_click(tab.center(), Default::default());
    visual.refresh().unwrap();
    assert_eq!(
        window.update(visual, |p, _, _| p.tab()).unwrap(),
        SwarmTab::Members
    );
    // Debug selectors are looked up by a 'static name.
    let coordinator_id: &'static str = format!("swarm-member-{COORDINATOR}").leak();
    let coordinator = visual.debug_bounds(coordinator_id).unwrap();
    let verifier = visual.debug_bounds("swarm-member-verifier").unwrap();
    let worker = visual.debug_bounds("swarm-member-worker-1").unwrap();
    // Coordinator, then the active verifier, then the worker standing by.
    assert!(coordinator.top() < verifier.top());
    assert!(verifier.top() < worker.top());
    visual.simulate_click(worker.center(), Default::default());
    assert_eq!(opened.get(), Some(11));
}

#[gpui::test]
fn the_graph_replays_and_returns_to_live(cx: &mut TestAppContext) {
    let window = open(cx);
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    visual.refresh().unwrap();
    let tab = visual.debug_bounds("swarm-tab-Graph").unwrap();
    visual.simulate_click(tab.center(), Default::default());
    visual.refresh().unwrap();
    assert!(visual.debug_bounds("swarm-graph").is_some());
    let len = window
        .update(visual, |p, _, _| p.board().posts.len())
        .unwrap();
    assert_eq!(
        window.update(visual, |p, _, _| p.timeline.cursor).unwrap(),
        len,
        "a live graph shows every post"
    );
    let first = visual.debug_bounds("swarm-first").unwrap();
    visual.simulate_click(first.center(), Default::default());
    let next = visual.debug_bounds("swarm-next").unwrap();
    visual.simulate_click(next.center(), Default::default());
    window
        .update(visual, |p, _, _| {
            assert_eq!(p.timeline.cursor, 1);
            assert!(!p.timeline.live);
        })
        .unwrap();
    // A new post while replaying does not move the cursor.
    window
        .update(visual, |p, _, cx| {
            let mut more = (*p.board).clone();
            more.post(COORDINATOR, "@all rebase", 4_000).unwrap();
            p.set_board(1, Arc::new(more), cx);
            assert_eq!(p.timeline.cursor, 1);
        })
        .unwrap();
    visual.refresh().unwrap();
    let live = visual.debug_bounds("swarm-go-live").unwrap();
    visual.simulate_click(live.center(), Default::default());
    window
        .update(visual, |p, _, _| {
            assert_eq!(p.timeline.cursor, len + 1);
            assert!(p.timeline.live);
        })
        .unwrap();
    // Replay runs on a timer and stops at the end, back to live.
    let play = visual.debug_bounds("swarm-play").unwrap();
    visual.simulate_click(play.center(), Default::default());
    assert!(window.update(visual, |p, _, _| p.timeline.playing).unwrap());
    for _ in 0..10 {
        visual
            .executor()
            .advance_clock(std::time::Duration::from_secs(1));
        visual.run_until_parked();
    }
    window
        .update(visual, |p, _, _| {
            assert!(!p.timeline.playing);
            assert!(p.timeline.live);
            assert_eq!(p.timeline.cursor, len + 1);
        })
        .unwrap();
}

#[gpui::test]
fn new_posts_since_the_last_visit_sit_under_new(cx: &mut TestAppContext) {
    let window = open(cx);
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    window
        .update(visual, |p, _, cx| {
            assert_eq!(p.divider, None, "a first visit has nothing new");
            p.set_shown(false, cx);
            let mut more = (*p.board).clone();
            more.post("verifier", "@coordinator all 28 tests pass", 5_000)
                .unwrap();
            let seq = more.next_seq - 1;
            p.set_board(1, Arc::new(more), cx);
            p.set_shown(true, cx);
            assert_eq!(p.divider, Some(seq));
        })
        .unwrap();
}

#[gpui::test]
fn a_graph_node_opens_its_members_chat(cx: &mut TestAppContext) {
    let window = open(cx);
    let opened = Rc::new(std::cell::Cell::new(None));
    let seen = opened.clone();
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    window
        .update(visual, |p, _, cx| {
            let entity = cx.entity();
            cx.subscribe(&entity, move |_, _, event: &OpenMember, _| {
                seen.set(Some(event.0))
            })
            .detach();
            p.select(SwarmTab::Graph, cx);
        })
        .unwrap();
    visual.refresh().unwrap();
    let nodes = window
        .update(visual, |p, _, _| p.timeline.node_bounds())
        .unwrap();
    let worker = window
        .update(visual, |p, _, _| {
            p.board()
                .members
                .iter()
                .position(|m| m.handle == "worker-1")
        })
        .unwrap()
        .unwrap();
    let (bounds, _) = nodes.iter().find(|(_, m)| *m == worker).copied().unwrap();
    visual.simulate_mouse_move(bounds.center(), None, Default::default());
    assert_eq!(
        window.update(visual, |p, _, _| p.timeline.hover).unwrap(),
        Some(worker)
    );
    visual.simulate_click(bounds.center(), Default::default());
    assert_eq!(opened.get(), Some(11));
}

#[gpui::test]
fn the_timeline_follows_a_drag_past_its_track(cx: &mut TestAppContext) {
    let window = open(cx);
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    window
        .update(visual, |p, _, cx| p.select(SwarmTab::Graph, cx))
        .unwrap();
    visual.refresh().unwrap();
    let track = visual.debug_bounds("swarm-timeline").unwrap();
    let len = window
        .update(visual, |p, _, _| p.board().posts.len())
        .unwrap();
    let start = gpui::point(track.left() + gpui::px(1.), track.center().y);
    visual.simulate_mouse_down(start, gpui::MouseButton::Left, Default::default());
    visual.run_until_parked();
    assert_eq!(
        window.update(visual, |p, _, _| p.timeline.cursor).unwrap(),
        0
    );
    // Well past the track's right end and below it, the drag still counts.
    let past = gpui::point(
        track.right() + gpui::px(300.),
        track.bottom() + gpui::px(80.),
    );
    visual.simulate_mouse_move(past, Some(gpui::MouseButton::Left), Default::default());
    visual.run_until_parked();
    assert_eq!(
        window.update(visual, |p, _, _| p.timeline.cursor).unwrap(),
        len
    );
    visual.simulate_mouse_up(past, gpui::MouseButton::Left, Default::default());
    visual.run_until_parked();
    assert!(
        !window
            .update(visual, |p, _, _| p.timeline.sliding())
            .unwrap()
    );
}
