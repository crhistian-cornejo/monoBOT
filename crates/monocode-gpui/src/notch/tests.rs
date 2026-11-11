use super::*;
use gpui::{Subscription, TestAppContext, VisualTestContext};
use monocode_core::notch::{Activity, Ask, Screen};
use std::cell::RefCell;

/// A 14-inch MacBook Pro: a 186-point housing, 37 points tall.
fn macbook() -> Screen {
    Screen {
        width: 1512.,
        safe_top: 38.,
        left_area: 662.,
        right_area: 664.,
        menu_bar: 37.,
    }
}

struct Opened {
    window: WindowHandle<Notch>,
    saved: Rc<RefCell<Vec<String>>>,
    events: Rc<RefCell<Vec<NotchEvent>>>,
    _events: Subscription,
}

fn open_notch(look: Look, cx: &mut TestAppContext) -> Opened {
    cx.update(|cx| cx.set_reduce_motion(true));
    let saved = Rc::new(RefCell::new(Vec::new()));
    let save: Save = {
        let saved = saved.clone();
        Rc::new(move |json, _| saved.borrow_mut().push(json))
    };
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| {
                let mut notch = Notch::new(look, save, cx);
                notch.screen_override = Some(macbook());
                notch
            })
        })
        .unwrap()
    });
    let entity = window.update(cx, |_, _, cx| cx.entity()).unwrap();
    let events = Rc::new(RefCell::new(Vec::new()));
    let subscription = cx.update(|cx| {
        let events = events.clone();
        cx.subscribe(&entity, move |_, event: &NotchEvent, _| {
            events.borrow_mut().push(event.clone())
        })
    });
    Opened {
        window,
        saved,
        events,
        _events: subscription,
    }
}

fn click(visual: &mut VisualTestContext, selector: &'static str) {
    visual.run_until_parked();
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is drawn"));
    visual.simulate_click(bounds.center(), Default::default());
    visual.run_until_parked();
}

/// The pointer away from the notch, so it rests without swelling.
fn leave(visual: &mut VisualTestContext) {
    visual.simulate_mouse_move(gpui::point(px(-50.), px(-50.)), None, Default::default());
    visual.run_until_parked();
}

/// The island as drawn (the window around it may hold room to move).
fn size(window: WindowHandle<Notch>, visual: &mut VisualTestContext) -> (f32, f32) {
    window
        .update(visual, |notch, _, _| notch.island_size())
        .unwrap()
}

/// Both wings' widths for two sessions reading files.
fn busy_wings() -> f32 {
    let (left, right) = model::rest(&Feed {
        activities: vec![working(1), working(2)],
        ..Feed::default()
    })
    .wings();
    (left + right) as f32
}

fn working(id: i64) -> Activity {
    Activity {
        id,
        provider: "claude".into(),
        title: "Refactor auth".into(),
        step: "Reading files".into(),
        started_at: Some(0),
        needs_you: false,
        done: false,
        agent: None,
        steps: None,
        face: None,
    }
}

fn approval() -> Ask {
    Ask {
        session: 7,
        request: "r1".into(),
        provider: "codex".into(),
        title: "Fix the build".into(),
        label: "cargo test".into(),
        approval: true,
    }
}

#[gpui::test]
fn the_quiet_notch_is_the_camera_and_work_grows_wings(cx: &mut TestAppContext) {
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    leave(visual);
    // The housing plus the shoulders that curve out to the top edge.
    assert_eq!(size(opened.window, visual), (186. + 2. * SHOULDER, 37.));
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(
                Feed {
                    activities: vec![working(1), working(2)],
                    ..Feed::default()
                },
                cx,
            )
        })
        .unwrap();
    leave(visual);
    assert_eq!(
        size(opened.window, visual),
        (186. + busy_wings() + 2. * SHOULDER, 37.)
    );
    // Under the pointer it swells a little, to say it opens.
    let notch = visual.debug_bounds("notch").unwrap();
    visual.simulate_mouse_move(notch.center(), None, Default::default());
    visual.run_until_parked();
    assert_eq!(
        size(opened.window, visual),
        (186. + busy_wings() + 2. * SHOULDER + PEEK, 40.)
    );
}

#[gpui::test]
fn an_approval_hangs_under_the_notch_and_is_answered_there(cx: &mut TestAppContext) {
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(
                Feed {
                    asks: vec![approval()],
                    ..Feed::default()
                },
                cx,
            )
        })
        .unwrap();
    visual.run_until_parked();
    assert!(visual.debug_bounds("notch-notice").is_some());
    let (_, height) = size(opened.window, visual);
    assert!(height > 37., "the notice hangs below the camera");
    click(visual, "notch-allow");
    let events = opened.events.borrow();
    assert!(matches!(
        events.as_slice(),
        [NotchEvent::Decide { session: 7, request, decision: Decision::Allow }] if request == "r1"
    ));
    drop(events);
    opened
        .window
        .update(visual, |notch, _, _| assert!(notch.notice().is_none()))
        .unwrap();
}

#[gpui::test]
fn hovering_opens_the_shelf_and_leaving_closes_it(cx: &mut TestAppContext) {
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    visual.run_until_parked();
    let notch = visual.debug_bounds("notch").unwrap();
    visual.simulate_mouse_move(notch.center(), None, Default::default());
    visual.executor().advance_clock(OPEN_DELAY * 2);
    visual.run_until_parked();
    assert!(opened.window.update(visual, |n, _, _| n.open).unwrap());
    assert!(visual.debug_bounds("notch-shelf").is_some());
    assert!(visual.debug_bounds("notch-widget-agents").is_some());
    assert!(visual.debug_bounds("notch-widget-clock").is_some());
    // Out of the window: it folds back after a moment.
    visual.simulate_mouse_move(gpui::point(px(-50.), px(-50.)), None, Default::default());
    visual.executor().advance_clock(CLOSE_DELAY * 2);
    visual.run_until_parked();
    assert!(!opened.window.update(visual, |n, _, _| n.open).unwrap());
}

#[gpui::test]
fn the_pencil_arranges_the_shelf_and_saves_each_change(cx: &mut TestAppContext) {
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    opened
        .window
        .update(visual, |notch, _, cx| notch.set_open(true, cx))
        .unwrap();
    click(visual, "notch-edit");
    assert!(visual.debug_bounds("notch-catalog").is_some());
    // Both rows are full: taking the clock out makes room for a widget.
    assert!(visual.debug_bounds("notch-add-call").is_none());
    click(visual, "notch-remove-clock");
    assert!(visual.debug_bounds("notch-widget-clock").is_none());
    click(visual, "notch-add-call");
    // Moving is a drag: the call, picked up and dropped on New chat, goes
    // where New chat was.
    let from = visual.debug_bounds("notch-widget-call").unwrap().center();
    let to = visual.debug_bounds("notch-widget-new").unwrap().center();
    visual.simulate_mouse_down(from, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(
        from + gpui::point(px(12.), px(0.)),
        Some(gpui::MouseButton::Left),
        Default::default(),
    );
    visual.simulate_mouse_move(to, Some(gpui::MouseButton::Left), Default::default());
    visual.simulate_mouse_up(to, gpui::MouseButton::Left, Default::default());
    visual.run_until_parked();
    let look = opened
        .window
        .update(visual, |n, _, _| n.look.clone())
        .unwrap();
    let ids: Vec<_> = look.shelf.iter().map(|w| w.id.as_str()).collect();
    assert_eq!(ids, ["agents", "recent", "call", "new"]);
    // Every change was saved, and the last one is what the notch shows.
    let saved = opened.saved.borrow();
    assert_eq!(saved.len(), 3);
    assert_eq!(model::read(saved.last().map(String::as_str)), look);
}

#[gpui::test]
fn the_notch_has_its_own_settings_and_becomes_an_island(cx: &mut TestAppContext) {
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    opened
        .window
        .update(visual, |notch, _, cx| notch.set_open(true, cx))
        .unwrap();
    click(visual, "notch-tab-settings");
    assert!(visual.debug_bounds("notch-settings-page").is_some());
    click(visual, "notch-style-island");
    click(visual, "notch-opens-click");
    click(visual, "notch-page-widgets");
    click(visual, "notch-rows-1");
    let look = opened
        .window
        .update(visual, |n, _, _| n.look.clone())
        .unwrap();
    assert!(look.island() && look.opens_on_click());
    assert_eq!(look.shelf_rows, 1);
    assert_eq!(model::rows(&look.shelf), 1, "what no longer fits went");
    // Back to the widgets, then closed: an island at rest, no shoulders.
    click(visual, "notch-tab-home");
    assert!(visual.debug_bounds("notch-shelf").is_some());
    opened
        .window
        .update(visual, |notch, _, cx| notch.set_open(false, cx))
        .unwrap();
    leave(visual);
    assert_eq!(size(opened.window, visual), model::ISLAND_REST);
}

#[gpui::test]
fn opening_on_click_ignores_the_pointer_passing_over(cx: &mut TestAppContext) {
    let look = Look {
        opens_on: "click".into(),
        ..Look::default()
    };
    let opened = open_notch(look, cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    visual.run_until_parked();
    let notch = visual.debug_bounds("notch").unwrap();
    visual.simulate_mouse_move(notch.center(), None, Default::default());
    visual.executor().advance_clock(OPEN_DELAY * 4);
    visual.run_until_parked();
    assert!(!opened.window.update(visual, |n, _, _| n.open).unwrap());
    click(visual, "notch");
    assert!(opened.window.update(visual, |n, _, _| n.open).unwrap());
}

#[gpui::test]
fn shelf_widgets_open_sessions_and_start_chats(cx: &mut TestAppContext) {
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(
                Feed {
                    activities: vec![working(4)],
                    ..Feed::default()
                },
                cx,
            );
            notch.set_open(true, cx);
        })
        .unwrap();
    click(visual, "notch-new-chat");
    visual.run_until_parked();
    let agent_row = visual.debug_bounds("notch-widget-agents").unwrap();
    // The first row of the list, under the widget's label.
    let row = gpui::point(agent_row.origin.x + px(40.), agent_row.origin.y + px(34.));
    visual.simulate_click(row, Default::default());
    visual.run_until_parked();
    let events = opened.events.borrow();
    assert!(matches!(
        events.as_slice(),
        [NotchEvent::NewChat, NotchEvent::OpenSession(4)]
    ));
}

