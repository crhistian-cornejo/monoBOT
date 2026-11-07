//! Keeps the computer awake while agents work or a turn is due (keepAwake.ts):
//! every few seconds the shell gathers what its sessions and Monos are doing
//! and holds or releases the system's sleep assertion (core's keep_awake).
use super::Shell;
use gpui::Context;
use monocode_core::keep_awake::{self, Work};

impl Shell {
    pub(super) fn sync_keep_awake(&mut self, cx: &mut Context<Self>) {
        if !keep_awake::AVAILABLE {
            return;
        }
        let mut work: Vec<Work> = self
            .sessions
            .values()
            .map(|view| {
                let view = view.read(cx);
                Work {
                    cwd: view.summary.cwd.clone(),
                    busy: view.busy,
                    retry_scheduled: view.retry_scheduled(),
                    resume_at_reset: view
                        .settings
                        .usage_limit
                        .as_ref()
                        .is_some_and(|limit| limit.resume_at_reset),
                    scheduled_message: view.has_scheduled_message(),
                }
            })
            .collect();
        if let Some(monos) = &self.monos {
            work.extend(monos.read(cx).keep_awake_work(cx));
        }
        let want = self.workspace.preferences.keep_awake && keep_awake::has_work_underway(&work);
        if want != keep_awake::is_held() {
            cx.background_executor()
                .spawn(async move {
                    let _ = keep_awake::set(want);
                })
                .detach();
        }
    }
}
