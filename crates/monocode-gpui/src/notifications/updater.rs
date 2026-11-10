//! Shared app updater: one check/install across windows; no I/O on the UI thread.
//!
//! A found update downloads by itself and waits, verified, for a restart.
//! The Windows installer also finishes it when the app quits. When the
//! download or the installation fails, the card offers the installer from
//! the update Worker's download page instead.
use crate::{
    runtime::{Database, locked},
    theme::*,
};
use gpui::{App, Context, Entity, EventEmitter, Global, Role, div, prelude::*, px};
use monocode_core::app_update::{self, Config, Download, Installation, Installed, Update};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone)]
pub(crate) struct Shared(pub Entity<Updater>, Arc<Mutex<InstallGate>>);
impl Global for Shared {}
#[derive(Default)]
struct InstallGate {
    installing: bool,
    quitting: bool,
    /// A verified download waiting for a restart, or for the app to quit.
    ready: Option<Ready>,
}
struct Ready {
    update: Update,
    installation: Installation,
    download: Download,
    store: Database,
}

/// A normal quit can cancel a download, but must not interrupt a file swap or
/// a privileged package install. Keep this gate outside the entity borrow so
/// successful installation can call the regular quit path from its callback.
/// A downloaded update the platform installs unattended starts its installer
/// here; the quit continues once the installer is running.
pub(crate) fn defer_quit(cx: &mut App) -> bool {
    let Some(shared) = cx.try_global::<Shared>() else {
        return false;
    };
    let gate = shared.1.clone();
    let mut state = gate.lock().unwrap_or_else(|e| e.into_inner());
    state.quitting = true;
    if state.installing {
        return true;
    }
    let Some(ready) = state
        .ready
        .take_if(|ready| Update::installs_on_quit(&ready.installation))
    else {
        return false;
    };
    state.installing = true;
    drop(state);
    let work = cx.background_executor().spawn(async move {
        let Ready {
            update,
            installation,
            download,
            store,
        } = ready;
        let installed = mark_installing(&store, &update)
            .and_then(|_| update.install_on_quit(download, &installation));
        if installed.is_err() {
            let _ = locked(&store).and_then(|store| store.save_setting(INSTALLED, ""));
        }
    });
    cx.spawn(async move |cx| {
        work.await;
        gate.lock().unwrap_or_else(|e| e.into_inner()).installing = false;
        cx.update(crate::quit);
    })
    .detach();
    true
}
pub(crate) enum Event {
    Changed,
    Error(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Disabled,
    Idle,
    Checking,
    Current,
    Downloading,
    /// Downloaded and verified; installs on restart (or on quit on Windows).
    Ready,
    Installing,
    /// The download or the installation failed: offer the installer by hand.
    Failed,
}

const INSTALLED: &str = "monocode.installedUpdate";

/// Records the version being installed, so the next launch shows What's new
/// once it is really running.
fn mark_installing(store: &Database, update: &Update) -> Result<(), String> {
    let version = update.version.to_string();
    locked(store)?.save_setting(
        INSTALLED,
        &serde_json::json!({ "version": version }).to_string(),
    )?;
    locked(store)?.save_setting(
        "monocode.nativeReleaseNotes",
        &serde_json::json!({ "version": version, "body": update.notes }).to_string(),
    )
}

pub(crate) fn version() -> &'static str {
    option_env!("MONOCODE_NATIVE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}
pub(crate) fn shared(store: Database, cx: &mut App) -> Entity<Updater> {
    if let Some(shared) = cx.try_global::<Shared>() {
        return shared.0.clone();
    }
    let entity = cx.new(|cx| Updater::new(store, cx));
    let gate = entity.read(cx).gate.clone();
    cx.set_global(Shared(entity.clone(), gate));
    entity
}

pub(crate) struct Updater {
    config: Option<Config>,
    installation: Option<Installation>,
    store: Database,
    phase: Phase,
    pending: Option<Update>,
    progress: Option<u64>,
    message: Option<String>,
    poll: Option<gpui::Task<()>>,
    gate: Arc<Mutex<InstallGate>>,
}
impl EventEmitter<Event> for Updater {}
impl Updater {
    fn new(store: Database, cx: &mut Context<Self>) -> Self {
        let setup = (|| {
            let endpoint = app_update::built_in_feed()
                .ok_or("Automatic updates aren’t configured for this build")?;
            let key = option_env!("MONOCODE_NATIVE_UPDATER_PUBKEY")
                .filter(|v| !v.is_empty())
                .ok_or("This build has no update verification key")?;
            Config::new(endpoint, key, version())
        })();
        let (config, message) = match setup {
            Ok(config) => (Some(config), None),
            Err(error) => (None, Some(error)),
        };
        let mut this = Self {
            phase: if config.is_some() {
                Phase::Idle
            } else {
                Phase::Disabled
            },
            config,
            installation: None,
            store,
            pending: None,
            progress: None,
            message,
            poll: None,
            gate: Arc::new(Mutex::new(InstallGate::default())),
        };
        if this.config.is_some() {
            this.check(false, cx);
            this.poll = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_secs(4 * 60 * 60))
                        .await;
                    if this.update(cx, |this, cx| this.check(false, cx)).is_err() {
                        break;
                    }
                }
            }));
        }
        this
    }
    fn busy(&self) -> bool {
        matches!(
            self.phase,
            Phase::Checking | Phase::Downloading | Phase::Installing
        )
    }
    pub(crate) fn actionable(&self) -> bool {
        matches!(
            self.phase,
            Phase::Downloading | Phase::Ready | Phase::Installing | Phase::Failed
        )
    }
    fn changed(&self, cx: &mut Context<Self>) {
        cx.emit(Event::Changed);
        cx.notify();
    }
    /// The installer to download by hand, from the update Worker.
    fn manual_url(&self) -> Option<String> {
        let config = self.config.as_ref()?;
        app_update::manual_download_url(
            &config.endpoint,
            self.installation.as_ref().map(|i| i.kind),
        )
    }
    fn fail(&mut self, error: String, cx: &mut Context<Self>) {
        self.phase = Phase::Failed;
        self.progress = None;
        self.message = Some(error.clone());
        cx.emit(Event::Error(format!(
            "Couldn’t update monoBOT: {error}. You can download it by hand."
        )));
        self.changed(cx);
    }
    pub(crate) fn check(&mut self, manual: bool, cx: &mut Context<Self>) {
        // A verified download is already waiting for a restart.
        if self.busy() || self.phase == Phase::Ready {
            return;
        }
        let Some(config) = self.config.clone() else {
            self.changed(cx);
            return;
        };
        let previous = self.phase;
        self.phase = Phase::Checking;
        self.message = None;
        self.changed(cx);
        let installation = self.installation.clone();
        let work = cx.background_executor().spawn(async move {
            let installation = match installation {
                Some(installation) => installation,
                None => Installation::detect()?,
            };
            let update = app_update::check(&config, &installation)?;
            Ok::<_, String>((installation, update))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((installation, Some(update))) => {
                        this.installation = Some(installation);
                        this.pending = Some(update);
                        this.download(cx);
                        return;
                    }
                    Ok((installation, None)) => {
                        this.installation = Some(installation);
                        this.pending = None;
                        this.phase = Phase::Current;
                    }
                    Err(error) => {
                        // Offline: a failed update keeps offering its installer.
                        this.phase = if previous == Phase::Failed {
                            Phase::Failed
                        } else {
                            Phase::Idle
                        };
                        this.message = Some(error.clone());
                        if manual {
                            cx.emit(Event::Error(format!("Couldn’t check for updates: {error}")));
                        }
                    }
                }
                this.changed(cx);
            });
        })
        .detach();
    }
    /// Downloads and verifies the pending update in the background.
    fn download(&mut self, cx: &mut Context<Self>) {
        let (Some(update), Some(installation)) = (self.pending.clone(), self.installation.clone())
        else {
            return;
        };
        self.phase = Phase::Downloading;
        self.progress = Some(0);
        self.message = None;
        self.changed(cx);
        let (tx, rx) = async_channel::bounded::<Option<u64>>(8);
        let work = cx.background_executor().spawn(async move {
            let mut last_bytes = 0;
            let mut last_percent = None;
            update
                .download(|n, total| {
                    let percent = total
                        .filter(|n| *n > 0)
                        .map(|total| (n.saturating_mul(100) / total).min(100));
                    if (percent.is_some() && percent != last_percent)
                        || (percent.is_none() && n - last_bytes >= 1024 * 1024)
                    {
                        let _ = tx.try_send(percent);
                        last_percent = percent;
                        last_bytes = n;
                    }
                })
                .map(|download| (update, download))
        });
        cx.spawn(async move |this, cx| {
            while let Ok(percent) = rx.recv().await {
                let shown = this.update(cx, |this, cx| {
                    this.progress = percent;
                    this.changed(cx);
                });
                if shown.is_err() {
                    return;
                }
            }
            let result = work.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok((update, download)) => {
                    this.gate.lock().unwrap_or_else(|e| e.into_inner()).ready = Some(Ready {
                        update,
                        installation,
                        download,
                        store: this.store.clone(),
                    });
                    this.phase = Phase::Ready;
                    this.progress = None;
                    this.changed(cx);
                }
                Err(error) => this.fail(error, cx),
            });
        })
        .detach();
    }
    /// Installs the downloaded update now and reopens the app.
    fn install(&mut self, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        let gate = self.gate.clone();
        let ready = {
            let mut state = gate.lock().unwrap_or_else(|e| e.into_inner());
            if state.quitting {
                return;
            }
            let Some(ready) = state.ready.take() else {
                return;
            };
            state.installing = true;
            ready
        };
        self.phase = Phase::Installing;
        self.message = None;
        self.changed(cx); // Close the double-click window before starting I/O.
        let work = cx.background_executor().spawn(async move {
            let Ready {
                update,
                installation,
                download,
                store,
            } = ready;
            let result = mark_installing(&store, &update)
                .and_then(|_| update.install(download, &installation));
            if result.is_err() {
                let _ = locked(&store).and_then(|store| store.save_setting(INSTALLED, ""));
            }
            gate.lock().unwrap_or_else(|e| e.into_inner()).installing = false;
            result
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(Installed::Relaunch(path)) => {
                    if let Err(error) = app_update::schedule_relaunch(&path) {
                        this.phase = Phase::Current;
                        this.message = Some(error.clone());
                        cx.emit(Event::Error(format!(
                            "The update was installed. Reopen monoBOT to finish: {error}"
                        )));
                        this.changed(cx);
                    } else {
                        crate::quit(cx);
                    }
                }
                Ok(Installed::InstallerStarted) => crate::quit(cx),
                Err(error) => {
                    this.fail(error, cx);
                    if this.gate.lock().unwrap_or_else(|e| e.into_inner()).quitting {
                        crate::quit(cx);
                    }
                }
            });
        })
        .detach();
    }
}
/// What the updater offers right now, for Settings' About button.
pub(crate) enum Action {
    Check,
    /// Restart into the downloaded version.
    Restart(String),
    Busy(String),
    /// Download this version's installer by hand: the version and its URL.
    Manual(String, String),
}
pub(crate) fn action(entity: &Entity<Updater>, cx: &App) -> Action {
    let state = entity.read(cx);
    match state.phase {
        Phase::Ready => Action::Restart(state.pending_version()),
        Phase::Failed => match state.manual_url() {
            Some(url) => Action::Manual(state.pending_version(), url),
            None => Action::Check,
        },
        Phase::Checking => Action::Busy("Checking…".into()),
        Phase::Downloading | Phase::Installing => Action::Busy(state.progress_label()),
        Phase::Disabled | Phase::Idle | Phase::Current => Action::Check,
    }
}
/// A line for Settings' About description: the last check's outcome, if any.
pub(crate) fn status(entity: &Entity<Updater>, cx: &App) -> Option<String> {
    let state = entity.read(cx);
    match state.phase {
        Phase::Current => Some("You’re on the latest version.".into()),
        Phase::Ready => Some(state.ready_line()),
        Phase::Checking => None,
        _ => state.message.clone(),
    }
}
pub(crate) fn install(entity: &Entity<Updater>, cx: &mut App) {
    entity.update(cx, |updater, cx| updater.install(cx));
}
impl Updater {
    fn pending_version(&self) -> String {
        self.pending
            .as_ref()
            .map(|u| u.version.to_string())
            .unwrap_or_default()
    }
    fn progress_label(&self) -> String {
        match self.phase {
            Phase::Installing => "Installing…".into(),
            _ => self
                .progress
                .map(|p| format!("Downloading {p}%"))
                .unwrap_or_else(|| "Downloading…".into()),
        }
    }
    fn ready_line(&self) -> String {
        let version = self.pending_version();
        let quits = self
            .installation
            .as_ref()
            .is_some_and(Update::installs_on_quit);
        if quits {
            format!("monoBOT {version} is ready. It installs when you restart or quit.")
        } else {
            format!("monoBOT {version} is ready. Restart to install it.")
        }
    }
}
/// The sidebar's update card: progress while an update downloads or installs,
/// a restart once it is ready, and the installer by hand if it failed.
pub(crate) fn row(entity: Entity<Updater>, cx: &App) -> gpui::AnyElement {
    let state = entity.read(cx);
    let (label, glyph, aria) = match state.phase {
        Phase::Ready => (
            format!("Restart to update to v{}", state.pending_version()),
            "arrow-down-circle",
            "Restart monoBOT to install the update",
        ),
        Phase::Failed => (
            format!("Download v{} by hand", state.pending_version()),
            "external-link",
            "Open the monoBOT download page",
        ),
        _ => (state.progress_label(), "loader", ""),
    };
    let clickable = matches!(state.phase, Phase::Ready | Phase::Failed);
    let manual = state.manual_url();
    let glyph = if state.busy() {
        crate::pulse::looping(
            "native-update-spin",
            Duration::from_secs(1),
            crate::pulse::Rate::Smooth,
            |turn| {
                icon("loader", 16.)
                    .text_color(content(0.7))
                    .with_transformation(gpui::Transformation::rotate(gpui::percentage(turn)))
            },
        )
        .into_any_element()
    } else {
        icon(glyph, 16.)
            .text_color(accent_color())
            .into_any_element()
    };
    let mut button = div()
        .id("install-native-update")
        .role(Role::Button)
        .rounded(px(8.))
        .px_2()
        .py_2()
        .flex()
        .items_center()
        .gap_2()
        .bg(if clickable {
            accent_color().opacity(0.15)
        } else {
            content(0.05)
        })
        .text_color(content(if clickable { 1. } else { 0.75 }))
        .child(
            div()
                .size(px(18.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(glyph),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(label),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(11.))
                        .text_color(content(0.4))
                        .child(format!("v{}", version())),
                ),
        );
    if clickable {
        let ready = state.phase == Phase::Ready;
        let action = entity.clone();
        button = button
            .aria_label(aria)
            .cursor_pointer()
            .hover(|s| s.bg(accent_color().opacity(0.2)))
            .on_click(move |_, _, cx| {
                if ready {
                    action.update(cx, |updater, cx| updater.install(cx));
                } else if let Some(url) = &manual {
                    cx.open_url(url);
                }
            });
    }
    div()
        .id("native-app-update")
        .role(Role::Status)
        .w_full()
        .min_w_0()
        .child(button)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use std::sync::{Arc, Mutex};
    fn memory() -> Database {
        Arc::new(Mutex::new(
            monocode_core::store::Store::open(std::path::Path::new(":memory:")).unwrap(),
        ))
    }
    #[gpui::test]
    fn quit_waits_for_installation_to_finish(cx: &mut TestAppContext) {
        let updater = cx.update(|cx| shared(memory(), cx));
        let gate = updater.read_with(cx, |updater, _| updater.gate.clone());
        gate.lock().unwrap().installing = true;
        assert!(cx.update(defer_quit));
        assert!(gate.lock().unwrap().quitting);
        gate.lock().unwrap().installing = false;
        assert!(!cx.update(defer_quit));
    }
    #[gpui::test]
    fn windows_share_one_updater_and_busy_operations_cannot_start_again(cx: &mut TestAppContext) {
        let store = memory();
        let first = cx.update(|cx| shared(store.clone(), cx));
        let second = cx.update(|cx| shared(store, cx));
        assert_eq!(first.entity_id(), second.entity_id());
        first.update(cx, |view, cx| {
            assert!(!view.actionable());
            for phase in [Phase::Downloading, Phase::Installing] {
                view.phase = phase;
                view.progress = Some(42);
                view.install(cx);
                view.check(true, cx);
                assert_eq!(view.phase, phase);
                assert_eq!(view.progress, Some(42));
                assert!(view.actionable());
            }
            view.phase = Phase::Current;
            assert!(!view.actionable());
        });
    }
    #[gpui::test]
    fn a_ready_update_waits_for_a_restart_instead_of_checking_again(cx: &mut TestAppContext) {
        let updater = cx.update(|cx| shared(memory(), cx));
        updater.update(cx, |view, cx| {
            view.phase = Phase::Ready;
            view.check(true, cx);
            assert_eq!(view.phase, Phase::Ready);
            assert!(view.actionable());
            // Nothing downloaded is waiting in the gate: a click does nothing.
            view.install(cx);
            assert_eq!(view.phase, Phase::Ready);
        });
        assert!(matches!(
            cx.update(|cx| action(&updater, cx)),
            Action::Restart(_)
        ));
    }
    #[gpui::test]
    fn a_failed_update_offers_the_installer_by_hand(cx: &mut TestAppContext) {
        let updater = cx.update(|cx| shared(memory(), cx));
        updater.update(cx, |view, cx| {
            view.config = Some(
                Config::new(
                    "https://monobot-updates.example.workers.dev/latest.json",
                    include_str!("../../../monocode-core/src/app_update/fixtures/public-key.txt"),
                    "0.0.1",
                )
                .unwrap(),
            );
            view.fail("Update download was incomplete".into(), cx);
            assert!(view.actionable());
        });
        match cx.update(|cx| action(&updater, cx)) {
            Action::Manual(_, url) => assert!(
                url.starts_with("https://monobot-updates.example.workers.dev/download/"),
                "{url}"
            ),
            _ => panic!("expected the manual download"),
        }
    }
}