#[gpui::test]
fn the_pages_list_chats_and_the_chat_page_asks_an_agent(cx: &mut TestAppContext) {
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(
                Feed {
                    activities: vec![working(4)],
                    recent: vec![monocode_core::notch::Recent {
                        id: 9,
                        provider: "codex".into(),
                        title: "Ship the notch".into(),
                        updated_at: 0,
                    }],
                    ..Feed::default()
                },
                cx,
            );
            notch.set_open(true, cx);
        })
        .unwrap();
    visual.run_until_parked();
    // Home shows the last chats beside the agents at work.
    assert!(visual.debug_bounds("notch-widget-recent").is_some());
    click(visual, "notch-tab-agents");
    assert!(visual.debug_bounds("notch-agents-page").is_some());
    click(visual, "notch-tab-chat");
    assert!(visual.debug_bounds("notch-chat-page").is_some());
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.field.update(cx, |field, cx| {
                field.set_text("Explain the notch".into(), cx)
            })
        })
        .unwrap();
    click(visual, "notch-ask");
    let events = opened.events.borrow();
    assert!(matches!(
        events.as_slice(),
        [NotchEvent::Ask(prompt)] if prompt == "Explain the notch"
    ));
    drop(events);
    // Asking folds the panel and clears the field.
    let (open, typed) = opened
        .window
        .update(visual, |notch, _, cx| {
            (notch.open, notch.field.read(cx).content.to_string())
        })
        .unwrap();
    assert!(!open && typed.is_empty());
}

#[gpui::test]
fn the_widgets_page_shows_hides_and_widens_each_widget(cx: &mut TestAppContext) {
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    opened
        .window
        .update(visual, |notch, _, cx| notch.set_open(true, cx))
        .unwrap();
    click(visual, "notch-tab-settings");
    click(visual, "notch-page-widgets");
    // Make room, then show the reminders widget and widen it.
    click(visual, "notch-rows-3");
    click(visual, "notch-show-today");
    click(visual, "notch-span-today-2");
    click(visual, "notch-show-clock");
    let look = opened
        .window
        .update(visual, |n, _, _| n.look.clone())
        .unwrap();
    let today = look.shelf.iter().find(|w| w.id == "today").unwrap();
    assert_eq!(today.span, 2);
    assert!(look.shelf.iter().all(|w| w.id != "clock"), "hidden");
    click(visual, "notch-page-look");
    click(visual, "notch-edge-off");
    click(visual, "notch-page-general");
    click(visual, "notch-width-wide");
    let look = opened
        .window
        .update(visual, |n, _, _| n.look.clone())
        .unwrap();
    assert!(!look.edge);
    assert_eq!(look.width, "wide");
}

#[gpui::test]
fn several_agents_spread_the_wings_with_the_leads_progress(cx: &mut TestAppContext) {
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    let mut lead = working(1);
    lead.steps = Some((2, 6));
    let feed = Feed {
        activities: vec![lead, working(2), working(3), working(4)],
        ..Feed::default()
    };
    let rest = model::rest(&feed);
    opened
        .window
        .update(visual, |notch, _, cx| notch.update_feed(feed, cx))
        .unwrap();
    leave(visual);
    // Four Claude Code sessions: one Claude mark and "+3" on the left; the
    // lead's state and 2/6 on the right, the wings even about the camera.
    let (left, right) = rest.wings();
    assert_eq!(rest.marks.len(), 1);
    assert_eq!(rest.more(), 3);
    assert_eq!(
        size(opened.window, visual),
        (186. + (left + right) as f32 + 2. * SHOULDER, 37.)
    );
}

#[gpui::test]
fn a_volume_change_opens_the_wings_for_a_moment(cx: &mut TestAppContext) {
    use monocode_core::notch::system::Level;
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    leave(visual);
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.show_level(Level::Volume(0.4, false), cx)
        })
        .unwrap();
    visual.run_until_parked();
    assert!(visual.debug_bounds("notch-level").is_some());
    assert_eq!(
        size(opened.window, visual),
        (186. + 2. * (levels::LEVEL_WING as f32 + SHOULDER), 37.)
    );
    // It folds back by itself.
    visual.executor().advance_clock(Duration::from_secs(3));
    visual.run_until_parked();
    assert!(visual.debug_bounds("notch-level").is_none());
    assert_eq!(size(opened.window, visual), (186. + 2. * SHOULDER, 37.));
}

#[gpui::test]
fn what_plays_glides_past_at_rest_and_leads_home_with_its_player(cx: &mut TestAppContext) {
    use monocode_core::notch::media::{Lyrics, Playing, Track, parse_lrc};
    let look = Look {
        shelf: vec![monocode_core::notch::Widget::new("calendar", 3)],
        ..Look::default()
    };
    let opened = open_notch(look, cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    leave(visual);
    let track = Track {
        app: "Spotify.exe".into(),
        title: "Song".into(),
        artist: "Artist".into(),
        album: "Album".into(),
        duration: Some(Duration::from_secs(200)),
    };
    let playing = |playing| Playing {
        track: track.clone(),
        playing,
        position: Duration::from_secs(12),
        at: Instant::now(),
        art: None,
        shuffle: None,
    };
    opened
        .window
        .update(visual, |notch, _, cx| {
            // The lyrics are in hand already: nothing is fetched in a test.
            notch.lyrics_for = Some(track.clone());
            notch.lyrics = Some((
                track.clone(),
                Lyrics::Synced(parse_lrc("[00:10.00]first line\n[00:20.00]second line")),
            ));
            notch.set_media(Some(playing(true)), cx);
            assert_eq!(
                notch.lyric_moment().map(|(_, line, _)| line).as_deref(),
                Some("first line")
            );
        })
        .unwrap();
    visual.run_until_parked();
    // At rest the notch grows wings for the cover and the title while it plays.
    assert!(visual.debug_bounds("notch-media").is_some());
    assert_eq!(
        size(opened.window, visual),
        (186. + 2. * (media::MEDIA_WING as f32 + SHOULDER), 37.)
    );
    // Open, the player leads Home, above the shelf and its calendar.
    opened
        .window
        .update(visual, |notch, _, cx| notch.set_open(true, cx))
        .unwrap();
    visual.run_until_parked();
    assert!(visual.debug_bounds("notch-player").is_some());
    assert!(visual.debug_bounds("notch-widget-calendar").is_some());
    assert!(visual.debug_bounds("notch-media-play").is_some());
    assert!(visual.debug_bounds("notch-media-next").is_some());
    // Paused, the closed notch goes quiet again.
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.set_open(false, cx);
            notch.set_media(Some(playing(false)), cx);
        })
        .unwrap();
    visual.run_until_parked();
    assert!(visual.debug_bounds("notch-media").is_none());
    assert_eq!(size(opened.window, visual), (186. + 2. * SHOULDER, 37.));
}

#[gpui::test]
fn agents_under_the_music_take_the_left_wing_and_keep_the_width(cx: &mut TestAppContext) {
    use monocode_core::notch::media::{Playing, Track};
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    leave(visual);
    let playing = Playing {
        track: Track {
            app: "Spotify.exe".into(),
            title: "Song".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            duration: Some(Duration::from_secs(200)),
        },
        playing: true,
        position: Duration::from_secs(12),
        at: Instant::now(),
        art: None,
        shuffle: None,
    };
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.lyrics_for = Some(playing.track.clone());
            notch.set_media(Some(playing), cx);
        })
        .unwrap();
    visual.run_until_parked();
    let music_only = size(opened.window, visual);
    assert_eq!(
        music_only,
        (186. + 2. * (media::MEDIA_WING as f32 + SHOULDER), 37.)
    );
    // An agent starts: the notch keeps its width, the state on the left of
    // the camera and the music on its right, the two wings alike.
    let feed = |step: &str| Feed {
        activities: vec![Activity {
            step: step.into(),
            ..working(1)
        }],
        ..Feed::default()
    };
    for step in ["Reading files", "Delegating to an agent", "Editing main.rs"] {
        opened
            .window
            .update(visual, |notch, _, cx| notch.update_feed(feed(step), cx))
            .unwrap();
        visual.run_until_parked();
        assert_eq!(size(opened.window, visual), music_only, "{step}");
        let state = visual.debug_bounds("notch-state-wing").unwrap();
        let music = visual.debug_bounds("notch-music-wing").unwrap();
        assert_eq!(state.size.width, music.size.width);
        assert!(state.right() < music.left());
    }
    // The music stops: the agents' own width, the same for every state.
    opened
        .window
        .update(visual, |notch, _, cx| notch.set_media(None, cx))
        .unwrap();
    visual.run_until_parked();
    assert!(visual.debug_bounds("notch-music-wing").is_none());
    assert_eq!(
        size(opened.window, visual),
        (186. + 2. * (model::AGENT_WING as f32 + SHOULDER), 37.)
    );
}

fn open_music_bar(cx: &mut TestAppContext) -> Opened {
    use monocode_core::notch::media::{Playing, Track};
    let opened = open_notch(
        Look {
            finished_notices: false,
            lyrics: false,
            ..Look::default()
        },
        cx,
    );
    opened
        .window
        .update(cx, |notch, _, cx| {
            notch.screen_override = Some(Screen {
                safe_top: 0.,
                ..macbook()
            });
            notch.set_media(
                Some(Playing {
                    track: Track {
                        app: "Spotify.exe".into(),
                        title: "Four Seasons".into(),
                        artist: "Digitalism".into(),
                        album: String::new(),
                        duration: Some(Duration::from_secs(200)),
                    },
                    playing: true,
                    position: Duration::from_secs(12),
                    at: Instant::now(),
                    art: Some(std::sync::Arc::from(
                        include_bytes!("../../../../assets/icons/icon.png").as_slice(),
                    )),
                    shuffle: None,
                }),
                cx,
            );
            notch.update_feed(
                Feed {
                    activities: vec![working(1)],
                    ..Feed::default()
                },
                cx,
            );
        })
        .unwrap();
    opened
}

fn music_frame(
    opened: &Opened,
    visual: &mut VisualTestContext,
    elapsed: Duration,
) -> (Bounds<Pixels>, Bounds<Pixels>) {
    visual.executor().advance_clock(elapsed);
    opened
        .window
        .update(visual, |_, window, cx| {
            window.simulate_next_frame(cx);
            window.refresh();
        })
        .unwrap();
    visual.run_until_parked();
    (
        visual.debug_bounds("notch-music-cover").unwrap(),
        visual.debug_bounds("notch-music-wave").unwrap(),
    )
}

