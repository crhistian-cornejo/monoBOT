use super::*;

fn ids(look: &Look) -> Vec<(&str, u32)> {
    look.shelf
        .iter()
        .map(|widget| (widget.id.as_str(), widget.span))
        .collect()
}

#[test]
fn an_unreadable_or_odd_look_falls_back_field_by_field() {
    assert_eq!(read(None), Look::default());
    assert_eq!(read(Some("not json")), Look::default());
    let look = read(Some(
        r#"{"style":"blob","opensOn":"click","noticeSeconds":99,"shelfRows":0,"width":"huge",
            "shelf":[{"id":"clock","span":3},{"id":"nope","span":1},{"id":"clock","span":1},
                     {"id":"agents","span":1},{"id":"system","span":2}]}"#,
    ));
    assert_eq!(look.style, "notch");
    assert!(look.opens_on_click());
    assert_eq!(look.notice_seconds, 12);
    assert_eq!(look.width, "regular", "an unknown width is the default");
    assert_eq!(look.shelf_rows, 1);
    // Unknown and repeated widgets go, a width it does not take is the
    // widest it does below it, and what no longer fits one row is dropped.
    assert_eq!(ids(&look), [("clock", 1), ("agents", 2)]);
    assert_eq!(read(Some(&to_json(&look))), look);
}

#[test]
fn widgets_fill_the_first_row_with_room() {
    let shelf = [
        Widget::new("agents", 2),
        Widget::new("asks", 2),
        Widget::new("clock", 1),
        Widget::new("new", 1),
    ];
    assert_eq!(layout(&shelf), [vec![0, 2], vec![1, 3]]);
    assert_eq!(rows(&shelf), 2);
    assert_eq!(rows(&[]), 0);
}

#[test]
fn the_shelf_is_arranged_only_within_its_rows() {
    let mut look = Look::default();
    assert_eq!(
        ids(&look),
        [("agents", 2), ("clock", 1), ("recent", 2), ("new", 1)]
    );
    // Both rows are full: nothing more fits, and nothing can grow.
    assert!(look.addable().is_empty());
    assert!(!look.add("call"));
    assert!(!look.resize("recent", 1));
    // A third row makes room.
    look.set_rows(3);
    assert_eq!(look.addable(), ["asks", "today", "call", "system"]);
    assert!(look.add("asks"));
    assert!(look.resize("asks", 1));
    assert!(look.resize("asks", 1));
    assert!(!look.resize("asks", 1), "three columns is its widest");
    assert!(look.resize("asks", -1));
    assert!(!look.resize("clock", 1), "the clock has one width");
    assert!(look.shift("new", -1));
    assert_eq!(look.shelf[2].id, "new");
    assert!(!look.shift("agents", -1), "already first");
    look.remove("agents");
    assert!(look.shelf.iter().all(|widget| widget.id != "agents"));
    // Back to one row: what does not fit goes, last first.
    look.set_rows(1);
    assert_eq!(rows(&look.shelf), 1);
    assert_eq!(ids(&look), [("clock", 1), ("new", 1)]);
}

#[test]
fn ages_read_short() {
    assert_eq!(ago(0, 30_000), "Now");
    assert_eq!(ago(0, 5 * 60_000), "5m");
    assert_eq!(ago(0, 3 * 3_600_000), "3h");
    assert_eq!(ago(0, 30 * 3_600_000), "Yesterday");
    assert_eq!(ago(0, 5 * 86_400_000), "5d");
}

fn macbook() -> Screen {
    Screen {
        width: 1512.,
        safe_top: 38.,
        left_area: 662.,
        right_area: 664.,
        menu_bar: 37.,
    }
}

#[test]
fn the_camera_is_what_the_menu_bar_leaves_between_its_areas() {
    let camera = camera(&macbook()).unwrap();
    assert_eq!(camera.width, 186.);
    assert_eq!(camera.center, 755.);
    assert_eq!(camera.height, 37., "never taller than the menu bar");
    // An external display, or Windows, has no camera.
    assert_eq!(
        super::camera(&Screen {
            width: 2560.,
            menu_bar: 25.,
            ..Screen::default()
        }),
        None
    );
}

