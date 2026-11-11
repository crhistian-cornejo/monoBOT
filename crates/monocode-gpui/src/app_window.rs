//! The workspace window closed to the background and brought back. With
//! Settings › General's "Keep monoBOT running in the background when the
//! window closes" on, closing the window hides it (`monocode_core::app_window`
//! decides), so the shell, the notch, the tray and the agents keep going.
//! Every way back activates the window, which shows it again: the notch's
//! Open, the tray's "Open monoBOT", a notification, the Dock (macOS) and a
//! second launch (Windows). Quitting (Ctrl+Q / ⌘Q, the tray's Quit) quits.
use gpui::{App, Window};

/// Whether this system has a way back to a hidden window: the notch and the
/// tray or the Dock. Linux has neither, so closing closes there.
pub(crate) const CAN_HIDE: bool = cfg!(any(target_os = "macos", target_os = "windows"));

/// Hides the workspace window instead of closing it. The system takes the
/// keyboard away from it; activating it again shows it (`reveal`).
pub(crate) fn hide(window: &Window) {
    #[cfg(all(target_os = "macos", not(test)))]
    crate::traffic_lights::hide(window);
    #[cfg(all(target_os = "windows", not(test)))]
    win::hide(window);
    #[cfg(any(test, not(any(target_os = "macos", target_os = "windows"))))]
    let _ = window;
}

/// Brings the workspace window forward, shown again if it was hidden:
/// activating a window shows it on macOS (`makeKeyAndOrderFront`) and on
/// Windows (the vendored platform's `activate` shows a hidden window).
#[cfg_attr(
    not(any(test, target_os = "macos", target_os = "windows")),
    allow(dead_code)
)]
pub(crate) fn reveal(cx: &mut App) {
    cx.activate(true);
    for window in cx.windows() {
        if let Some(shell) = window.downcast::<crate::shell::Shell>() {
            let _ = shell.update(cx, |_, window, _| window.activate_window());
        }
    }
}

/// Ctrl+Q / ⌘Q and the menu's Quit with no workspace window in front (it
/// was closed to the background, or the notch has the keys) still quit, by
/// the workspace's own quit, which asks about unsaved edits first.
pub(crate) fn bind_quit(cx: &mut App) {
    cx.on_action(|_: &crate::Quit, cx| {
        let shell = cx
            .windows()
            .into_iter()
            .find_map(|window| window.downcast::<crate::shell::Shell>());
        match shell {
            Some(shell) => {
                let _ = shell.update(cx, |shell, window, cx| shell.request_quit(window, cx));
            }
            None => crate::quit(cx),
        }
    });
}

/// Windows: a second launch of the same data folder wakes this copy, which
/// shows its window; the second copy exits (`main.rs`).
#[cfg(target_os = "windows")]
pub(crate) fn listen(instance: monocode_core::app_window::Instance, cx: &mut App) {
    let (sender, asks) = async_channel::unbounded::<()>();
    std::thread::Builder::new()
        .name("monobot-instance".into())
        .spawn(move || {
            while instance.wait() {
                if sender.send_blocking(()).is_err() {
                    break;
                }
            }
        })
        .ok();
    cx.spawn(async move |cx| {
        while asks.recv().await.is_ok() {
            cx.update(reveal);
        }
    })
    .detach();
}

#[cfg(all(target_os = "windows", not(test)))]
mod win {
    use gpui::Window;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindowAsync};

    /// Hidden through the message queue: this runs inside the window's own
    /// close message, and a synchronous hide would deliver its activation
    /// and size messages to GPUI while the app is still borrowed.
    pub(super) fn hide(window: &Window) {
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return;
        };
        // SAFETY: a plain request on the live window; a refusal changes nothing.
        let _ = unsafe { ShowWindowAsync(HWND(handle.hwnd.get() as _), SW_HIDE) };
    }
}

/// The Dock icon brings back a window closed to the background (macOS:
/// Windows has no such click, and a second launch does it instead).
pub(crate) fn on_reopen(application: gpui::Application) -> gpui::Application {
    #[cfg(target_os = "macos")]
    application.on_reopen(reveal);
    application
}