fn finish_music_agent(opened: &Opened, visual: &mut VisualTestContext) {
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(
                Feed {
                    activities: vec![Activity {
                        done: true,
                        ..working(1)
                    }],
                    ..Feed::default()
                },
                cx,
            )
        })
        .unwrap();
    visual.run_until_parked();
}

#[gpui::test]
fn finishing_the_agent_carries_one_cover_left_in_200ms(cx: &mut TestAppContext) {
    let opened = open_music_bar(cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    leave(visual);
    let busy = music_frame(&opened, visual, Duration::ZERO);
    let island = size(opened.window, visual);
    let art = opened
        .window
        .update(visual, |notch, _, _| notch.art.as_ref().unwrap().1.clone())
        .unwrap();
    assert_eq!(busy.0.size.width, px(16.));
    // While the agent works the music keeps the right half, past the
    // hairline: its cover, then the lyric or title, then the waveform.
    let bar = visual.debug_bounds("notch-media").unwrap();
    let divider = visual.debug_bounds("notch-music-divider").unwrap();
    let agents = visual.debug_bounds("notch-music-agents").unwrap();
    assert!(agents.right() <= divider.left());
    assert_eq!(busy.0.left(), divider.left() + px(8.));
    assert!((divider.left() - (bar.left() + bar.size.width / 2.)).abs() <= px(1.));
    let line = visual.debug_bounds("notch-music-title").unwrap();
    assert_eq!(line.left(), busy.0.right() + px(8.));
    assert_eq!(line.right() + px(8.), busy.1.left());
    visual.update(|_, cx| cx.set_reduce_motion(false));
    finish_music_agent(&opened, visual);
    assert_eq!(music_frame(&opened, visual, Duration::ZERO), busy);

    let mut previous = busy.0.left();
    for _ in 0..12 {
        let (cover, wave) = music_frame(&opened, visual, Duration::from_millis(16));
        assert!(cover.left() < previous, "the cover glides, never teleports");
        assert_eq!(wave, busy.1, "the waveform never jumps or duplicates");
        assert_eq!(size(opened.window, visual), island);
        assert!(cover.size.width > px(16.) && cover.size.width <= px(20.));
        opened
            .window
            .update(visual, |notch, _, _| {
                assert!(std::sync::Arc::ptr_eq(&art, &notch.art.as_ref().unwrap().1));
            })
            .unwrap();
        previous = cover.left();
    }
    let (cover, wave) = music_frame(&opened, visual, Duration::from_millis(8));
    let bar = visual.debug_bounds("notch-media").unwrap();
    assert_eq!(cover.left(), bar.left() + px(6.));
    assert_eq!(cover.size.width, px(20.));
    assert_eq!(wave, busy.1);
    assert!(visual.debug_bounds("notch-music-agents").is_none());
    assert!(visual.debug_bounds("notch-music-divider").is_none());
    let title = visual.debug_bounds("notch-music-title").unwrap();
    assert_eq!(title.left(), cover.right() + px(8.));
    assert_eq!(title.right() + px(8.), wave.left());
    assert_eq!(
        music_frame(&opened, visual, crate::motion::QUICK),
        (cover, wave),
        "the transition ends instead of continuing to drift"
    );
}

#[gpui::test]
fn another_agent_reverses_the_music_from_its_current_position(cx: &mut TestAppContext) {
    let opened = open_music_bar(cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    leave(visual);
    let busy = music_frame(&opened, visual, Duration::ZERO);
    visual.update(|_, cx| cx.set_reduce_motion(false));
    finish_music_agent(&opened, visual);
    let midway = music_frame(&opened, visual, Duration::from_millis(64));
    assert!(midway.0.left() < busy.0.left());
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(
                Feed {
                    activities: vec![working(2), working(3)],
                    ..Feed::default()
                },
                cx,
            );
            let mut playing = notch.media.clone().unwrap();
            playing.track.title = "A different track".into();
            notch.set_media(Some(playing), cx);
        })
        .unwrap();
    assert_eq!(music_frame(&opened, visual, Duration::ZERO), midway);
    let returning = music_frame(&opened, visual, Duration::from_millis(32));
    assert!(returning.0.left() > midway.0.left());
    assert!(returning.0.left() < busy.0.left());
    // One agent finishes, but the other still needs the left side.
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(
                Feed {
                    activities: vec![working(3)],
                    ..Feed::default()
                },
                cx,
            )
        })
        .unwrap();
    assert_eq!(music_frame(&opened, visual, Duration::ZERO), returning);
    assert_eq!(music_frame(&opened, visual, crate::motion::QUICK), busy);
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(Feed::default(), cx)
        })
        .unwrap();
    music_frame(&opened, visual, Duration::ZERO);
    let (cover, _) = music_frame(&opened, visual, crate::motion::QUICK);
    assert_eq!(cover.size.width, px(20.));
}

#[gpui::test]
fn reduced_motion_finishes_the_music_move_immediately(cx: &mut TestAppContext) {
    let opened = open_music_bar(cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    leave(visual);
    visual.update(|_, cx| cx.set_reduce_motion(false));
    finish_music_agent(&opened, visual);
    music_frame(&opened, visual, Duration::from_millis(48));
    visual.update(|_, cx| cx.set_reduce_motion(true));
    let resting = music_frame(&opened, visual, Duration::ZERO);
    assert_eq!(resting.0.size.width, px(20.));
    assert!(visual.debug_bounds("notch-music-agents").is_none());
    // Turning motion back on must not replay an unfinished move.
    visual.update(|_, cx| cx.set_reduce_motion(false));
    assert_eq!(music_frame(&opened, visual, Duration::ZERO), resting);
}

#[gpui::test]
fn interrupted_music_moves_leave_no_stale_cover(cx: &mut TestAppContext) {
    for interrupt in 0..4 {
        let opened = open_music_bar(cx);
        let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
        leave(visual);
        visual.update(|_, cx| cx.set_reduce_motion(false));
        finish_music_agent(&opened, visual);
        music_frame(&opened, visual, Duration::from_millis(48));
        opened
            .window
            .update(visual, |notch, _, cx| match interrupt {
                0 => notch.set_open(true, cx),
                1 => {
                    notch.level = Some(monocode_core::notch::system::Level::Volume(0.5, false));
                    cx.notify();
                }
                2 => notch.set_media(None, cx),
                _ => notch.update_feed(
                    Feed {
                        asks: vec![approval()],
                        ..Feed::default()
                    },
                    cx,
                ),
            })
            .unwrap();
        visual.run_until_parked();
        assert!(visual.debug_bounds("notch-music-cover").is_none());
        opened
            .window
            .update(visual, |notch, _, _| {
                assert!(notch.music_motion.is_none());
            })
            .unwrap();
    }
}

#[gpui::test]
fn agents_lists_the_users_own_agents_and_chat_the_coding_chats(cx: &mut TestAppContext) {
    use monocode_core::notch::{Face, Personal, Recent};
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    let mut agent_chat = working(20);
    agent_chat.agent = Some("Ivy".into());
    agent_chat.face = Some(Face {
        mascot: "ivy".into(),
        color: 0x7c5cff,
    });
    let agent_ask = Ask {
        session: 20,
        ..approval()
    };
    let code_ask = Ask {
        session: 4,
        request: "r2".into(),
        ..approval()
    };
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(
                Feed {
                    agents: vec![Personal {
                        name: "Ivy".into(),
                        face: Face {
                            mascot: "ivy".into(),
                            color: 0x7c5cff,
                        },
                        session: Some(20),
                        doing: Some("Reading files".into()),
                        needs_you: true,
                        coding: false,
                    }],
                    activities: vec![working(4), agent_chat],
                    asks: vec![agent_ask, code_ask],
                    recent: vec![Recent {
                        id: 9,
                        provider: "codex".into(),
                        title: "Ship the notch".into(),
                        updated_at: 0,
                    }],
                    ..Feed::default()
                },
                cx,
            );
            notch.set_open(true, cx);
        })
        .unwrap();
    visual.run_until_parked();
    opened
        .window
        .update(visual, |notch, _, _| {
            // The agent's approval is the Agents page's, the coding one the
            // Chat page's, and each page's work is its own.
            let sessions = |asks: Vec<Ask>| asks.iter().map(|a| a.session).collect::<Vec<_>>();
            assert_eq!(sessions(notch.asks_of(true)), [20]);
            assert_eq!(sessions(notch.asks_of(false)), [4]);
            let ids = |work: Vec<Activity>| work.iter().map(|a| a.id).collect::<Vec<_>>();
            assert_eq!(ids(notch.coding_work(false)), [4]);
            // Ivy's own chat is Ivy's row, not another agent at work.
            assert!(notch.other_agent_work().is_empty());
        })
        .unwrap();
    click(visual, "notch-tab-agents");
    assert!(visual.debug_bounds("notch-agents-page").is_some());
    click(visual, "notch-tab-chat");
    assert!(visual.debug_bounds("notch-chat-page").is_some());
}

#[gpui::test]
fn the_clipboard_page_keeps_what_was_copied_a_click_from_copying_again(cx: &mut TestAppContext) {
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.copied("cargo xtask dev", cx);
            notch.copied("https://usemono.dev", cx);
            notch.set_open(true, cx);
        })
        .unwrap();
    click(visual, "notch-tab-clips");
    assert!(visual.debug_bounds("notch-clips-page").is_some());
    // The older copy, copied again: it is on the clipboard and the newest.
    click(visual, "notch-clip-1");
    assert_eq!(
        visual.read_from_clipboard().and_then(|item| item.text()),
        Some("cargo xtask dev".into())
    );
    let texts = |notch: &Notch| {
        notch
            .clips
            .clips()
            .iter()
            .map(|clip| clip.text.clone())
            .collect::<Vec<_>>()
    };
    let kept = opened.window.update(visual, |notch, _, _| texts(notch));
    assert_eq!(kept.unwrap(), ["cargo xtask dev", "https://usemono.dev"]);
    // Its cross removes one without copying it.
    click(visual, "notch-clip-remove-0");
    let kept = opened.window.update(visual, |notch, _, _| texts(notch));
    assert_eq!(kept.unwrap(), ["https://usemono.dev"]);
    click(visual, "notch-clips-clear");
    assert!(visual.debug_bounds("notch-clip-0").is_none());
    // Turned off, the page goes and nothing is kept.
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.copied("kept for now", cx);
            notch.change_look(|look| look.clipboard = false, cx);
            notch.copied("not kept", cx);
            assert!(notch.clips.clips().is_empty());
            assert_eq!(notch.tab, Tab::Home);
        })
        .unwrap();
    visual.run_until_parked();
    assert!(visual.debug_bounds("notch-tab-clips").is_none());
}

