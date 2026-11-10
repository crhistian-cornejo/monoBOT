//! The notch's chat with an agent hears the answer while it streams, not
//! only once the turn ends.
use super::*;
use crate::notch::{Handle, Notch};
use gpui::{TestAppContext, WindowHandle};
use monocode_core::notch::{Look, Said};
use monocode_core::store::Store;
use monocode_core::transcript::Row;
use std::rc::Rc;

fn row(role: &str, text: &str) -> Row {
    Row {
        role: role.into(),
        text: text.into(),
        first: true,
        sent_at: None,
    }
}

#[gpui::test]
fn a_streaming_answer_reaches_the_notchs_chat_before_the_turn_ends(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!(
        "native-notch-talk-{}",
        monocode_core::mono::new_id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let project = dir.to_string_lossy().to_string();
    let store = Arc::new(Mutex::new(Store::open(&dir.join("native.sqlite")).unwrap()));
    let cwd = project.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| Shell::new(store, Vec::new(), cwd, None, cx))
        })
        .unwrap()
    });
    cx.run_until_parked();
    let manager = window
        .update(cx, |shell, _, _| shell.mono_manager())
        .unwrap();
    let handle: gpui::AnyWindowHandle = window.into();
    handle
        .update(cx, |_, w, cx| {
            manager.update(cx, |m, cx| m.create(vec![project.clone()], w, cx))
        })
        .unwrap();
    cx.run_until_parked();
    let session = cx.update(|cx| manager.read(cx).current().unwrap().session_id.unwrap());
    // The notch, with its chat open on this agent, as the shell holds it.
    let notch = cx.update(|cx| {
        let notch = cx
            .open_window(Default::default(), |_, cx| {
                cx.new(|cx| Notch::new(Look::default(), Rc::new(|_, _| {}), cx))
            })
            .unwrap();
        Handle::from_window(notch, cx).unwrap()
    });
    let entity = notch.entity.clone();
    window
        .update(cx, |shell, _, _| {
            shell.notch = Some(notch);
            shell.notch_talk = Some(session);
        })
        .unwrap();
    let chat = cx.update(|cx| {
        manager
            .read(cx)
            .notification_entity(session, cx)
            .expect("the agent's chat")
    });
    // A streamed update, as a turn's loop lands it: rows patched in and the
    // chat redrawn, with no request-level change.
    let stream = |rows: Vec<Row>, cx: &mut TestAppContext| {
        chat.update(cx, |chat, cx| {
            chat.busy = true;
            chat.transcript
                .update(cx, |view, cx| view.patch(0, rows, cx));
            cx.notify();
        });
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(300));
        cx.run_until_parked();
    };
    let talk = |cx: &mut TestAppContext| cx.update(|cx| entity.read(cx).feed().talk.clone());
    stream(
        vec![row("user", "What changed?"), row("assistant", "Reading")],
        cx,
    );
    let heard = talk(cx).expect("the notch was told what was said");
    assert_eq!(heard.session, session);
    assert!(heard.busy);
    assert_eq!(
        heard.said.last(),
        Some(&Said {
            mine: false,
            text: "Reading".into(),
            files: Vec::new(),
        })
    );
    // More of the same answer: the notch catches up while the turn runs.
    stream(
        vec![
            row("user", "What changed?"),
            row("assistant", "Reading the shell"),
        ],
        cx,
    );
    let heard = talk(cx).unwrap();
    assert!(heard.busy);
    assert_eq!(heard.said.last().unwrap().text, "Reading the shell");
}

