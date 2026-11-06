//! The composers' mic, where Composer.tsx and MonoComposer.tsx put it.
use super::*;
use crate::dictation::tests::Fake;
use gpui::{TestAppContext, VisualTestContext, WindowHandle, size};
use monocode_core::{dictation::Phase, store::Store};
use std::sync::Mutex;

fn conversation(fake: &Arc<Fake>, cx: &mut TestAppContext) -> WindowHandle<Conversation> {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let summary = store.create("codex", "/missing/dictation-project").unwrap();
    cx.update(|cx| {
        input::bind(cx);
        cx.set_global(crate::dictation::Engines(fake.clone()));
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| {
                let mut view = Conversation::new(summary, Arc::new(Mutex::new(store)), cx);
                view.loaded = true;
                view
            })
        })
        .unwrap()
    })
}

fn phase(window: WindowHandle<Conversation>, visual: &mut VisualTestContext) -> Phase {
    window
        .update(visual, |view, _, cx| view.dictation.read(cx).phase())
        .unwrap()
}

#[gpui::test]
fn the_mic_sits_before_send_and_dictates_into_the_draft(cx: &mut TestAppContext) {
    let fake = Fake::installed("ship it");
    let window = conversation(&fake, cx);
    window
        .update(cx, |view, _, cx| {
            view.input
                .update(cx, |input, cx| input.set_text("Please".into(), cx))
        })
        .unwrap();
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    visual.refresh().unwrap();
    let mic = visual
        .debug_bounds("dictation-mic")
        .expect("the composer shows the mic");
    let send = visual.debug_bounds("send-or-stop").unwrap();
    let toolbar = visual.debug_bounds("composer-toolbar").unwrap();
    assert_eq!(mic.size, size(px(26.), px(26.)));
    // Last before send, a `gap-1` away (send's morph paints a little wider).
    assert!(
        mic.right() < send.right() && (send.left() - mic.right()).abs() <= px(4.),
        "{mic:?} {send:?}"
    );
    assert!(toolbar.contains(&mic.center()));
    assert!(visual.debug_bounds("dictation-bar").is_none());

    visual.simulate_click(mic.center(), Default::default());
    visual.run_until_parked();
    assert_eq!(phase(window, visual), Phase::Listening);
    visual.refresh().unwrap();
    let bar = visual
        .debug_bounds("dictation-bar")
        .expect("listening shows the waveform under the toolbar");
    let composer = visual.debug_bounds("composer-box").unwrap();
    let toolbar = visual.debug_bounds("composer-toolbar").unwrap();
    assert!(
        bar.top() >= toolbar.bottom() && bar.bottom() <= composer.bottom(),
        "{bar:?} {toolbar:?} {composer:?}"
    );
    assert_eq!(bar.size.width, composer.size.width - px(2.));

    // The mic again stops listening and puts the words in at the caret.
    let mic = visual.debug_bounds("dictation-mic").unwrap();
    visual.simulate_click(mic.center(), Default::default());
    visual.run_until_parked();
    assert_eq!(phase(window, visual), Phase::Idle);
    let text = window
        .update(visual, |view, _, cx| {
            view.input.read(cx).content.to_string()
        })
        .unwrap();
    assert_eq!(text, "Please ship it");
    visual.refresh().unwrap();
    assert!(visual.debug_bounds("dictation-bar").is_none());

    // Escape in the composer drops a dictation instead.
    let mic = visual.debug_bounds("dictation-mic").unwrap();
    visual.simulate_click(mic.center(), Default::default());
    visual.run_until_parked();
    assert_eq!(phase(window, visual), Phase::Listening);
    window
        .update(visual, |view, window, cx| {
            window.focus(&view.input.focus_handle(cx), cx)
        })
        .unwrap();
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    assert_eq!(phase(window, visual), Phase::Idle);
    assert_eq!(fake.calls(), ["start", "stop 7", "start", "cancel 7"]);
}

#[gpui::test]
fn the_mono_composer_has_the_mic_beside_send(cx: &mut TestAppContext) {
    let fake = Fake::installed("hola");
    let window = conversation(&fake, cx);
    window
        .update(cx, |view, _, cx| {
            view.configure_mono(
                monocode_core::mono::Mono::default(),
                "/tmp".into(),
                Default::default(),
                cx,
            );
        })
        .unwrap();
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    visual.refresh().unwrap();
    let mic = visual
        .debug_bounds("dictation-mic")
        .expect("the Mono composer shows the mic");
    let send = visual.debug_bounds("mono-send").unwrap();
    let composer = visual.debug_bounds("mono-composer").unwrap();
    assert_eq!(mic.right() + px(4.), send.left());
    assert!(composer.contains(&mic.center()));
    visual.simulate_click(mic.center(), Default::default());
    visual.run_until_parked();
    assert_eq!(phase(window, visual), Phase::Listening);
    visual.refresh().unwrap();
    let bar = visual.debug_bounds("dictation-bar").unwrap();
    let composer = visual.debug_bounds("mono-composer").unwrap();
    let mic = visual.debug_bounds("dictation-mic").unwrap();
    assert!(
        bar.bottom() <= composer.bottom() && bar.top() > mic.bottom(),
        "{bar:?} {mic:?} {composer:?}"
    );
}