/// The card above the list shows one copy large: the newest, or the one the
/// pointer is on. What can be done with it depends on what it is.
#[gpui::test]
fn the_clipboard_card_shows_the_copy_pointed_at_and_what_to_do_with_it(cx: &mut TestAppContext) {
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.copied("#ff8800", cx);
            notch.copied("fn main() {\n    run();\n}", cx);
            notch.copied("https://docs.rs/gpui/latest", cx);
            notch.set_open(true, cx);
        })
        .unwrap();
    click(visual, "notch-tab-clips");
    let shown = |visual: &mut VisualTestContext| {
        visual.run_until_parked();
        opened
            .window
            .update(visual, |notch, _, _| notch.shown_clip())
            .unwrap()
    };
    let point_at = |visual: &mut VisualTestContext, row: &'static str| {
        let bounds = visual.debug_bounds(row).expect("the row is drawn");
        visual.simulate_mouse_move(bounds.center(), None, Default::default());
        visual.run_until_parked();
    };
    // The newest is shown without pointing at anything: a link, to open.
    let card = visual.debug_bounds("notch-clip-card").expect("the card");
    assert_eq!(shown(visual), Some(0));
    assert!(visual.debug_bounds("notch-clip-open").is_some());
    // The rows are under the card, which keeps its place and its size.
    let first = visual.debug_bounds("notch-clip-0").unwrap();
    assert!(first.top() >= card.bottom());
    // Pointing at the code shows it: copied again from the card.
    point_at(visual, "notch-clip-1");
    assert_eq!(shown(visual), Some(1));
    assert!(visual.debug_bounds("notch-clip-open").is_none());
    assert_eq!(visual.debug_bounds("notch-clip-card").unwrap(), card);
    click(visual, "notch-clip-copy");
    assert_eq!(
        visual.read_from_clipboard().and_then(|item| item.text()),
        Some("fn main() {\n    run();\n}".into())
    );
    // It became the newest, and the card went with it.
    assert_eq!(shown(visual), Some(0));
    let newest = opened
        .window
        .update(visual, |notch, _, _| notch.clips.clips()[0].text.clone())
        .unwrap();
    assert!(newest.starts_with("fn main"));
    // A color is copied the way it will be written.
    point_at(visual, "notch-clip-2");
    assert_eq!(shown(visual), Some(2));
    assert!(visual.debug_bounds("notch-clip-copy").is_none());
    click(visual, "notch-clip-format-1");
    assert_eq!(
        visual.read_from_clipboard().and_then(|item| item.text()),
        Some("rgb(255, 136, 0)".into())
    );
    // Removing the copy shown leaves the card on the newest.
    click(visual, "notch-clip-remove-2");
    assert_eq!(shown(visual), Some(0));
    // A new copy takes the card.
    point_at(visual, "notch-clip-1");
    assert_eq!(shown(visual), Some(1));
    opened
        .window
        .update(visual, |notch, _, cx| notch.copied("ana@ejemplo.test", cx))
        .unwrap();
    assert_eq!(shown(visual), Some(0));
    assert!(visual.debug_bounds("notch-clip-write").is_some());
    click(visual, "notch-clips-clear");
    assert!(visual.debug_bounds("notch-clip-card").is_none());
}

#[gpui::test]
fn the_battery_shows_on_the_wings_like_a_level(cx: &mut TestAppContext) {
    use monocode_core::notch::system::Level;
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    leave(visual);
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.show_level(Level::Power(0.18, false), cx)
        })
        .unwrap();
    visual.run_until_parked();
    assert!(visual.debug_bounds("notch-level").is_some());
}

#[gpui::test]
fn the_island_wears_a_face_at_rest_unless_turned_off(cx: &mut TestAppContext) {
    let island = Look {
        style: "island".into(),
        ..Look::default()
    };
    let opened = open_notch(island, cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    leave(visual);
    opened
        .window
        .update(visual, |notch, window, cx| {
            // Under reduced motion the eyes are open and still.
            notch.blink(cx);
            let eyes = notch.eyes(window, cx).expect("a face");
            assert_eq!((eyes.open, eyes.gaze, eyes.glad), (1., 0., false));
            // A finished turn makes it glad; what was done already did not.
            let done = Activity {
                done: true,
                ..working(1)
            };
            notch.update_feed(
                Feed {
                    activities: vec![done.clone()],
                    ..Feed::default()
                },
                cx,
            );
            assert!(!notch.eyes(window, cx).unwrap().glad);
            notch.update_feed(
                Feed {
                    activities: vec![
                        done,
                        Activity {
                            done: true,
                            ..working(2)
                        },
                    ],
                    ..Feed::default()
                },
                cx,
            );
            assert!(notch.eyes(window, cx).unwrap().glad);
            notch.change_look(|look| look.face = false, cx);
            assert!(notch.eyes(window, cx).is_none());
        })
        .unwrap();
    // The cheer passes by itself.
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.change_look(|look| look.face = true, cx);
            notch.cheer(cx);
        })
        .unwrap();
    visual.executor().advance_clock(Duration::from_secs(5));
    visual.run_until_parked();
    opened
        .window
        .update(visual, |notch, window, cx| {
            assert!(!notch.eyes(window, cx).unwrap().glad)
        })
        .unwrap();
}

fn ivy() -> monocode_core::notch::Personal {
    monocode_core::notch::Personal {
        name: "Ivy".into(),
        face: monocode_core::notch::Face {
            mascot: "ivy".into(),
            color: 0x7c5cff,
        },
        session: Some(20),
        doing: None,
        needs_you: false,
        coding: false,
    }
}

#[gpui::test]
fn an_agents_row_opens_its_chat_in_the_notch_and_sends_without_the_app(cx: &mut TestAppContext) {
    use monocode_core::notch::{Said, Talk};
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    let feed = |talk: Option<Talk>| Feed {
        agents: vec![ivy()],
        talk,
        ..Feed::default()
    };
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(feed(None), cx);
            notch.set_open(true, cx);
        })
        .unwrap();
    click(visual, "notch-tab-agents");
    click(visual, "notch-agent-0");
    // The chat is here, and the app was only asked for what was said.
    assert!(visual.debug_bounds("notch-talk").is_some());
    assert!(visual.debug_bounds("notch-agents-page").is_none());
    assert!(matches!(
        opened.events.borrow().as_slice(),
        [NotchEvent::Watch(Some(20))]
    ));
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(
                feed(Some(Talk {
                    session: 20,
                    said: vec![
                        Said {
                            mine: true,
                            text: "What changed today?".into(),
                            files: Vec::new(),
                        },
                        Said {
                            mine: false,
                            text: "Two files in the shell.".into(),
                            files: Vec::new(),
                        },
                    ],
                    busy: false,
                })),
                cx,
            );
        })
        .unwrap();
    // Typed as the user types: into the focused field, keys and all.
    click(visual, "notch-talk-field");
    visual.simulate_input("  Thanks  ");
    visual.run_until_parked();
    let typed = opened
        .window
        .update(visual, |notch, _, cx| {
            notch.talk_field.read(cx).content.to_string()
        })
        .unwrap();
    assert_eq!(typed, "  Thanks  ");
    click(visual, "notch-talk-send");
    assert!(matches!(
        opened.events.borrow().as_slice(),
        [_, NotchEvent::Tell { session: 20, text }] if text == "Thanks"
    ));
    // Sent: the field is clear and the panel stays open on the chat.
    let (open, typed) = opened
        .window
        .update(visual, |notch, _, cx| {
            (notch.open, notch.talk_field.read(cx).content.to_string())
        })
        .unwrap();
    assert!(open && typed.is_empty());
    // What could not be delivered comes back with why.
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.tell_failed("Thanks".into(), "The agent is busy.".into(), cx)
        })
        .unwrap();
    visual.run_until_parked();
    assert!(visual.debug_bounds("notch-talk-error").is_some());
    let typed = opened
        .window
        .update(visual, |notch, _, cx| {
            notch.talk_field.read(cx).content.to_string()
        })
        .unwrap();
    assert_eq!(typed, "Thanks");
    // Closed, the chat is no longer followed; open again, it is.
    opened
        .window
        .update(visual, |notch, _, cx| notch.set_open(false, cx))
        .unwrap();
    assert!(matches!(
        opened.events.borrow().last(),
        Some(NotchEvent::Watch(None))
    ));
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.set_open(true, cx);
            notch.set_tab(Tab::Agents, cx);
        })
        .unwrap();
    visual.run_until_parked();
    assert!(matches!(
        opened.events.borrow().last(),
        Some(NotchEvent::Watch(Some(20)))
    ));
    assert!(visual.debug_bounds("notch-talk").is_some());
    // Back to the agents; Open in app is the one way that raises the app.
    click(visual, "notch-talk-open");
    click(visual, "notch-talk-back");
    assert!(visual.debug_bounds("notch-agents-page").is_some());
    assert!(matches!(
        opened.events.borrow().as_slice(),
        [.., NotchEvent::OpenSession(20), NotchEvent::Watch(None)]
    ));
}

/// With Lyrics off, what plays is not looked up anywhere: no title leaves
/// this Mac for lyrics nobody asked to see.
#[gpui::test]
fn lyrics_are_not_asked_for_while_lyrics_is_off(cx: &mut TestAppContext) {
    use monocode_core::notch::media::{Playing, Track};
    let look = Look {
        lyrics: false,
        ..Look::default()
    };
    let opened = open_notch(look, cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    let track = Track {
        app: "Spotify.exe".into(),
        title: "Song".into(),
        artist: "Artist".into(),
        album: "Album".into(),
        duration: Some(Duration::from_secs(200)),
    };
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.set_media(
                Some(Playing {
                    track: track.clone(),
                    playing: true,
                    position: Duration::from_secs(12),
                    at: Instant::now(),
                    art: None,
                    shuffle: None,
                }),
                cx,
            );
            assert!(notch.lyrics_for.is_none(), "nothing was asked for");
            assert!(notch.lyric_moment().is_none());
            // In hand already (as after turning them on): nothing more is
            // asked for the same track.
            notch.lyrics_for = Some(track.clone());
            notch.change_look(|look| look.lyrics = true, cx);
            assert_eq!(notch.lyrics_for.as_ref(), Some(&track));
        })
        .unwrap();
    visual.run_until_parked();
}

