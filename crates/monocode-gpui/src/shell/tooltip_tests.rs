//! Icon-only controls name themselves with the app's standard hover tip.
use super::*;
use gpui::{TestAppContext, VisualTestContext, WindowHandle};

/// Hovers each control by its debug selector and asserts the chrome tip
/// with `label` appears beside it, then that it leaves with the pointer.
pub(crate) fn assert_tips(visual: &mut VisualTestContext, controls: &[(&'static str, &str)]) {
    for &(selector, label) in controls {
        visual.refresh().unwrap();
        let bounds = visual
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is drawn"));
        hover(visual, bounds.center());
        let tip: &'static str = Box::leak(format!("tooltip:{label}").into_boxed_str());
        assert!(
            visual.debug_bounds(tip).is_some(),
            "hovering {selector} shows the tip “{label}”"
        );
        // The tip never covers its control, so it cannot take the click.
        let card = visual.debug_bounds("chrome-tooltip").unwrap();
        assert!(
            !card.contains(&bounds.center()),
            "{selector}'s tip stays clear of the pointer"
        );
    }
    hover(visual, gpui::point(px(1.), px(1.)));
}

fn hover(visual: &mut VisualTestContext, at: gpui::Point<Pixels>) {
    visual.simulate_mouse_move(at, None, Default::default());
    visual.run_until_parked();
    visual
        .executor()
        .advance_clock(std::time::Duration::from_millis(600));
    visual.run_until_parked();
    visual.refresh().unwrap();
}

fn setup(cx: &mut TestAppContext) -> (WindowHandle<Shell>, i64) {
    cx.update(|cx| {
        cx.set_reduce_motion(true);
        crate::input::bind(cx);
        crate::popover::bind(cx);
    });
    let (window, _, session) = super::tests::window(cx);
    cx.simulate_window_resize(window.into(), gpui::size(px(1500.), px(900.)));
    window
        .update(cx, |shell, window, cx| {
            shell.workspace.sidebar_visible = true;
            shell.workspace.right_panel_open = false;
            shell.open(session, window, cx);
            shell.layout_changed(cx);
        })
        .unwrap();
    cx.run_until_parked();
    (window, session)
}

fn row(role: &str, text: &str) -> monocode_core::transcript::Row {
    monocode_core::transcript::Row {
        role: role.into(),
        text: text.into(),
        first: true,
        sent_at: None,
    }
}

#[gpui::test]
fn chat_message_actions_show_tips(cx: &mut TestAppContext) {
    let (window, session) = setup(cx);
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    // Let the chat finish loading its (empty) history before replacing it.
    visual.refresh().unwrap();
    visual.run_until_parked();
    window
        .update(visual, |shell, _, cx| {
            let conversation = shell.sessions[&session].clone();
            conversation.update(cx, |c, cx| {
                c.summary.has_messages = true;
                c.transcript.update(cx, |view, cx| {
                    view.patch(
                        0,
                        vec![
                            row("user", "Where is the parser?"),
                            row("assistant", "The parser lives in src/parse.rs."),
                        ],
                        cx,
                    )
                });
            });
        })
        .unwrap();
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(
        visual.debug_bounds("user-row-0").is_some(),
        "the chat renders its prompt"
    );
    assert_tips(
        visual,
        &[
            ("response-copy", "Copy response"),
            ("response-note", "Save as note"),
            ("response-handoff", "Handoff"),
            ("response-opinion", "Second opinion"),
        ],
    );
    // A prompt's own actions show while it is hovered.
    let prompt = visual
        .debug_bounds("user-row-0")
        .expect("the prompt renders");
    hover(visual, prompt.center());
    assert_tips(visual, &[("mono-copy-message", "Copy message")]);
    // The tip leaves the click to its button.
    visual.refresh().unwrap();
    let copy = visual.debug_bounds("response-copy").unwrap();
    hover(visual, copy.center());
    visual.simulate_click(copy.center(), Default::default());
    visual.run_until_parked();
    assert_eq!(
        visual.read_from_clipboard().and_then(|item| item.text()),
        Some("The parser lives in src/parse.rs.".into())
    );
}

#[gpui::test]
fn sidebar_icon_controls_show_tips(cx: &mut TestAppContext) {
    let (window, _) = setup(cx);
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    assert_tips(
        visual,
        &[
            ("sidebar-options", "Sidebar options"),
            ("sidebar-filter-button", "Filter sessions"),
        ],
    );
}

#[gpui::test]
fn agents_zone_icon_controls_show_tips(cx: &mut TestAppContext) {
    let (window, _) = setup(cx);
    window
        .update(cx, |shell, window, cx| {
            shell.workspace.preferences.monos = true;
            shell.set_agents_zone(true, window, cx);
        })
        .unwrap();
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    assert_tips(visual, &[("sidebar-options", "Sidebar options")]);
}

#[gpui::test]
fn inbox_icon_controls_show_tips(cx: &mut TestAppContext) {
    let (window, _) = setup(cx);
    window
        .update(cx, |shell, window, cx| shell.open_inbox(window, cx))
        .unwrap();
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    // Its first load settles before a provider is marked connected.
    visual.refresh().unwrap();
    visual.run_until_parked();
    window
        .update(visual, |shell, _, cx| {
            let inbox = shell.inbox.clone().expect("the inbox opened");
            inbox.update(cx, |inbox, cx| inbox.connect_for_test("github", cx));
        })
        .unwrap();
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(
        visual.debug_bounds("inbox-source-github").is_some(),
        "the connected provider's tab renders"
    );
    assert_tips(
        visual,
        &[
            ("inbox-filter", "Filter inbox"),
            ("inbox-read-all", "Mark all as read"),
            ("inbox-refresh", "Refresh"),
            // With a provider beside it, Agenda shrinks to its icon.
            ("inbox-source-agenda", "Agenda"),
        ],
    );
}
