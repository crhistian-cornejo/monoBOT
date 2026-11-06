//! Keeps the same bots on Windows and Mac through a folder both computers
//! share (BotSyncRunner.tsx): every two minutes, and a few seconds after the
//! roster changes. The plan and the files are core's (mono::sync).
use super::*;
use monocode_core::mono::sync;
use monocode_core::time::now_ms;

#[derive(Default)]
pub(super) struct SyncState {
    running: bool,
    last: u64,
    /// When the roster last changed, so a burst of edits syncs once.
    dirty_at: Option<u64>,
    /// Bots changed on both computers since the last sync, for the user.
    pub(super) conflicts: Vec<sync::Conflict>,
}

impl Monos {
    /// A roster change: sync it out after the settle pause.
    pub(super) fn sync_soon(&mut self) {
        self.sync.dirty_at = Some(now_ms());
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
            // The folder the desktop app chose, until the native app has one.
            if sync::load_folder(&db).is_none()
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
            Ok::<_, String>((roster, conflicts))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.sync.running = false;
                let Ok((roster, conflicts)) = result else {
                    return;
                };
                this.sync.conflicts = conflicts;
                // Edits made while it ran win; the next run carries them.
                let unchanged =
                    serde_json::to_string(&this.roster.monos).unwrap_or_default() == before;
                if unchanged && serde_json::to_string(&roster.monos).unwrap_or_default() != before {
                    this.roster.monos = roster.monos;
                    this.changed(cx);
                }
            });
        })
        .detach();
    }
}