#[test]
fn content_leaves_with_the_fold_only_when_the_shape_becomes_another_kind() {
    // A shorter state word or a shorter page shows at once: no blink.
    assert!(!leaves(
        Shape::Busy {
            left: 73,
            right: 135
        },
        Shape::Busy {
            left: 73,
            right: 96
        }
    ));
    assert!(!leaves(
        Shape::Open { body: 300 },
        Shape::Open { body: 180 }
    ));
    // The panel closing to its wings, or the wings going quiet, fade out.
    assert!(leaves(
        Shape::Open { body: 300 },
        Shape::Busy {
            left: 73,
            right: 96
        }
    ));
    assert!(leaves(
        Shape::Busy {
            left: 73,
            right: 96
        },
        Shape::Quiet
    ));
}

/// A wheel over the chat, in points; negative moves toward the end.
fn wheel(visual: &mut VisualTestContext, at: gpui::Point<Pixels>, delta: f32) {
    visual.simulate_event(gpui::ScrollWheelEvent {
        position: at,
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(delta))),
        modifiers: Default::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    visual.run_until_parked();
}

#[gpui::test]
fn the_chat_with_an_agent_scrolls_and_follows_the_answer_as_it_streams(cx: &mut TestAppContext) {
    use monocode_core::notch::{Said, Talk};
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    let said = |answer: &str| {
        let mut said: Vec<Said> = (0..7)
            .map(|n| Said {
                mine: n % 2 == 0,
                text: format!("Message {n}: {}", "a line of what was said ".repeat(6)),
                files: Vec::new(),
            })
            .collect();
        said.push(Said {
            mine: false,
            text: answer.to_owned(),
            files: Vec::new(),
        });
        said
    };
    let feed = |said: Vec<Said>, busy: bool| Feed {
        agents: vec![ivy()],
        talk: Some(Talk {
            session: 20,
            said,
            busy,
        }),
        ..Feed::default()
    };
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(
                Feed {
                    agents: vec![ivy()],
                    ..Feed::default()
                },
                cx,
            );
            notch.set_open(true, cx);
        })
        .unwrap();
    click(visual, "notch-tab-agents");
    click(visual, "notch-agent-0");
    // A short chat rests on the field, with nothing to scroll.
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(
                Feed {
                    agents: vec![ivy()],
                    talk: Some(Talk {
                        session: 20,
                        said: vec![Said {
                            mine: true,
                            text: "Hi".into(),
                            files: Vec::new(),
                        }],
                        busy: false,
                    }),
                    ..Feed::default()
                },
                cx,
            )
        })
        .unwrap();
    visual.run_until_parked();
    let viewport = visual.debug_bounds("notch-talk-said").unwrap();
    let lines = visual.debug_bounds("notch-talk-lines").unwrap();
    assert_eq!(lines.bottom(), viewport.bottom());
    let (max, _) = scroll_of(&opened, visual);
    assert_eq!(max, px(0.));
    // A long one opens at its end.
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(feed(said("Reading the shell"), true), cx)
        })
        .unwrap();
    visual.run_until_parked();
    let (max, offset) = scroll_of(&opened, visual);
    assert!(
        max > px(100.),
        "the history is taller than the chat: {max:?}"
    );
    assert_eq!(offset, -max);
    // The answer grows while it streams, and the end stays in view.
    let longer = format!("Reading the shell. {}", "More of the answer. ".repeat(20));
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(feed(said(&longer), true), cx)
        })
        .unwrap();
    visual.run_until_parked();
    let (grown, offset) = scroll_of(&opened, visual);
    assert!(grown > max);
    assert_eq!(offset, -grown);
    // Up to read: the first message is reached, and what streams in after
    // leaves the reader where they are.
    let at = viewport.center();
    wheel(visual, at, 10_000.);
    let (_, offset) = scroll_of(&opened, visual);
    assert_eq!(offset, px(0.));
    let first = visual.debug_bounds("notch-talk-lines").unwrap();
    assert_eq!(first.top(), viewport.top());
    wheel(visual, at, -40.);
    let (_, reading) = scroll_of(&opened, visual);
    assert_eq!(reading, px(-40.));
    let longest = format!("{longer} {}", "Still going. ".repeat(20));
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(feed(said(&longest), true), cx)
        })
        .unwrap();
    visual.run_until_parked();
    let (max, offset) = scroll_of(&opened, visual);
    assert!(max > grown);
    assert_eq!(
        offset, reading,
        "streaming must not move a reader who scrolled up"
    );
    assert!(!following(&opened, visual));
    // Back down to the end, and it follows again.
    wheel(visual, at, -10_000.);
    let done = format!("{longest} Done.");
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(feed(said(&done), false), cx)
        })
        .unwrap();
    visual.run_until_parked();
    let (max, offset) = scroll_of(&opened, visual);
    assert_eq!(offset, -max);
    // Up again, then a message sent: the chat goes back to its end.
    wheel(visual, at, 200.);
    assert!(scroll_of(&opened, visual).1 > -max);
    click(visual, "notch-talk-field");
    visual.simulate_input("Thanks");
    click(visual, "notch-talk-send");
    let mut sent = said(&done);
    sent.push(Said {
        mine: true,
        text: "Thanks".into(),
        files: Vec::new(),
    });
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(feed(sent, true), cx)
        })
        .unwrap();
    visual.run_until_parked();
    let (max, offset) = scroll_of(&opened, visual);
    assert_eq!(offset, -max);
    assert!(following(&opened, visual));
    assert!(matches!(
        opened.events.borrow().last(),
        Some(NotchEvent::Tell { session: 20, text }) if text == "Thanks"
    ));
}

/// Whether the chat keeps to its end as more arrives.
fn following(opened: &Opened, visual: &mut VisualTestContext) -> bool {
    opened
        .window
        .update(visual, |notch, _, _| {
            notch
                .talk
                .as_ref()
                .expect("the chat is open")
                .scroll
                .following()
        })
        .unwrap()
}

/// The chat's room to scroll and where it is (negative is further down).
fn scroll_of(opened: &Opened, visual: &mut VisualTestContext) -> (Pixels, Pixels) {
    opened
        .window
        .update(visual, |notch, _, _| {
            let handle = &notch.talk.as_ref().expect("the chat is open").scroll.handle;
            (handle.max_offset().y, handle.offset().y)
        })
        .unwrap()
}

#[gpui::test]
fn a_copied_row_says_so_and_each_kind_wears_its_mark(cx: &mut TestAppContext) {
    use monocode_core::notch::clipboard::Kind;
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    // A 2 × 1 picture at 24 bits, as a BMP file.
    let mut dib = Vec::new();
    dib.extend_from_slice(&40u32.to_le_bytes());
    dib.extend_from_slice(&2i32.to_le_bytes());
    dib.extend_from_slice(&1i32.to_le_bytes());
    dib.extend_from_slice(&1u16.to_le_bytes());
    dib.extend_from_slice(&24u16.to_le_bytes());
    dib.extend_from_slice(&[0; 24]);
    dib.extend_from_slice(&[0, 0, 255, 255, 0, 0, 0, 0]);
    let (bmp, width, height) = monocode_core::notch::clipboard::bmp_from_dib(&dib).unwrap();
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.copied("fn main() {}", cx);
            notch.copied("ana@example.com", cx);
            notch.copied_picture(bmp, width, height, cx);
            notch.set_open(true, cx);
        })
        .unwrap();
    let kinds = opened
        .window
        .update(visual, |notch, _, _| {
            notch
                .clips
                .clips()
                .iter()
                .map(|clip| clip.kind)
                .collect::<Vec<_>>()
        })
        .unwrap();
    assert_eq!(kinds, [Kind::Image(2, 1), Kind::Email, Kind::Code]);
    click(visual, "notch-tab-clips");
    assert!(visual.debug_bounds("notch-clip-copied").is_none());
    // The code, copied again: it is the newest and says it was copied.
    click(visual, "notch-clip-2");
    assert!(visual.debug_bounds("notch-clip-copied").is_some());
    assert_eq!(
        visual.read_from_clipboard().and_then(|item| item.text()),
        Some("fn main() {}".into())
    );
    let first = opened
        .window
        .update(visual, |notch, _, _| notch.clips.clips()[0].kind)
        .unwrap();
    assert_eq!(first, Kind::Code);
    // The word passes by itself.
    visual.executor().advance_clock(Duration::from_millis(1500));
    visual.run_until_parked();
    assert!(visual.debug_bounds("notch-clip-copied").is_none());
    // A picture is copied back as a picture.
    click(visual, "notch-clip-1");
    let item = visual.read_from_clipboard().expect("something was copied");
    assert!(item.text().is_none());
    assert!(matches!(
        item.entries().first(),
        Some(gpui::ClipboardEntry::Image(_))
    ));
}

