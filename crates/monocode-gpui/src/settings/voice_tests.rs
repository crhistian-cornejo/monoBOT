//! Settings › Voice with real clicks and the app's stand-ins: the fake
//! dictation engine instead of the microphone and the model files, and an
//! in-memory keychain instead of the real one.
use super::*;
use crate::dictation::{Engines, tests::Fake};
use gpui::{
    Bounds, TestAppContext, VisualTestContext, WindowBounds, WindowHandle, WindowOptions, point,
    size,
};
use model::GROUPS;
use monocode_core::voice::{Provider, keys};
use pages::VoiceModel;
use std::sync::Arc;

const KEY: &str = "AIzaSyD-test-key-0123456789abcdef";

fn window(fake: Arc<Fake>, cx: &mut TestAppContext) -> WindowHandle<SettingsView> {
    keys::keep_in_memory();
    cx.update(|cx| cx.set_global(Engines(fake)));
    let window = cx.update(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(1200.), px(900.)),
                ))),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|cx| {
                    let mut view = SettingsView::new(
                        Section::Voice,
                        Preferences::default(),
                        SettingsContext::default(),
                        cx,
                    );
                    view.discovery_task = None;
                    view.discovery_loading = false;
                    view
                })
            },
        )
        .unwrap()
    });
    cx.refresh().unwrap();
    cx.run_until_parked();
    window
}

fn snapshot(window: &WindowHandle<SettingsView>, cx: &mut TestAppContext) -> pages::VoiceSnapshot {
    window
        .read_with(cx, |view, _| view.voice.snapshot.clone())
        .unwrap()
        .expect("the page read the disk and the keychain")
}

fn click(visual: &mut VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is on the page"));
    visual.simulate_click(bounds.center(), gpui::Modifiers::none());
    visual.run_until_parked();
}

#[gpui::test]
fn the_page_lists_both_models_how_calls_talk_and_each_key(cx: &mut TestAppContext) {
    let window = window(Fake::installed(""), cx);
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    for row in [
        "settings-row-voice-dictation",
        "settings-row-voice-call-voice",
        "settings-row-voice-live",
        "settings-row-voice-pace",
        "settings-row-voice-key-gemini",
        "settings-row-voice-key-phonic",
    ] {
        assert!(visual.debug_bounds(row).is_some(), "{row}");
    }
    let shown = snapshot(&window, cx);
    assert!(shown.dictation.installed && shown.dictation.neural);
    assert_eq!(shown.dictation.on_disk, 460_000_000);
}

#[gpui::test]
fn voice_is_an_app_page_in_the_rail(cx: &mut TestAppContext) {
    let _ = cx;
    assert!(Section::ALL.contains(&Section::Voice));
    let (group, sections) = GROUPS[0];
    assert_eq!(group, "App");
    assert!(sections.contains(&Section::Voice));
    assert_eq!(Section::Voice.id(), "voice");
    assert_eq!(Section::Voice.icon(), "mic-01");
}

#[gpui::test]
fn a_missing_speech_model_downloads_from_the_page(cx: &mut TestAppContext) {
    let fake = Arc::new(Fake::default());
    let window = window(fake.clone(), cx);
    assert!(!snapshot(&window, cx).dictation.installed);
    window
        .update(cx, |view, _, cx| {
            view.voice_download(VoiceModel::Dictation, cx)
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(fake.calls(), ["install"]);
    let shown = snapshot(&window, cx);
    assert!(shown.dictation.installed, "the page read the disk again");
    assert!(
        window
            .read_with(cx, |view, _| view.voice.downloading.is_none())
            .unwrap()
    );
}

#[gpui::test]
fn deleting_a_model_asks_once_then_deletes(cx: &mut TestAppContext) {
    let fake = Fake::installed("");
    let window = window(fake.clone(), cx);
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    click(visual, "settings-row-voice-dictation");
    window
        .update(visual, |view, _, cx| {
            view.voice_delete(VoiceModel::Dictation, cx)
        })
        .unwrap();
    visual.run_until_parked();
    assert!(fake.calls().is_empty(), "the first click only asks");
    assert_eq!(
        window
            .read_with(visual, |view, _| view.voice.confirming)
            .unwrap(),
        Some(VoiceModel::Dictation)
    );
    window
        .update(visual, |view, _, cx| {
            view.voice_delete(VoiceModel::Dictation, cx)
        })
        .unwrap();
    visual.run_until_parked();
    assert_eq!(fake.calls(), ["remove"]);
    assert!(!snapshot(&window, visual).dictation.installed);
}

#[gpui::test]
fn a_pasted_key_goes_to_the_keychain_and_leaves_the_field(cx: &mut TestAppContext) {
    keys::keep_in_memory();
    keys::remove(Provider::Phonic).unwrap();
    let window = window(Fake::installed(""), cx);
    // A paste that is not a key is refused with a reason, and nothing saved.
    window
        .update(cx, |view, _, cx| {
            view.voice_type_key(Provider::Phonic, "not a key", cx);
            view.voice_save_key(Provider::Phonic, cx);
        })
        .unwrap();
    cx.run_until_parked();
    assert!(!keys::saved(Provider::Phonic));
    let message = window
        .read_with(cx, |view, _| view.voice.messages.get("phonic").cloned())
        .unwrap();
    assert!(message.unwrap().contains("does not look like an API key"));

    window
        .update(cx, |view, _, cx| {
            view.voice_type_key(Provider::Phonic, &format!(" {KEY}\n"), cx);
            view.voice_save_key(Provider::Phonic, cx);
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(keys::get(Provider::Phonic).unwrap().expose(), KEY);
    assert_eq!(snapshot(&window, cx).keys.get("phonic"), Some(&true));
    let left = window
        .update(cx, |view, _, cx| {
            view.voice_field_text(Provider::Phonic, cx)
        })
        .unwrap();
    assert_eq!(left, "", "the key is not kept on screen");

    // Choosing Phonic, then removing its key, sends calls back to this Mac.
    window
        .update(cx, |view, _, cx| {
            view.update(cx, |p| p.voice_live = "phonic".into());
            view.voice_remove_key(Provider::Phonic, cx);
        })
        .unwrap();
    cx.run_until_parked();
    assert!(!keys::saved(Provider::Phonic));
    assert_eq!(
        window
            .read_with(cx, |view, _| view.prefs.voice_live.clone())
            .unwrap(),
        "local"
    );
}

#[gpui::test]
fn picking_the_call_pace_is_saved(cx: &mut TestAppContext) {
    let window = window(Fake::installed(""), cx);
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    assert_eq!(
        window
            .read_with(visual, |view, _| view.prefs.voice_pace.clone())
            .unwrap(),
        "normal"
    );
    click(visual, "settings-voice-pace-patient");
    assert_eq!(
        window
            .read_with(visual, |view, _| view.prefs.voice_pace.clone())
            .unwrap(),
        "patient"
    );
}

#[gpui::test]
fn picking_how_calls_talk_is_saved(cx: &mut TestAppContext) {
    let window = window(Fake::installed(""), cx);
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    click(visual, "settings-voice-live-gemini");
    assert_eq!(
        window
            .read_with(visual, |view, _| view.prefs.voice_live.clone())
            .unwrap(),
        "gemini"
    );
}
