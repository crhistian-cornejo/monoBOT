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
    // Off the UI thread: a file and two registry values.
    std::thread::spawn(|| {
        if let Err(error) = register_icon() {
            eprintln!("notification icon: {error}");
        }
    });
    cx.on_system_notification_response(|response, _| {
        if let Some(target) = Target::from_tag(&response.tag)
            && let Ok(mut queue) = CLICKS.lock()
        {
            queue.push_back(target);
        }
    });
    true
}

/// monoBOT's logo for its toasts. GPUI registers the AppUserModelID with a
/// name only, so Windows drew a generic icon: an unpackaged app gives the
/// picture as `IconUri`, a PNG on disk, beside that name.
fn register_icon() -> Result<(), String> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegSetKeyValueW};
    use windows::core::HSTRING;
    let folder = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .ok_or("LOCALAPPDATA is not set")?
        .join("monoBOT");
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let icon = folder.join("notification-icon.png");
    let png = include_bytes!("../../../../assets/icons/icon.png");
    if std::fs::read(&icon).ok().as_deref() != Some(png.as_slice()) {
        std::fs::write(&icon, png).map_err(|e| e.to_string())?;
    }
    let key = HSTRING::from(format!(r"Software\Classes\AppUserModelId\{APP_ID}"));
    let set = |name: &str, value: &str| -> Result<(), String> {
        let wide: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                &key,
                &HSTRING::from(name),
                REG_SZ.0,
                Some(wide.as_ptr().cast()),
                (wide.len() * 2) as u32,
            )
        };
        status.ok().map_err(|e| format!("{name}: {e}"))
    };
    // Older builds registered "MonoBOT Native"; the toast's header shows it.
    set("DisplayName", APP_NAME)?;
    set("IconUri", &icon.to_string_lossy())?;
    // The logo's own tile is black: keep the square behind it black too.
    set("IconBackgroundColor", "FF000000")
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