#[gpui::test]
fn two_players_share_home_in_two_columns(cx: &mut TestAppContext) {
    use monocode_core::notch::media::{Playing, Track};
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    let player = |app: &str, title: &str, playing| Playing {
        track: Track {
            app: app.into(),
            title: title.into(),
            artist: "Someone".into(),
            album: "Album".into(),
            duration: Some(Duration::from_secs(200)),
        },
        playing,
        position: Duration::from_secs(20),
        at: Instant::now(),
        art: None,
        shuffle: None,
    };
    let lead = |visual: &mut VisualTestContext| {
        opened
            .window
            .update(visual, |notch, _, _| {
                notch.media.as_ref().map(|media| media.track.app.clone())
            })
            .unwrap()
    };
    // One player: the large player as ever.
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.set_players(vec![player("browser", "Video", false)], cx);
            notch.set_open(true, cx);
        })
        .unwrap();
    visual.run_until_parked();
    assert!(visual.debug_bounds("notch-media-play").is_some());
    assert!(visual.debug_bounds("notch-duo-lead").is_none());
    // A paused video and Spotify playing: two columns, Spotify leads (the
    // closed notch and the lyrics follow it), on the left.
    let both = vec![
        player("browser", "Video", false),
        player("spotify", "Song", true),
    ];
    opened
        .window
        .update(visual, |notch, _, cx| notch.set_players(both.clone(), cx))
        .unwrap();
    visual.run_until_parked();
    assert_eq!(lead(visual).as_deref(), Some("spotify"));
    assert!(visual.debug_bounds("notch-media-play").is_none());
    let left = visual.debug_bounds("notch-duo-lead").unwrap();
    let right = visual.debug_bounds("notch-duo-beside").unwrap();
    assert!(left.left() < right.left());
    assert_eq!(left.size.height, right.size.height);
    assert!(
        right.bottom() - left.top() <= px(media::PLAYER_HEIGHT + 0.5),
        "as tall as the single player"
    );
    // Each column's buttons go to its own app, and pressing one never
    // moves the lead.
    let (commands, sent) = std::sync::mpsc::channel();
    opened
        .window
        .update(visual, |notch, _, _| notch.media_commands = Some(commands))
        .unwrap();
    click(visual, "notch-duo-beside-next");
    click(visual, "notch-duo-lead-play");
    assert_eq!(
        sent.try_iter().collect::<Vec<_>>(),
        [
            ("browser".to_owned(), platform::MediaCommand::Next),
            ("spotify".to_owned(), platform::MediaCommand::TogglePlay)
        ]
    );
    assert_eq!(lead(visual).as_deref(), Some("spotify"));
    // A click on the other column puts it first; the columns keep their
    // sides, only the lit one moves.
    click(visual, "notch-duo-beside");
    assert_eq!(lead(visual).as_deref(), Some("browser"));
    let lit = visual.debug_bounds("notch-duo-lead").unwrap();
    let other = visual.debug_bounds("notch-duo-beside").unwrap();
    assert!(lit.left() > other.left(), "the video stays on the right");
    // The video goes away: the large player again.
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.media_commands = None;
            notch.set_players(vec![player("spotify", "Song", true)], cx);
        })
        .unwrap();
    visual.run_until_parked();
    assert_eq!(lead(visual).as_deref(), Some("spotify"));
    assert!(visual.debug_bounds("notch-duo-lead").is_none());
    assert!(visual.debug_bounds("notch-media-play").is_some());
}

#[gpui::test]
fn playing_next_lists_spotifys_queue_under_the_player(cx: &mut TestAppContext) {
    use monocode_core::notch::media::{Playing, Track};
    use monocode_core::notch::spotify::Next;
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    let next = |title: &str| Next {
        title: title.into(),
        artist: "Someone".into(),
        uri: format!("spotify:track:{title}"),
        art: Some(format!("https://i.scdn.co/image/{title}")),
    };
    let heights = opened
        .window
        .update(visual, |notch, _, cx| {
            notch.set_media(
                Some(Playing {
                    track: Track {
                        app: "Spotify.exe".into(),
                        title: "Now".into(),
                        artist: "Someone".into(),
                        album: String::new(),
                        duration: None,
                    },
                    playing: false,
                    position: Duration::ZERO,
                    at: Instant::now(),
                    art: None,
                    shuffle: None,
                }),
                cx,
            );
            notch.set_open(true, cx);
            let without = notch.body_height();
            notch.set_snapshot(
                monocode_core::notch::spotify::Snapshot {
                    next: vec![next("One"), next("Two")],
                    ..Default::default()
                },
                cx,
            );
            (without, notch.body_height())
        })
        .unwrap();
    visual.run_until_parked();
    assert!(visual.debug_bounds("notch-player").is_some());
    assert!(visual.debug_bounds("notch-queue").is_some());
    assert!(heights.1 > heights.0, "the panel grows by the queue");
    // A row is a button: choosing the second song skips the first, shows
    // that at once and tells the player "next" twice.
    let (commands, sent) = std::sync::mpsc::channel();
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.media_commands = Some(commands);
            notch.set_snapshot(
                monocode_core::notch::spotify::Snapshot {
                    next: vec![next("One"), next("Two"), next("Three")],
                    ..Default::default()
                },
                cx,
            );
        })
        .unwrap();
    click(visual, "notch-next-1");
    let listed = opened
        .window
        .update(visual, |notch, _, _| {
            notch.media_commands = None;
            notch
                .queue
                .next
                .iter()
                .map(|next| next.title.clone())
                .collect::<Vec<_>>()
        })
        .unwrap();
    assert_eq!(listed, ["Three"]);
    assert_eq!(
        sent.try_iter().collect::<Vec<_>>(),
        [
            ("Spotify.exe".to_owned(), platform::MediaCommand::Next),
            ("Spotify.exe".to_owned(), platform::MediaCommand::Next)
        ]
    );
    assert!(visual.debug_bounds("notch-next-0").is_some());
    assert!(visual.debug_bounds("notch-next-1").is_none());
    // Settings › Music holds the sign-in.
    click(visual, "notch-tab-settings");
    click(visual, "notch-page-music");
    assert!(visual.debug_bounds("notch-music-page").is_some());
    // A Client ID that is not one is refused before anything is asked.
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch
                .spotify_field
                .update(cx, |field, cx| field.set_text("not an id".into(), cx))
        })
        .unwrap();
    click(visual, "notch-spotify-connect");
    assert!(visual.debug_bounds("notch-spotify-status").is_some());
    let saved = opened
        .window
        .update(visual, |notch, _, _| notch.look.spotify_client.clone())
        .unwrap();
    assert_eq!(saved, "");
}

#[gpui::test]
fn the_catalog_steps_through_the_widgets_that_do_not_fit_its_row(cx: &mut TestAppContext) {
    // One widget on the shelf: nearly every other one can be added, more
    // than the row shows at once.
    let look = Look {
        shelf: vec![monocode_core::notch::Widget::new("clock", 1)],
        ..Look::default()
    };
    let opened = open_notch(look, cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    opened
        .window
        .update(visual, |notch, _, cx| notch.set_open(true, cx))
        .unwrap();
    click(visual, "notch-edit");
    let row = visual.debug_bounds("notch-catalog-scroll").unwrap();
    let last = "notch-add-calendar";
    let hidden = visual
        .debug_bounds(last)
        .is_none_or(|pill| pill.right() > row.right());
    assert!(hidden, "the last widget starts out of the row");
    let offset = |visual: &mut VisualTestContext| {
        opened
            .window
            .update(visual, |notch, _, _| notch.catalog_scroll.offset().x)
            .unwrap()
    };
    assert_eq!(offset(visual), px(0.));
    // Forward until the row ends: the last widget is in it and can be added.
    for _ in 0..6 {
        click(visual, "notch-catalog-forward");
    }
    assert!(offset(visual) < px(0.));
    let row = visual.debug_bounds("notch-catalog-scroll").unwrap();
    let pill = visual.debug_bounds(last).unwrap();
    assert!(pill.right() <= row.right() + px(0.5));
    // And back to the start.
    for _ in 0..6 {
        click(visual, "notch-catalog-back");
    }
    assert_eq!(offset(visual), px(0.));
    click(visual, "notch-catalog-forward");
    click(visual, "notch-catalog-forward");
    click(visual, "notch-catalog-forward");
    click(visual, last);
    let shelf = opened
        .window
        .update(visual, |notch, _, _| notch.look.shelf.clone())
        .unwrap();
    assert!(shelf.iter().any(|widget| widget.id == "calendar"));
}

#[gpui::test]
fn the_system_widget_shows_the_processor_the_memory_and_the_graphics(cx: &mut TestAppContext) {
    use monocode_core::notch::stats::{Memory, Stats};
    let look = Look {
        shelf: vec![monocode_core::notch::Widget::new("system", 2)],
        ..Look::default()
    };
    let opened = open_notch(look, cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.usage = Some(Stats {
                cpu: Some(23.4),
                memory: Some(Memory {
                    used: 8 << 30,
                    total: 32 << 30,
                }),
                pressure: None,
                gpu: Some(61.),
            });
            notch.open = true;
            cx.notify();
        })
        .unwrap();
    visual.run_until_parked();
    assert!(visual.debug_bounds("notch-system").is_some());
    assert!(visual.debug_bounds("notch-system-cpu").is_some());
    assert!(visual.debug_bounds("notch-system-ram").is_some());
    assert!(visual.debug_bounds("notch-system-gpu").is_some());
    // A second reading: the first stays for the bars to glide from, and
    // both are in the last minute's graph.
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.usage_history = Default::default();
            let first = notch.usage.unwrap();
            notch.take_usage(first);
            notch.take_usage(Stats {
                cpu: Some(80.),
                ..first
            });
            assert_eq!(
                notch.usage_before.map(|(before, _)| before.cpu),
                Some(Some(23.4))
            );
            assert_eq!(notch.usage_history.cpu, [23.4, 80.]);
            cx.notify();
        })
        .unwrap();
    visual.run_until_parked();
    assert!(visual.debug_bounds("notch-system-cpu").is_some());
}

#[gpui::test]
fn files_dragged_onto_the_notch_open_it_and_wait_on_its_tray(cx: &mut TestAppContext) {
    use gpui::{ExternalPaths, FileDropEvent};
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    leave(visual);
    assert!(visual.debug_bounds("notch-tray").is_none());
    // Held over the closed notch: it opens on Home and says it takes them.
    let over = visual.debug_bounds("notch").unwrap().center();
    let files: Vec<std::path::PathBuf> = vec!["notes/plan.md".into(), "shots".into()];
    visual.simulate_event(FileDropEvent::Entered {
        position: over,
        paths: ExternalPaths(files.clone().into()),
    });
    visual.simulate_event(FileDropEvent::Pending { position: over });
    visual.run_until_parked();
    assert!(opened.window.update(visual, |n, _, _| n.open).unwrap());
    assert!(visual.debug_bounds("notch-tray").is_some());
    // Dropped: they wait there, newest first.
    let over = visual.debug_bounds("notch").unwrap().center();
    visual.simulate_event(FileDropEvent::Pending { position: over });
    visual.simulate_event(FileDropEvent::Submit { position: over });
    visual.run_until_parked();
    let kept = |visual: &mut VisualTestContext| {
        opened
            .window
            .update(visual, |notch, _, _| notch.tray.items().to_vec())
            .unwrap()
    };
    assert_eq!(kept(visual), files);
    assert!(visual.debug_bounds("notch-tray-file-0").is_some());
    assert!(visual.debug_bounds("notch-tray-file-1").is_some());
    // The cross takes one out without opening it; Clear empties the tray
    // and its row goes.
    click(visual, "notch-tray-remove-0");
    assert_eq!(kept(visual), files[1..]);
    click(visual, "notch-tray-clear");
    assert!(kept(visual).is_empty());
    assert!(visual.debug_bounds("notch-tray").is_none());
}