#[test]
fn the_notch_wraps_the_camera_and_the_island_floats_under_the_bar() {
    let look = Look::default();
    let screen = macbook();
    // At rest it is the camera itself.
    assert_eq!(
        frame(&look, &screen, Shape::Quiet),
        Frame {
            x: 662.,
            y: 0.,
            width: 186.,
            height: 37.,
        }
    );
    let busy = frame(
        &look,
        &screen,
        Shape::Busy {
            left: 40,
            right: 160,
        },
    );
    // The wider wing sets both, so the camera stays in the middle.
    assert_eq!((busy.width, busy.x), (186. + 2. * 160., 662. - 160.));
    let open = frame(&look, &screen, Shape::Open { body: 200 });
    assert_eq!(open.y, 0.);
    assert_eq!(open.height, 37. + PANEL_HEADER + 200. + PANEL_PAD);
    assert_eq!(open.x + open.width / 2., 755.);

    let island = Look {
        style: "island".into(),
        ..Look::default()
    };
    let rest = frame(&island, &screen, Shape::Quiet);
    assert_eq!(rest.y, 38. + ISLAND_GAP);
    assert_eq!((rest.width, rest.height), ISLAND_REST);
    assert_eq!(rest.x + rest.width / 2., 756., "centered on the screen");
    // Its panel has no camera to clear.
    let open = frame(&island, &screen, Shape::Open { body: 200 });
    assert_eq!(open.height, PANEL_HEADER + 200. + PANEL_PAD);
}

#[test]
fn without_a_camera_the_quiet_notch_is_a_handle_on_the_top_edge() {
    let screen = Screen {
        width: 1920.,
        ..Screen::default()
    };
    let rest = frame(&Look::default(), &screen, Shape::Quiet);
    assert_eq!((rest.width, rest.height), HANDLE);
    assert_eq!((rest.x, rest.y), (900., 0.));
    let busy = frame(
        &Look::default(),
        &screen,
        Shape::Busy {
            left: 40,
            right: 36,
        },
    );
    assert_eq!(busy.height, BAR_HEIGHT);
}

#[test]
fn cells_split_the_panel_into_three_columns() {
    let width = PANEL_WIDTH;
    let one = cell_width(width, 1);
    assert_eq!(cell_width(width, 3), width - 2. * PANEL_PAD);
    assert_eq!(cell_width(width, 2), 2. * one + CELL_GAP);
    assert_eq!(shelf_height(2), 2. * CELL_HEIGHT + CELL_GAP);
}

fn working(id: i64, provider: &str, started_at: u64) -> Activity {
    Activity {
        id,
        provider: provider.into(),
        title: format!("Session {id}"),
        step: "Reading files".into(),
        started_at: Some(started_at),
        needs_you: false,
        done: false,
        agent: None,
        steps: None,
    }
}

fn ask(session: i64, request: &str) -> Ask {
    Ask {
        session,
        request: request.into(),
        provider: "codex".into(),
        title: format!("Session {session}"),
        label: "cargo test".into(),
        approval: true,
    }
}

#[test]
fn the_rest_counts_work_and_leads_with_what_asks() {
    let mut feed = Feed {
        activities: vec![working(1, "claude", 20), working(2, "codex", 10)],
        ..Feed::default()
    };
    let at_rest = rest(&feed);
    assert_eq!(at_rest.working, 2);
    assert_eq!(
        at_rest.lead.as_deref(),
        Some("codex"),
        "the longest running"
    );
    assert!(at_rest.busy());
    feed.asks.push(Ask {
        provider: "claude".into(),
        ..ask(1, "r1")
    });
    assert_eq!(rest(&feed).lead.as_deref(), Some("claude"));
    assert!(!rest(&Feed::default()).busy());
}

