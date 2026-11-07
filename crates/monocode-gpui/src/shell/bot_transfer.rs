//! What Settings asks of the bots (BotTransferControls, BotSyncControls and
//! BotSyncRunner.tsx): save them to a file, import a file, choose the sync
//! folder, and compare the bots a sync found changed on both computers. The
//! shell opens the one import dialog for both; the Monos manager does the work.
use super::*;
use crate::monos::{
    SyncUpdated,
    bot_import::{BotImportDialog, Event as ImportEvent},
    bot_transfer::PreparedImport,
};
use monocode_core::{
    mono::{
        sync,
        transfer::{self, Bot, Export},
    },
    time::now_ms,
};
use std::path::PathBuf;

/// The open import dialog.
pub(super) struct OpenImport {
    view: Entity<BotImportDialog>,
    /// The bots a sync found changed on both computers, while it decides them.
    sync: Option<Vec<sync::Conflict>>,
    _events: Subscription,
}

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

impl Shell {
    /// Watches the Monos manager's sync, which ends runs and finds conflicts.
    pub(super) fn watch_bot_sync(
        &mut self,
        monos: &Entity<crate::monos::Monos>,
        cx: &mut Context<Self>,
    ) {
        self.observations
            .push(cx.subscribe(monos, |_, _, _: &SyncUpdated, cx| {
                cx.spawn(async move |this, cx| {
                    let _ =
                        this.update_in(cx, |this, window, cx| this.bot_sync_updated(window, cx));
                })
                .detach();
            }));
    }

    /// A connector's switch: saved where the native app keeps preferences,
    /// and new turns use it at once.
    pub(super) fn save_connector(&mut self, id: &str, on: bool, cx: &mut Context<Self>) {
        let saved = crate::runtime::locked(&self.store)
            .and_then(|db| monocode_core::connectors::save_enabled(&db, id, on));
        if let Err(error) = saved {
            eprintln!("Could not save the {id} connector: {error}");
        }
        self.sync_settings(cx);
    }

    /// A skill pack's switch. The composer's /skill list notices the change
    /// the next time it opens.
    pub(super) fn save_skill_pack(&mut self, id: &str, on: bool, cx: &mut Context<Self>) {
        let saved = crate::runtime::locked(&self.store)
            .and_then(|db| monocode_core::skills::bundled::save_skill_pack(&db, id, on));
        if let Err(error) = saved {
            eprintln!("Could not save the {id} skill pack: {error}");
        }
        self.sync_settings(cx);
    }

    fn bots_status(&mut self, text: impl Into<String>, error: bool, cx: &mut Context<Self>) {
        if let Some(settings) = self.settings.clone() {
            let status = Some((text.into(), error));
            settings.update(cx, |settings, cx| settings.set_bots_status(status, cx));
        }
    }