#[gpui::test]
fn the_open_player_leaves_the_lyrics_to_the_closed_notch(cx: &mut TestAppContext) {
    use monocode_core::notch::media::{Lyrics, Playing, Track, parse_lrc};
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    let track = Track {
        app: "Spotify.exe".into(),
        title: "Song".into(),
        artist: "Artist".into(),
        album: "Album".into(),
        duration: Some(Duration::from_secs(200)),
    };
    let height = |visual: &mut VisualTestContext| size(opened.window, visual).1;
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.lyrics_for = Some(track.clone());
            notch.lyrics = Some((
                track.clone(),
                Lyrics::Synced(parse_lrc("[00:10.00]first line\n[00:20.00]second line")),
            ));
            notch.set_media(
                Some(Playing {
                    track: track.clone(),
                    playing: false,
                    position: Duration::from_secs(12),
                    at: Instant::now(),
                    art: None,
                    shuffle: None,
                }),
                cx,
            );
            notch.set_open(true, cx);
        })
        .unwrap();
    visual.run_until_parked();
    assert!(visual.debug_bounds("notch-player").is_some());
    let moment = |visual: &mut VisualTestContext| {
        opened
            .window
            .update(visual, |notch, _, _| {
                notch.lyric_moment().map(|(_, line, _)| line)
            })
            .unwrap()
    };
    // The closed notch has the line; the open player only its switch.
    assert_eq!(moment(visual).as_deref(), Some("first line"));
    let with_lyrics = height(visual);
    click(visual, "notch-media-lyrics");
    assert_eq!(moment(visual), None);
    assert_eq!(height(visual), with_lyrics);
}

#[gpui::test]
fn the_player_leads_into_spotify_shuffles_and_keeps_liked_songs(cx: &mut TestAppContext) {
    use monocode_core::notch::media::{Playing, Track};
    use monocode_core::notch::spotify::{Artist, Now, Snapshot};
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    let playing = |app: &str, shuffle| Playing {
        track: Track {
            app: app.into(),
            title: "Song".into(),
            artist: "Ana, Bo".into(),
            album: "Album".into(),
            duration: Some(Duration::from_secs(200)),
        },
        playing: false,
        position: Duration::from_secs(12),
        at: Instant::now(),
        art: None,
        shuffle,
    };
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.set_media(Some(playing("Spotify.exe", Some(false))), cx);
            notch.set_open(true, cx);
        })
        .unwrap();
    // Not signed in: the title and each artist lead to Spotify's search, and
    // a line under the player says where "Playing next" comes from.
    assert!(visual.debug_bounds("notch-queue-hint").is_some());
    click(visual, "notch-player-open-track");
    assert_eq!(
        visual.opened_url().as_deref(),
        Some("spotify:search:Song%20Ana%2C%20Bo")
    );
    click(visual, "notch-player-artist-1");
    assert_eq!(visual.opened_url().as_deref(), Some("spotify:search:Bo"));
    // Shuffle is the player's own word: off, then on at a click.
    let shuffle = |visual: &mut VisualTestContext| {
        opened
            .window
            .update(visual, |notch, _, _| notch.shuffle())
            .unwrap()
    };
    assert_eq!(shuffle(visual), Some(false));
    assert!(visual.debug_bounds("notch-media-shuffle-on").is_none());
    click(visual, "notch-media-shuffle");
    assert_eq!(shuffle(visual), Some(true));
    assert!(visual.debug_bounds("notch-media-shuffle-on").is_some());
    // Signed in, Spotify says which track this is: its own addresses, its
    // Liked Songs, and whether there is an order to shuffle.
    let said = |can_shuffle| Snapshot {
        now: Some(Now {
            title: "Song".into(),
            uri: "spotify:track:t1".into(),
            artists: vec![Artist {
                name: "Ana".into(),
                uri: "spotify:artist:a1".into(),
            }],
            shuffle: true,
            can_shuffle,
        }),
        liked: Some(false),
        ..Snapshot::default()
    };
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.look.spotify_client = "0f".repeat(16);
            notch.queue.connected = true;
            notch.set_snapshot(said(true), cx);
            assert_eq!(notch.like(), queue::Like::Known(false));
        })
        .unwrap();
    assert!(visual.debug_bounds("notch-queue-hint").is_none());
    click(visual, "notch-player-open-track");
    assert_eq!(visual.opened_url().as_deref(), Some("spotify:track:t1"));
    click(visual, "notch-player-artist-0");
    assert_eq!(visual.opened_url().as_deref(), Some("spotify:artist:a1"));
    click(visual, "notch-media-like");
    opened
        .window
        .update(visual, |notch, _, cx| {
            assert_eq!(notch.like(), queue::Like::Known(true));
            // A radio: Spotify says there is nothing to shuffle there.
            notch.set_snapshot(said(false), cx);
            assert_eq!(notch.shuffle(), None);
            // The answer is still about the track before: nothing of it is
            // shown for this one.
            let mut other = playing("Spotify.exe", None);
            other.track.title = "Another".into();
            notch.set_media(Some(other), cx);
            assert_eq!(notch.like(), queue::Like::Unknown);
            assert_eq!(notch.shuffle(), None);
            assert_eq!(
                notch.track_link().as_deref(),
                Some("spotify:search:Another%20Ana%2C%20Bo")
            );
            // A browser's video is not Spotify's: no heart, no links.
            notch.set_media(Some(playing("chrome.exe", None)), cx);
            assert_eq!(notch.track_link(), None);
        })
        .unwrap();
    visual.run_until_parked();
    assert!(visual.debug_bounds("notch-media-like").is_none());
    assert!(visual.debug_bounds("notch-media-shuffle").is_none());
    assert!(visual.debug_bounds("notch-media-play").is_some());
}

#[gpui::test]
fn the_heart_leads_to_the_sign_in_until_spotify_is_connected(cx: &mut TestAppContext) {
    use monocode_core::notch::media::{Playing, Track};
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.set_media(
                Some(Playing {
                    track: Track {
                        app: "Spotify.exe".into(),
                        title: "Song".into(),
                        artist: "Ana".into(),
                        album: String::new(),
                        duration: None,
                    },
                    playing: false,
                    position: Duration::ZERO,
                    at: Instant::now(),
                    art: None,
                    shuffle: None,
                }),
                cx,
            );
            notch.set_open(true, cx);
            assert_eq!(notch.like(), queue::Like::Connect);
            // The player does not say how shuffle stands: dimmed, no click.
            assert_eq!(notch.shuffle(), None);
        })
        .unwrap();
    click(visual, "notch-media-shuffle");
    assert!(visual.debug_bounds("notch-media-shuffle-on").is_none());
    click(visual, "notch-media-like");
    assert!(visual.debug_bounds("notch-music-page").is_some());
}

#[gpui::test]
fn the_agents_page_lists_the_personal_agents_apart_from_the_ones_that_code(
    cx: &mut TestAppContext,
) {
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    let crab = monocode_core::notch::Personal {
        name: "MonoCrab".into(),
        session: Some(21),
        coding: true,
        ..ivy()
    };
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(
                Feed {
                    agents: vec![ivy()],
                    ..Feed::default()
                },
                cx,
            );
            notch.set_open(true, cx);
        })
        .unwrap();
    click(visual, "notch-tab-agents");
    // Only personal agents: one section, as before.
    assert!(visual.debug_bounds("notch-agents-code").is_none());
    let one = opened
        .window
        .update(visual, |notch, _, cx| {
            let one = notch.agents_height();
            // The one that codes comes first in the feed and still lands
            // under Code, below the personal ones.
            notch.update_feed(
                Feed {
                    agents: vec![crab, ivy()],
                    ..Feed::default()
                },
                cx,
            );
            one
        })
        .unwrap();
    visual.run_until_parked();
    let top = |visual: &mut VisualTestContext, selector: &'static str| {
        visual.debug_bounds(selector).unwrap().top()
    };
    let code = top(visual, "notch-agents-code");
    assert!(top(visual, "notch-agent-1") < code, "Ivy is personal");
    assert!(code < top(visual, "notch-agent-0"), "MonoCrab codes");
    let two = opened
        .window
        .update(visual, |notch, _, _| notch.agents_height())
        .unwrap();
    assert_eq!(two, one + CAPTION + ROW, "the page grows by Code's section");
}

#[gpui::test]
fn an_agents_chat_is_written_in_at_once_and_shows_what_came_with_what_was_said(
    cx: &mut TestAppContext,
) {
    use monocode_core::notch::{Attached, Said, Talk};
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(
                Feed {
                    agents: vec![ivy()],
                    ..Feed::default()
                },
                cx,
            );
            notch.set_open(true, cx);
        })
        .unwrap();
    click(visual, "notch-tab-agents");
    click(visual, "notch-agent-0");
    // The caret is in the chat's field: what is typed next is the message.
    visual.simulate_input("hola");
    let typed = opened
        .window
        .update(visual, |notch, _, cx| {
            notch.talk_field.read(cx).content.to_string()
        })
        .unwrap();
    assert_eq!(typed, "hola");
    // A picture that is on disk, one that is not, and a document that lives
    // in the app only.
    let picture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons/icon.png");
    let gone = std::env::temp_dir().join("monobot-notch-no-such-picture.png");
    let file = |name: &str, image: bool, path: Option<&std::path::Path>| Attached {
        name: name.into(),
        image,
        path: path.map(std::path::Path::to_path_buf),
    };
    let said = vec![
        Said {
            mine: true,
            text: String::new(),
            files: vec![file("icon.png", true, Some(&picture))],
        },
        Said {
            mine: false,
            text: "Here they are.".into(),
            files: vec![
                file("lost.png", true, Some(&gone)),
                file("Report", false, None),
            ],
        },
    ];
    let feed = |said: Vec<Said>| Feed {
        agents: vec![ivy()],
        talk: Some(Talk {
            session: 20,
            said,
            busy: false,
        }),
        ..Feed::default()
    };
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(feed(said.clone()), cx)
        })
        .unwrap();
    visual.run_until_parked();
    // The picture shows itself, on the side of who sent it; the one that
    // cannot be read is named like a file, beside the document.
    let mine = visual.debug_bounds("notch-talk-photo-0-0").unwrap();
    let theirs = visual.debug_bounds("notch-talk-file-1-0").unwrap();
    assert!(mine.left() > theirs.left());
    assert!(mine.size.height > px(22.), "a picture, not a chip");
    assert!(visual.debug_bounds("notch-talk-photo-1-0").is_none());
    let photos = |visual: &mut VisualTestContext| {
        opened
            .window
            .update(visual, |notch, _, _| {
                notch.talk.as_ref().map_or(0, |talk| talk.photo_count())
            })
            .unwrap()
    };
    assert_eq!(photos(visual), 2);
    // The document opens its chat in the app.
    click(visual, "notch-talk-file-1-1");
    assert!(
        opened
            .events
            .borrow()
            .iter()
            .any(|event| matches!(event, NotchEvent::OpenSession(20)))
    );
    // What is no longer said lets go of its pictures.
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(feed(said[1..].to_vec()), cx)
        })
        .unwrap();
    visual.run_until_parked();
    assert_eq!(photos(visual), 1);
}