#[test]
fn asks_wait_for_an_answer_and_finished_turns_go_by_themselves() {
    let look = Look::default();
    let mut notices = Notices::default();
    let mut feed = Feed {
        activities: vec![working(1, "claude", 0)],
        ..Feed::default()
    };
    notices.update(&feed, &look, 0);
    assert_eq!(notices.current(), None);

    feed.activities[0].done = true;
    notices.update(&feed, &look, 1_000);
    assert!(matches!(
        notices.current(),
        Some(Notice::Done { session: 1, .. })
    ));
    // An ask comes in: it goes first, and the finished turn waits.
    feed.asks.push(ask(2, "r1"));
    notices.update(&feed, &look, 2_000);
    assert!(matches!(notices.current(), Some(Notice::Ask(a)) if a.request == "r1"));
    // An ask never times out.
    assert_eq!(notices.tick(&look, 60_000), None);
    assert!(matches!(notices.current(), Some(Notice::Ask(_))));
    // Answered elsewhere, it goes, and the finished turn shows again.
    feed.asks.clear();
    notices.update(&feed, &look, 61_000);
    assert!(matches!(notices.current(), Some(Notice::Done { .. })));
    assert_eq!(notices.tick(&look, 62_000), Some(3_000));
    assert_eq!(notices.tick(&look, 65_000), None);
    assert_eq!(notices.current(), None);
    // Still finished: no second notice.
    notices.update(&feed, &look, 66_000);
    assert_eq!(notices.current(), None);
}

#[test]
fn what_had_finished_before_the_notch_started_is_not_news() {
    let mut finished = working(1, "claude", 0);
    finished.done = true;
    let feed = Feed {
        activities: vec![finished],
        asks: vec![ask(3, "r9")],
        ..Feed::default()
    };
    let mut notices = Notices::default();
    notices.update(&feed, &Look::default(), 0);
    // A pending ask still needs the user.
    assert!(matches!(notices.current(), Some(Notice::Ask(_))));
    notices.dismiss(1);
    assert_eq!(notices.current(), None);
}

#[test]
fn finished_turns_can_stay_quiet() {
    let look = Look {
        finished_notices: false,
        ..Look::default()
    };
    let mut notices = Notices::default();
    let mut feed = Feed {
        activities: vec![working(1, "claude", 0)],
        ..Feed::default()
    };
    notices.update(&feed, &look, 0);
    feed.activities[0].done = true;
    notices.update(&feed, &look, 1);
    assert_eq!(notices.current(), None);
}

#[test]
fn several_asks_show_one_after_another_in_order() {
    let look = Look::default();
    let mut notices = Notices::default();
    let feed = Feed {
        asks: vec![ask(1, "a"), ask(2, "b")],
        ..Feed::default()
    };
    notices.update(&feed, &look, 0);
    assert!(matches!(notices.current(), Some(Notice::Ask(a)) if a.request == "a"));
    notices.dismiss(1);
    assert!(matches!(notices.current(), Some(Notice::Ask(a)) if a.request == "b"));
    // A dismissed ask that is still pending is not raised again.
    notices.update(&feed, &look, 2);
    assert!(matches!(notices.current(), Some(Notice::Ask(a)) if a.request == "b"));
    notices.dismiss(3);
    notices.update(&feed, &look, 4);
    assert_eq!(notices.current(), None);
}

#[test]
fn the_panel_takes_the_chosen_width() {
    let screen = macbook();
    for (width, points) in [("compact", 392.), ("regular", PANEL_WIDTH), ("wide", 496.)] {
        let look = Look {
            width: width.into(),
            ..Look::default()
        };
        assert_eq!(
            frame(&look, &screen, Shape::Open { body: 100 }).width,
            points
        );
    }
}

#[test]
fn the_wings_show_who_works_and_what_the_lead_does() {
    let mut first = working(1, "claude", 10);
    first.steps = Some((2, 6));
    let mut agent = working(2, "codex", 20);
    agent.agent = Some("Finance".into());
    let feed = Feed {
        activities: vec![
            agent,
            first,
            working(3, "codex", 30),
            working(4, "codex", 40),
        ],
        ..Feed::default()
    };
    let at_rest = rest(&feed);
    assert_eq!(
        at_rest.marks,
        ["claude", "codex", "codex"],
        "longest running first"
    );
    assert_eq!(at_rest.more(), 1);
    assert_eq!(at_rest.lead_text.as_deref(), Some("Claude Code · Reading files"));
    assert_eq!(at_rest.lead_steps, Some((2, 6)));
    let (left, right) = at_rest.wings(true);
    assert_eq!(left, 14 + 3 * 18 + 22);
    assert_eq!(right, 196);
    assert_eq!(
        at_rest.wings(false).0,
        14 + 3 * 18,
        "no +N without the count"
    );
    // An ask leads, and says so.
    let asked = Feed {
        asks: vec![ask(2, "r1")],
        ..feed
    };
    assert_eq!(
        rest(&asked).lead_text.as_deref(),
        Some("Finance · Needs you")
    );
}
