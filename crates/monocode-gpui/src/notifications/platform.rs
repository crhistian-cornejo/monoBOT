//! OS bridge. Call initialization and Dock updates on the UI thread. All other
//! results are channels so authorization/dispatch never block the UI thread.

use std::sync::mpsc::{self, Receiver};

use super::delivery::{Permission, Target};

pub struct Notification {
    pub tag: String,
    pub subtitle: String,
    pub body: String,
    /// Read only where the OS plays the sound (`PLAYS_SOUND`).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub sound: bool,
}

/// Whether a shown notification plays the system sound itself. Windows toasts
/// are silent, so the host plays its own cue there.
pub const PLAYS_SOUND: bool = cfg!(target_os = "macos");

#[cfg(target_os = "macos")]
#[path = "platform_macos.rs"]
mod implementation;

#[cfg(target_os = "windows")]
#[path = "platform_windows.rs"]
mod implementation;

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use implementation::*;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn initialize(_: &gpui::App) -> bool {
    false
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn permission() -> Receiver<Permission> {
    resolved(Permission::Unsupported)
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn request_permission() -> Receiver<Permission> {
    permission()
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn show(_: Notification, _: &gpui::App) -> Receiver<Result<(), String>> {
    resolved(Err(
        "Native notifications are unavailable on this platform".into()
    ))
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn drain_clicks() -> Vec<Target> {
    Vec::new()
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn set_dock_badge(_: usize) {}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn dismiss(_: &str, _: &gpui::App) {}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn open_settings() -> Result<(), String> {
    Err("Notification settings are unavailable on this platform".into())
}

fn resolved<T>(value: T) -> Receiver<T> {
    let (sender, receiver) = mpsc::channel();
    let _ = sender.send(value);
    receiver
}