/// Three chats in one project, the shell in the given layout, and the notch
/// following it as the app wires it: its clicks reach the shell.
fn shell_with_notch(
    classic: bool,
    cx: &mut TestAppContext,
) -> (WindowHandle<Shell>, WindowHandle<Notch>, Vec<i64>) {
    cx.update(|cx| {
        input::bind(cx);
        cx.set_reduce_motion(true);
    });
    let mut store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let mut summaries = Vec::new();
    for title in ["Fix the build", "Notch chats", "Tray menu"] {
        let mut summary = store.create("claude", "/work/monobot").unwrap();
        store
            .write_rows(summary.id, 0, &[row("user", title)])
            .unwrap();
        store.title(summary.id, title).unwrap();
        summary.title = title.into();
        summary.has_messages = true;
        summaries.push(summary);
    }
    let ids: Vec<i64> = summaries.iter().map(|summary| summary.id).collect();
    let mut state = Workspace {
        classic_layout: classic,
        ..Default::default()
    };
    state.add_project("/work/monobot");
    let saved = serde_json::to_string(&state).unwrap();
    let store = Arc::new(Mutex::new(store));
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| Shell::new(store, summaries, "/work/monobot".into(), Some(saved), cx))
        })
        .unwrap()
    });
    cx.run_until_parked();
    let notch = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| {
                let mut notch = Notch::new(Look::default(), Rc::new(|_, _| {}), cx);
                notch.screen_override = Some(monocode_core::notch::Screen {
                    width: 1512.,
                    safe_top: 38.,
                    left_area: 662.,
                    right_area: 664.,
                    menu_bar: 37.,
                });
                notch
            })
        })
        .unwrap()
    });
    let handle = cx.update(|cx| Handle::from_window(notch, cx).unwrap());
    window
        .update(cx, |shell, _, cx| {
            shell.follow_notch(&handle, cx);
            shell.notch = Some(handle);
            shell.sync_notch(cx);
        })
        .unwrap();
    settle(cx);
    (window, notch, ids)
}

/// Lets the notch's sync (at most four a second) and the shell catch up.
fn settle(cx: &mut TestAppContext) {
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(300));
    cx.run_until_parked();
}

fn click(notch: WindowHandle<Notch>, selector: String, cx: &mut TestAppContext) {
    let visual = &mut gpui::VisualTestContext::from_window(notch.into(), cx);
    visual.run_until_parked();
    let bounds = visual
        .debug_bounds(Box::leak(selector.clone().into_boxed_str()))
        .unwrap_or_else(|| panic!("{selector} is drawn"));
    visual.simulate_click(bounds.center(), Default::default());
    settle(cx);
}

fn active(window: WindowHandle<Shell>, cx: &mut TestAppContext) -> Option<i64> {
    window
        .update(cx, |shell, _, _| shell.workspace.active)
        .unwrap()
}

fn a_click_in_the_notch_opens_that_very_chat(classic: bool, cx: &mut TestAppContext) {
    let (window, notch, ids) = shell_with_notch(classic, cx);
    notch
        .update(cx, |notch, _, cx| {
            notch.set_open(true, cx);
            notch.set_tab(crate::notch::Tab::Chat, cx);
        })
        .unwrap();
    // The chat in the middle of the list, then the first: each opens itself.
    click(notch, format!("notch-chat-recent-{}", ids[1]), cx);
    assert_eq!(active(window, cx), Some(ids[1]));
    click(notch, format!("notch-chat-recent-{}", ids[0]), cx);
    assert_eq!(active(window, cx), Some(ids[0]));
    // A Claude Code session in a terminal, in the same folder, is a coding
    // chat: it is listed with them, by what it was asked, and its click
    // brings its own app forward without opening one of monoBOT's chats.
    let hook = |name: &str| {
        monocode_core::notch::hooks::parse(&format!(
            "claude\n{}",
            serde_json::json!({
                "session_id": "terminal",
                "cwd": "/work/monobot",
                "hook_event_name": name,
                "prompt": "Explain the tray menu",
            })
        ))
        .unwrap()
    };
    window
        .update(cx, |shell, _, cx| {
            let now = monocode_core::time::now_ms();
            shell.notch_externals.apply(hook("UserPromptSubmit"), now);
            shell.sync_notch(cx);
        })
        .unwrap();
    settle(cx);
    let external = monocode_core::notch::hooks::external_id("terminal");
    let title = cx.update(|cx| {
        notch
            .read(cx)
            .unwrap()
            .feed()
            .activities
            .iter()
            .find(|activity| activity.id == external)
            .map(|activity| (activity.title.clone(), activity.agent.clone()))
    });
    assert_eq!(
        title,
        Some(("monobot · Explain the tray menu".to_owned(), None))
    );
    click(notch, format!("notch-working-{external}"), cx);
    assert_eq!(active(window, cx), Some(ids[0]));
    click(notch, format!("notch-chat-recent-{}", ids[2]), cx);
    assert_eq!(active(window, cx), Some(ids[2]));
}

#[gpui::test]
fn a_click_in_the_notch_opens_that_very_chat_in_focused(cx: &mut TestAppContext) {
    a_click_in_the_notch_opens_that_very_chat(false, cx);
}

#[gpui::test]
fn a_click_in_the_notch_opens_that_very_chat_in_classic(cx: &mut TestAppContext) {
    a_click_in_the_notch_opens_that_very_chat(true, cx);
}
