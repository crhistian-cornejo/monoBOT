use super::*;
use gpui::{TestAppContext, VisualTestContext};
use monocode_core::store::Store;
use std::sync::{Arc, Mutex};

fn notes_window(cx: &mut TestAppContext, cwd: &str) -> (Entity<Notes>, VisualTestContext) {
    cx.update(input::bind);
    cx.update(|cx| cx.set_reduce_motion(true));
    let store = Arc::new(Mutex::new(
        Store::open(std::path::Path::new(":memory:")).unwrap(),
    ));
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| Notes::new(store, cx))
        })
        .unwrap()
    });
    let view = window.root(cx).unwrap();
    let cwd = cwd.to_string();
    window
        .update(cx, |notes, _, cx| {
            notes.set_context(cwd, Vec::new(), cx);
            notes.refresh(cx);
        })
        .unwrap();
    let visual = VisualTestContext::from_window(window.into(), cx);
    (view, visual)
}

#[gpui::test]
fn the_plus_button_creates_a_note_without_a_project(cx: &mut TestAppContext) {
    let (view, mut visual) = notes_window(cx, "");
    visual.run_until_parked();
    assert!(
        !visual.read(|cx| view.read(cx).dirty),
        "nothing to save yet"
    );
    let plus = visual
        .debug_bounds("create-note")
        .expect("the New note button");
    visual.simulate_click(plus.center(), gpui::Modifiers::default());
    visual.run_until_parked();
    let (count, error) = visual.read(|cx| {
        let notes = view.read(cx);
        (notes.notes.len(), notes.error.clone())
    });
    assert_eq!(error, None);
    assert_eq!(count, 1, "one new note");
    // A second one too, while the first is still empty.
    visual.simulate_click(plus.center(), gpui::Modifiers::default());
    visual.run_until_parked();
    let (count, error) = visual.read(|cx| {
        let notes = view.read(cx);
        (notes.notes.len(), notes.error.clone())
    });
    assert_eq!(error, None);
    assert_eq!(count, 2, "another new note");
}
