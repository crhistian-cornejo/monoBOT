//! Closing the workspace window with "Keep monoBOT running in the
//! background" on hides it instead of closing it: the shell, and the notch
//! it owns, stay. Activating it again (the notch, the tray, the Dock, a
//! second launch) brings it back.
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use monocode_core::store::Store;

#[gpui::test]
fn closing_kept_running_hides_the_window_and_reveal_brings_it_back(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!(
        "native-close-to-background-{}",
        monocode_core::mono::new_id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Arc::new(Mutex::new(Store::open(&dir.join("native.sqlite")).unwrap()));
    let cwd = dir.to_string_lossy().to_string();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| Shell::new(store, Vec::new(), cwd, None, cx))
        })
        .unwrap()
    });
    let shell = window.update(cx, |_, _, cx| cx.entity()).unwrap();
    let id = window.window_id();
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    let set = |on: bool, visual: &mut VisualTestContext| {
        window
            .update(visual, |shell, _, _| {
                shell.workspace.preferences.close_to_tray = on;
            })
            .unwrap();
    };

    // Turned off, closing closes, as before.
    set(false, visual);
    assert!(visual.simulate_close(), "the window closes");

    // Kept running (the default), the window stays open behind the scenes.
    set(true, visual);
    assert!(!visual.simulate_close(), "the window hides instead");
    visual.run_until_parked();
    assert!(
        visual.update(|_, cx| cx.windows().iter().any(|w| w.window_id() == id)),
        "the window is still there"
    );
    assert!(
        shell.read_with(visual, |shell, _| !shell.quit_prompted),
        "no unsaved edits, so nothing is asked"
    );

    // Hidden, the system takes the keyboard from it. Any way back reveals it.
    visual.deactivate_window();
    assert!(!visual.update(|window, _| window.is_window_active()));
    // From the app, as the tray, the Dock and a second launch call it.
    visual.cx.update(crate::app_window::reveal);
    visual.run_until_parked();
    assert!(
        visual.update(|window, _| window.is_window_active()),
        "the window is in front again"
    );
    assert!(
        shell.read_with(visual, |shell, _| shell.window_active),
        "the shell saw it come back"
    );
}
