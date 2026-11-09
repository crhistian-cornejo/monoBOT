//! The notice that a project brings its own Claude Code configuration
//! (hooks, MCP servers) that stays off until the user trusts the project
//! (`monocode_core::project_trust`).
use super::*;

impl Conversation {
    /// Looks, off the interface thread, whether this chat's project has
    /// Claude Code configuration the user has not trusted yet.
    pub(super) fn check_project_trust(&mut self, cx: &mut Context<Self>) {
        if self.is_remote() || Kind::from_id(&self.summary.provider) != Kind::Claude {
            self.project_untrusted = false;
            return;
        }
        let cwd = std::path::PathBuf::from(&self.summary.cwd);
        let check = cx
            .background_executor()
            .spawn(async move { monocode_core::project_trust::needs_trust(&cwd) });
        cx.spawn(async move |this, cx| {
            let untrusted = check.await;
            let _ = this.update(cx, |this, cx| {
                if this.project_untrusted != untrusted {
                    this.project_untrusted = untrusted;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn trust_project(&mut self, cx: &mut Context<Self>) {
        let cwd = std::path::PathBuf::from(&self.summary.cwd);
        let saved = cx
            .background_executor()
            .spawn(async move { monocode_core::project_trust::trust(&cwd) });
        cx.spawn(async move |this, cx| {
            let saved = saved.await;
            let _ = this.update(cx, |this, cx| {
                match saved {
                    Ok(()) => this.project_untrusted = false,
                    Err(error) => this.status = format!("Could not trust the project: {error}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn trust_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.project_untrusted {
            return None;
        }
        Some(
            div()
                .debug_selector(|| "project-trust-notice".into())
                .mx_2()
                .mb_2()
                .p_3()
                .rounded_lg()
                .bg(content(0.04))
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(12.))
                .child(icon("shield-alert", 16.))
                .child(div().flex_1().child(
                    "This project brings its own Claude Code settings (hooks, MCP servers). \
                     They stay off until you trust it.",
                ))
                .child(
                    crate::settings::widgets::secondary_button(
                        "project-trust",
                        "Trust project",
                        false,
                        false,
                    )
                    .debug_selector(|| "project-trust".into())
                    .child("Trust project")
                    .on_click(cx.listener(|this, _, _, cx| this.trust_project(cx))),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// A Claude Code chat in a project with configuration of its own shows
    /// the notice; once trusted it goes.
    #[gpui::test]
    fn an_untrusted_projects_claude_settings_are_announced(cx: &mut gpui::TestAppContext) {
        let store = Arc::new(std::sync::Mutex::new(
            monocode_core::store::Store::open(Path::new(":memory:")).unwrap(),
        ));
        let summary = locked(&store)
            .unwrap()
            .create("claude", "/fixture")
            .unwrap();
        let window = cx.update(|cx| {
            crate::accounts::install(store.clone(), cx);
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(summary, store, cx);
                    view.loaded = true;
                    view.project_untrusted = true;
                    view
                })
            })
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("project-trust-notice").is_some());
        assert!(visual.debug_bounds("project-trust").is_some());
        window
            .update(visual, |view, _, cx| {
                view.project_untrusted = false;
                cx.notify();
            })
            .unwrap();
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("project-trust-notice").is_none());
    }
}
