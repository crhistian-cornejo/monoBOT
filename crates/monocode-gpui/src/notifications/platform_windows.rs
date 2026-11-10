//! Windows toasts through GPUI's notification center. Toasts are silent, so
//! the host plays its own cue; Windows owns the on/off switch in Settings.

use std::{
    collections::VecDeque,
    sync::{
        Mutex,
        mpsc::{self, Receiver},
    },
};

use gpui::{App, SystemNotification};

use super::{Notification, Permission, Target, resolved};

/// The installed app's AppUserModelID, as the NSIS shortcut and the macOS bundle.
const APP_ID: &str = "dev.usemono.gpui";
const APP_NAME: &str = "monoBOT";
static CLICKS: Mutex<VecDeque<Target>> = Mutex::new(VecDeque::new());

pub fn initialize(cx: &App) -> bool {
    cx.set_app_identity(APP_ID, APP_NAME);
    cx.on_system_notification_response(|response, _| {
        if let Some(target) = Target::from_tag(&response.tag)
            && let Ok(mut queue) = CLICKS.lock()
        {
            queue.push_back(target);
        }
    });
    true
}

/// Windows has no prompt: toasts show unless the user, or a policy, turned
/// them off. An AUMID GPUI has not registered yet reads as allowed.
pub fn permission() -> Receiver<Permission> {
    use windows::UI::Notifications::{NotificationSetting, ToastNotificationManager};
    let setting = ToastNotificationManager::CreateToastNotifierWithId(&APP_ID.into())
        .and_then(|notifier| notifier.Setting());
    resolved(match setting {
        Ok(
            NotificationSetting::DisabledForApplication
            | NotificationSetting::DisabledForUser
            | NotificationSetting::DisabledByGroupPolicy,
        ) => Permission::Denied,
        _ => Permission::Granted,
    })
}

pub fn request_permission() -> Receiver<Permission> {
    permission()
}

pub fn show(notification: Notification, cx: &App) -> Receiver<Result<(), String>> {
    let (sender, receiver) = mpsc::channel();
    cx.show_system_notification(SystemNotification {
        tag: notification.tag.into(),
        title: notification.subtitle.into(),
        body: notification.body.into(),
        actions: Vec::new(),
    });
    let _ = sender.send(Ok(()));
    receiver
}

pub fn drain_clicks() -> Vec<Target> {
    CLICKS
        .lock()
        .map(|mut queue| queue.drain(..).collect())
        .unwrap_or_default()
}

/// The taskbar has no numeric badge for unpackaged apps.
pub fn set_dock_badge(_: usize) {}

pub fn dismiss(tag: &str, cx: &App) {
    cx.dismiss_system_notification(tag);
}

pub fn open_settings() -> Result<(), String> {
    std::process::Command::new("explorer.exe")
        .arg("ms-settings:notifications")
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}
