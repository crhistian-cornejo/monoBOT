//! Keeps the same bots on Windows and Mac through a folder both computers
//! share (BotSyncRunner.tsx): every two minutes, and a few seconds after the
//! roster changes. The plan and the files are core's (mono::sync).
use super::*;
use monocode_core::mono::sync;
use monocode_core::time::now_ms;

/// A sync run ended: Settings shows its status, and bots both computers
/// changed wait for the user in `Monos::take_sync_conflicts`.
pub(crate) struct SyncUpdated;

/// What Settings shows of the sync (BotSyncControls).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SyncView {
    pub folder: Option<String>,
    pub status: Option<sync::Status>,
}

#[derive(Default)]
pub(super) struct SyncState {
    running: bool,
    last: u64,
    /// When the roster last changed, so a burst of edits syncs once.
    dirty_at: Option<u64>,
    /// Bots changed on both computers since the last sync, for the user.
    pub(super) conflicts: Vec<sync::Conflict>,
    pub(super) view: SyncView,
}

impl Monos {
    /// A roster change: sync it out after the settle pause.
    pub(super) fn sync_soon(&mut self) {
        self.sync.dirty_at = Some(now_ms());
    }

    /// The folder and last status, for the Settings page.
    pub(crate) fn sync_view(&self) -> SyncView {
        self.sync.view.clone()
    }

    /// Reads the folder and last status from the store.
    pub(super) fn load_sync_view(&mut self) {
        if let Ok(db) = locked(&self.store) {
            self.sync.view = SyncView {
                folder: sync::load_folder(&db),
                status: sync::load_status(&db),
            };
        }
    }

    /// The bots both computers changed, handed to the comparison dialog.
    pub(crate) fn take_sync_conflicts(&mut self) -> Vec<sync::Conflict> {
        std::mem::take(&mut self.sync.conflicts)
    }

    pub(crate) fn has_sync_conflicts(&self) -> bool {
        !self.sync.conflicts.is_empty()
    }

    /// Chooses the shared folder, or stops syncing with `None`.
    pub(crate) fn sync_set_folder(&mut self, folder: Option<String>, cx: &mut Context<Self>) {
        let saved = locked(&self.store).and_then(|db| sync::save_folder(&db, folder.as_deref()));
        if let Err(error) = saved {
            self.error = Some(error);
            cx.notify();
            return;
        }
        self.sync.view.folder = folder.clone();
        if folder.is_none() {
            self.sync.conflicts.clear();
        }
        cx.emit(SyncUpdated);
        if folder.is_some() {
            self.sync_now(cx);
        }
    }

    /// Sync now: runs at once, or right after the one running.
    pub(crate) fn sync_now(&mut self, cx: &mut Context<Self>) {
        self.sync.dirty_at = Some(now_ms().saturating_sub(sync::SETTLE_MS));
        self.sync_tick(cx);
    }

    /// From the clock: syncs when it is time.
    pub(super) fn sync_tick(&mut self, cx: &mut Context<Self>) {
        let now = now_ms();
        let settled = self
            .sync
            .dirty_at
            .is_some_and(|at| now.saturating_sub(at) >= sync::SETTLE_MS);
        let due = now.saturating_sub(self.sync.last) >= sync::EVERY_MS;
        if self.sync.running || !(settled || due) {
            return;
        }
        self.sync.running = true;
        self.sync.last = now;
        self.sync.dirty_at = None;
        let store = self.store.clone();
        let root = self.root.clone();
        let mut roster = self.roster.clone();
        let before = serde_json::to_string(&self.roster.monos).unwrap_or_default();
        let work = cx.background_executor().spawn(async move {
            let db = locked(&store)?;
            // The folder the desktop app chose, until the native app has one
            // (a folder turned off here is saved empty, and stays off).
            if db.setting(sync::FOLDER_KEY).ok().flatten().is_none()
                && let Some(folder) = db
                    .desktop_preferences()
                    .and_then(|p| p.get("monocode.botSyncFolder"))
                    .map(|raw| {
                        serde_json::from_str::<String>(raw).unwrap_or_else(|_| raw.to_owned())
                    })
                    .filter(|f| !f.trim().is_empty())
            {
                let _ = sync::save_folder(&db, Some(&folder));
            }
            let conflicts = sync::run(&db, &root, &mut roster, now_ms());
            let view = SyncView {
                folder: sync::load_folder(&db),
                status: sync::load_status(&db),
            };
            Ok::<_, String>((roster, conflicts, view))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.sync.running = false;
                let Ok((roster, conflicts, view)) = result else {
                    return;
                };
                // A later run must not change an open comparison under the user.
                if this.sync.conflicts.is_empty() {
                    this.sync.conflicts = conflicts;
                }
                this.sync.view = view;
                // Edits made while it ran win; the next run carries them.
                let unchanged =
                    serde_json::to_string(&this.roster.monos).unwrap_or_default() == before;
                if unchanged && serde_json::to_string(&roster.monos).unwrap_or_default() != before {
                    this.roster.monos = roster.monos;
                    this.changed(cx);
                }
                cx.emit(SyncUpdated);
            });
        })
        .detach();
    }
}