#[gpui::test]
fn dictating_grows_its_wings_over_the_agents_and_folds_after_pasting(cx: &mut TestAppContext) {
    use monocode_core::dictation::{BARS, system::Phase};
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    leave(visual);
    let show = |phase: Phase, visual: &mut VisualTestContext| {
        opened
            .window
            .update(visual, |notch, _, cx| {
                notch.show_dictation(phase, [0.4; BARS], cx)
            })
            .unwrap();
        leave(visual);
    };
    // Agents at work have the wings first.
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.update_feed(
                Feed {
                    activities: vec![working(1), working(2)],
                    ..Feed::default()
                },
                cx,
            )
        })
        .unwrap();
    leave(visual);
    let agents = (186. + busy_wings() + 2. * SHOULDER, 37.);
    assert_eq!(size(opened.window, visual), agents);

    // The microphone opens: the dictation takes the wings.
    show(Phase::Starting, visual);
    assert!(visual.debug_bounds("notch-dictation").is_some());
    assert!(visual.debug_bounds("notch-dictation-status").is_some());
    assert_eq!(
        size(opened.window, visual),
        (
            186. + (dictating::EMPTY_WING + dictating::LISTEN_WING) as f32 + 2. * SHOULDER,
            37.
        )
    );

    // Words come in on the left, and the wing grows for them.
    show(
        Phase::Listening {
            level: 0.4,
            words: "hola mundo".into(),
        },
        visual,
    );
    let words = opened
        .window
        .update(visual, |notch, _, _| notch.dictation_words())
        .unwrap();
    assert_eq!(words, ["hola", "mundo"]);
    assert!(visual.debug_bounds("notch-dictation-words").is_some());
    assert!(visual.debug_bounds("notch-dictation-word-0").is_some());
    assert!(visual.debug_bounds("notch-dictation-word-1").is_some());
    let wing = dictating::words_wing("hola mundo".chars().count());
    assert!(wing > dictating::EMPTY_WING);
    assert_eq!(
        size(opened.window, visual),
        (
            186. + (wing + dictating::LISTEN_WING) as f32 + 2. * SHOULDER,
            37.
        )
    );
    // Passing over the notch does not open the panel on the dictation.
    opened
        .window
        .update(visual, |notch, _, cx| notch.hover_changed(true, cx))
        .unwrap();
    visual.executor().advance_clock(OPEN_DELAY * 3);
    visual.run_until_parked();
    assert!(
        !opened
            .window
            .update(visual, |notch, _, _| notch.open)
            .unwrap()
    );
    opened
        .window
        .update(visual, |notch, _, cx| notch.hover_changed(false, cx))
        .unwrap();

    // Pasted: a check and its word, then it folds and the agents return.
    show(Phase::Finishing, visual);
    assert!(visual.debug_bounds("notch-dictation-word-1").is_some());
    show(Phase::Pasted, visual);
    assert!(visual.debug_bounds("notch-dictation-done").is_some());
    assert!(visual.debug_bounds("notch-dictation-words").is_none());
    assert_eq!(
        size(opened.window, visual),
        (
            186. + (dictating::MARK_WING + dictating::PASTED_WING) as f32 + 2. * SHOULDER,
            37.
        )
    );
    show(Phase::Idle, visual);
    assert!(visual.debug_bounds("notch-dictation").is_none());
    assert_eq!(size(opened.window, visual), agents);
}

#[gpui::test]
fn the_pill_shows_the_words_and_the_mic_in_one_row(cx: &mut TestAppContext) {
    use monocode_core::dictation::{BARS, system::Phase};
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    // A screen with no camera: Windows' pill under the top edge.
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.screen_override = Some(Screen {
                safe_top: 0.,
                left_area: 0.,
                right_area: 0.,
                ..macbook()
            });
            notch.show_dictation(
                Phase::Listening {
                    level: 0.7,
                    words: "uno dos tres".into(),
                },
                [0.7; BARS],
                cx,
            )
        })
        .unwrap();
    leave(visual);
    assert!(visual.debug_bounds("notch-dictation").is_some());
    assert!(visual.debug_bounds("notch-dictation-word-2").is_some());
    assert!(visual.debug_bounds("notch-dictation-status").is_some());
    let wing = dictating::words_wing("uno dos tres".chars().count());
    let (width, _) = size(opened.window, visual);
    assert_eq!(
        width,
        (wing + dictating::LISTEN_WING) as f32 + 12. + 2. * SHOULDER
    );
}

#[gpui::test]
fn the_system_dictation_drives_the_wings_and_paste_last_waits_in_the_header(
    cx: &mut TestAppContext,
) {
    use crate::dictation::{Engines, system::SystemDictation, tests::Fake};
    use monocode_core::dictation::{hotkey::Action, system::Phase};
    let fake = Fake::installed("hola mundo");
    let controller = cx.update(|cx| {
        cx.set_global(Engines(fake.clone()));
        let (hits, _) = async_channel::unbounded();
        cx.new(|cx| SystemDictation::new(hits, cx))
    });
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.follow_dictation(controller.clone(), cx)
        })
        .unwrap();
    leave(visual);
    assert_eq!(size(opened.window, visual), (186. + 2. * SHOULDER, 37.));

    controller.update(visual, |this, cx| this.act(Action::Start, cx));
    visual.run_until_parked();
    fake.hear(monobot_dictation::Event::Level(0.5));
    fake.hear(monobot_dictation::Event::Partial("hola".into()));
    visual.run_until_parked();
    assert!(visual.debug_bounds("notch-dictation-word-0").is_some());
    let words = opened
        .window
        .update(visual, |notch, _, _| notch.dictation_words())
        .unwrap();
    assert_eq!(words, ["hola"]);

    // The test platform has nothing in front to paste into: copied.
    controller.update(visual, |this, cx| this.act(Action::Stop, cx));
    visual.run_until_parked();
    assert_eq!(
        controller.read_with(visual, |this, _| this.phase().clone()),
        Phase::Copied
    );
    assert!(visual.debug_bounds("notch-dictation-done").is_some());

    // Paste last waits in the open panel's header and folds it.
    opened
        .window
        .update(visual, |notch, _, cx| notch.set_open(true, cx))
        .unwrap();
    click(visual, "notch-paste-last");
    assert!(
        !opened
            .window
            .update(visual, |notch, _, _| notch.open)
            .unwrap()
    );
    assert_eq!(
        controller.read_with(visual, |this, _| this.phase().clone()),
        Phase::Copied
    );

    // The ending lingers, then the notch is quiet again.
    visual.executor().advance_clock(Duration::from_secs(5));
    visual.run_until_parked();
    leave(visual);
    assert!(visual.debug_bounds("notch-dictation").is_none());
    assert_eq!(size(opened.window, visual), (186. + 2. * SHOULDER, 37.));
}

#[gpui::test]
fn pause_media_while_dictating_pauses_what_plays_and_plays_it_after(cx: &mut TestAppContext) {
    use crate::dictation::{Engines, system::SystemDictation, tests::Fake};
    use monocode_core::dictation::{hotkey::Action, quiet::WhileDictating, system::Phase};
    use monocode_core::notch::media::{Playing, Track};
    let player = |app: &str, playing| Playing {
        track: Track {
            app: app.into(),
            title: "Song".into(),
            artist: "Someone".into(),
            album: "Album".into(),
            duration: Some(Duration::from_secs(200)),
        },
        playing,
        position: Duration::from_secs(20),
        at: Instant::now(),
        art: None,
        shuffle: None,
    };
    let fake = Fake::installed("hola");
    let controller = cx.update(|cx| {
        cx.set_global(crate::settings::Preferences {
            while_dictating: WhileDictating::PauseMedia,
            ..Default::default()
        });
        cx.set_global(Engines(fake.clone()));
        let (hits, _) = async_channel::unbounded();
        cx.new(|cx| SystemDictation::new(hits, cx))
    });
    let opened = open_notch(Look::default(), cx);
    let visual = &mut VisualTestContext::from_window(opened.window.into(), cx);
    let (commands, sent) = std::sync::mpsc::channel();
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.media_commands = Some(commands);
            notch.set_players(vec![player("spotify", true), player("browser", false)], cx);
        })
        .unwrap();
    visual.run_until_parked();
    let toggles = |sent: &std::sync::mpsc::Receiver<(String, platform::MediaCommand)>| {
        sent.try_iter().collect::<Vec<_>>()
    };
    let toggle = |app: &str| (app.to_owned(), platform::MediaCommand::TogglePlay);
    let playing = |visual: &mut VisualTestContext| {
        opened
            .window
            .update(visual, |notch, _, _| {
                notch.media.as_ref().is_some_and(|media| media.playing)
            })
            .unwrap()
    };

    // Only Spotify plays: only Spotify is paused, and the notch shows it
    // paused at once.
    controller.update(visual, |this, cx| this.act(Action::Start, cx));
    visual.run_until_parked();
    assert_eq!(toggles(&sent), [toggle("spotify")]);
    assert!(!playing(visual));
    controller.update(visual, |this, cx| this.act(Action::Stop, cx));
    visual.run_until_parked();
    assert_eq!(
        controller.read_with(visual, |this, _| this.phase().clone()),
        Phase::Copied
    );
    assert_eq!(toggles(&sent), [toggle("spotify")], "played again");
    assert!(playing(visual));

    // Played again by hand while dictating: left playing.
    controller.update(visual, |this, cx| this.act(Action::Start, cx));
    visual.run_until_parked();
    assert_eq!(toggles(&sent), [toggle("spotify")]);
    opened
        .window
        .update(visual, |notch, _, cx| {
            notch.set_players(vec![player("spotify", true)], cx)
        })
        .unwrap();
    controller.update(visual, |this, cx| this.act(Action::Cancel, cx));
    visual.run_until_parked();
    assert!(toggles(&sent).is_empty(), "a toggle would pause it");
    assert!(playing(visual));
}
