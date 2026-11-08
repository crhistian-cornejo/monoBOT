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

fn size(window: WindowHandle<Notch>, visual: &mut VisualTestContext) -> (f32, f32) {
    window
        .update(visual, |_, window, _| {
            let size = window.bounds().size;
            (f32::from(size.width), f32::from(size.height))
        })
        .unwrap()
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
        (186. + 2. * (160. + SHOULDER), 37.)
    );
    // Under the pointer it swells a little, to say it opens.
    let notch = visual.debug_bounds("notch").unwrap();
    visual.simulate_mouse_move(notch.center(), None, Default::default());
    visual.run_until_parked();
    assert_eq!(
        size(opened.window, visual),
        (186. + 2. * (160. + SHOULDER) + PEEK, 40.)
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
    click(visual, "notch-earlier-call");
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
    // Three faces and "+1" on the left; the lead's words and 2/6 on the right,
    // the camera kept in the middle by the wider wing.
    let (left, right) = rest.wings(true);
    assert_eq!(rest.more(), 1);
    assert_eq!(
        size(opened.window, visual),
        (186. + 2. * left.max(right) as f32 + 2. * SHOULDER, 37.)
    );
}
