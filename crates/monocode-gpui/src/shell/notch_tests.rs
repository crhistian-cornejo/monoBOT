//! The notch's chat with an agent hears the answer while it streams, not
//! only once the turn ends.
use super::*;
use crate::notch::{Handle, Notch};
use gpui::TestAppContext;
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
