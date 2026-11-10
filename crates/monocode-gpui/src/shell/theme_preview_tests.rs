//! A workspace theme tried on in Settings paints the app at once and saves
//! nothing; the saved theme is back when the trying on ends.
use super::*;
use crate::settings::{PALETTES, SettingsEvent, palette};
use gpui::TestAppContext;

#[gpui::test]
fn a_theme_tried_on_in_settings_paints_at_once_and_saves_nothing(cx: &mut TestAppContext) {
    let (window, _, _) = super::tests::window(cx);
    let painted = |cx: &mut TestAppContext| {
        cx.refresh().unwrap();
        cx.run_until_parked();
        crate::theme::background_base()
    };
    let saved_color = painted(cx);
    let saved = window
        .update(cx, |shell, _, _| {
            shell.workspace.preferences.workspace_theme.clone()
        })
        .unwrap();
    let surface = |id: &str| -> gpui::Hsla {
        gpui::rgb(palette(id, crate::theme::is_light()).surface).into()
    };
    let other = PALETTES
        .iter()
        .map(|palette| palette.0)
        .find(|id| *id != saved && surface(id) != saved_color)
        .expect("another theme with another surface");
    // Settings says which theme is under the pointer, as its list does.
    let settings = window
        .update(cx, |shell, window, cx| {
            shell.open_settings(crate::settings::Section::Appearance, window, cx);
            shell.settings.clone().expect("Settings is open")
        })
        .unwrap();
    cx.run_until_parked();
    settings.update(cx, |_, cx| {
        cx.emit(SettingsEvent::PreviewTheme(Some(other.to_owned())))
    });
    assert_eq!(painted(cx), surface(other), "the app wears it at once");
    window
        .update(cx, |shell, _, _| {
            assert_eq!(shell.workspace.preferences.workspace_theme, saved);
            let stored = locked(&shell.store).unwrap().ui_state().unwrap();
            assert!(
                stored.is_none_or(|state| !state.contains(other)),
                "nothing about it is saved"
            );
        })
        .unwrap();
    // The list closes unchosen: the saved theme is back.
    settings.update(cx, |_, cx| cx.emit(SettingsEvent::PreviewTheme(None)));
    assert_eq!(painted(cx), saved_color);
    // Tried on again, then Settings itself closes: back again.
    settings.update(cx, |_, cx| {
        cx.emit(SettingsEvent::PreviewTheme(Some(other.to_owned())))
    });
    assert_eq!(painted(cx), surface(other));
    window
        .update(cx, |shell, window, cx| shell.close_settings(window, cx))
        .unwrap();
    assert_eq!(painted(cx), saved_color);
}