    /// Export bots: build the file, ask where to save it, write it.
    pub(super) fn export_bots(&mut self, cx: &mut Context<Self>) {
        let Some(monos) = self.monos.clone() else {
            self.bots_status(transfer::NO_BOTS, true, cx);
            return;
        };
        let built = monos.update(cx, |monos, cx| monos.export_bots(cx));
        cx.spawn(async move |this, cx| {
            let (text, count) = match built.await {
                Ok(done) => done,
                Err(error) => {
                    let _ = this.update(cx, |this, cx| this.bots_status(error, true, cx));
                    return;
                }
            };
            let Ok(prompt) = this.update(cx, |_, cx| {
                cx.prompt_for_new_path(&home(), Some(&transfer::default_file_name(now_ms())))
            }) else {
                return;
            };
            // Cancelling the dialog is not a failure.
            let Ok(Ok(Some(path))) = prompt.await else {
                return;
            };
            let target = path.clone();
            let write = cx
                .background_executor()
                .spawn(async move { std::fs::write(&target, text) });
            let result = write.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(()) => this.bots_status(
                    format!(
                        "{count} bot{} saved to {}",
                        if count == 1 { "" } else { "s" },
                        path.display()
                    ),
                    false,
                    cx,
                ),
                Err(error) => this.bots_status(format!("{}: {error}", path.display()), true, cx),
            });
        })
        .detach();
    }

    /// Import bots: pick a file, read it, then compare what already exists.
    pub(super) fn import_bots(&mut self, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a bots file".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = prompt.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let read = cx
                .background_executor()
                .spawn(async move { std::fs::read_to_string(&path).map_err(|e| e.to_string()) });
            let file = match read.await.and_then(|raw| Export::parse(&raw)) {
                Ok(file) => file,
                Err(error) => {
                    let _ = this.update(cx, |this, cx| this.bots_status(error, true, cx));
                    return;
                }
            };
            if file.bots.is_empty() {
                let _ = this.update(cx, |this, cx| {
                    this.bots_status("That file has no bots in it.", false, cx)
                });
                return;
            }
            let prepared = this.update(cx, |this, cx| {
                let monos = this.monos.clone()?;
                let bots = file.bots.clone();
                Some(monos.update(cx, |monos, cx| monos.prepare_import(bots, None, cx)))
            });
            let Ok(Some(prepared)) = prepared else {
                return;
            };
            match prepared.await {
                Ok(prepared) => {
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.open_bot_import(
                            &file.platform,
                            &file.exported_at,
                            prepared,
                            None,
                            window,
                            cx,
                        )
                    });
                }
                Err(error) => {
                    let _ = this.update(cx, |this, cx| this.bots_status(error, true, cx));
                }
            }
        })
        .detach();
    }

    /// Choose folder / Change folder.
    pub(super) fn choose_bot_sync_folder(&mut self, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Folder your computers share (OneDrive, iCloud Drive, Dropbox…)".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = prompt.await else {
                return;
            };
            let Some(folder) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.bot_sync_folder(Some(folder.to_string_lossy().into_owned()), cx)
            });
        })
        .detach();
    }

    /// Chooses the shared folder (and syncs at once), or turns syncing off.
    pub(super) fn bot_sync_folder(&mut self, folder: Option<String>, cx: &mut Context<Self>) {
        if let Some(monos) = self.monos.clone() {
            monos.update(cx, |monos, cx| monos.sync_set_folder(folder, cx));
        }
        self.sync_settings(cx);
    }

    pub(super) fn bot_sync_now(&mut self, cx: &mut Context<Self>) {
        if let Some(monos) = self.monos.clone() {
            monos.update(cx, |monos, cx| monos.sync_now(cx));
        }
    }

    /// A sync ended: Settings shows how it went, and bots both computers
    /// changed open the comparison unless one is already open.
    fn bot_sync_updated(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_settings(cx);
        if self.bot_import.is_some() {
            return;
        }
        let Some(monos) = self.monos.clone() else {
            return;
        };
        if !monos.read(cx).has_sync_conflicts() {
            return;
        }
        let conflicts = monos.update(cx, |monos, _| monos.take_sync_conflicts());
        let Some(first) = conflicts.first() else {
            return;
        };
        let platform = sync::platform_of(&first.machine_name);
        let bots: Vec<Bot> = conflicts.iter().map(|c| c.bot.bot.clone()).collect();
        let matches = conflicts
            .iter()
            .map(|c| Some(c.local_id.clone()))
            .collect::<Vec<_>>();
        let prepared = monos.update(cx, |monos, cx| {
            monos.prepare_import(bots, Some(matches), cx)
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(prepared) = prepared.await {
                let _ = this.update_in(cx, |this, window, cx| {
                    this.open_bot_import(platform, "", prepared, Some(conflicts), window, cx)
                });
            }
        })
        .detach();
    }

    fn open_bot_import(
        &mut self,
        platform: &str,
        exported_at: &str,
        prepared: PreparedImport,
        sync: Option<Vec<sync::Conflict>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.bot_import.is_some() {
            return;
        }
        let harness = crate::changes::pick_text_harness(
            Kind::Claude,
            self.workspace.preferences.background_text_model(),
        );
        let max_height = window.viewport_size().height.as_f32() * 0.8;
        let view = cx.new(|cx| {
            BotImportDialog::new(
                platform,
                exported_at,
                prepared,
                harness,
                home(),
                max_height,
                cx,
            )
        });
        let events = cx.subscribe_in(
            &view,
            window,
            |this, view, event: &ImportEvent, window, cx| match event {
                ImportEvent::Close => this.close_bot_import(window, cx),
                ImportEvent::Apply(plan) => {
                    let Some(monos) = this.monos.clone() else {
                        return;
                    };
                    let result = monos.update(cx, |monos, cx| monos.apply_bot_import(plan, cx));
                    match result {
                        Ok(summary) => {
                            let settled =
                                this.bot_import.as_mut().and_then(|open| open.sync.take());
                            this.close_bot_import(window, cx);
                            match settled {
                                // Whatever was chosen, the other computer's
                                // version is now known here.
                                Some(conflicts) => {
                                    monos.update(cx, |monos, cx| {
                                        monos.settle_sync(&conflicts);
                                        monos.sync_now(cx);
                                    });
                                }
                                None => this.bots_status(summary, false, cx),
                            }
                        }
                        Err(error) => view.update(cx, |view, cx| view.failed(error, cx)),
                    }
                }
            },
        );
        window.focus(&view.focus_handle(cx), cx);
        self.bot_import = Some(OpenImport {
            view,
            sync,
            _events: events,
        });
        cx.notify();
    }

    fn close_bot_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.bot_import = None;
        match self.settings.clone() {
            Some(settings) => window.focus(&settings.focus_handle(cx), cx),
            None => self.focus_workspace(window, cx),
        }
        cx.notify();
    }

    pub(super) fn bot_import_modal(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let view = self.bot_import.as_ref()?.view.clone();
        let viewport = window.viewport_size();
        let width = 880f32.min(viewport.width.as_f32() - 24.);
        let measured = view.read(cx).measured_height().unwrap_or(420.);
        let height = (measured + 2.).min(viewport.height.as_f32() - 32.);
        let anchor = Anchor::Point(gpui::point(
            px((viewport.width.as_f32() - width) / 2.),
            px(((viewport.height.as_f32() - height) / 2.).max(16.)),
        ));
        let weak = cx.weak_entity();
        Some(
            div()
                .absolute()
                .size_full()
                .child(crate::modal::scrim("bot-import-scrim"))
                .child(
                    popover("bot-import-modal", anchor)
                        .modal_surface(self.workspace.boxy_layout())
                        .width(width)
                        .gap(0.)
                        .max_height(viewport.height.as_f32() - 32.)
                        .priority(60)
                        .on_dismiss(move |window, cx| {
                            let _ = weak.update(cx, |this, cx| this.close_bot_import(window, cx));
                        })
                        .child(view),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests;
