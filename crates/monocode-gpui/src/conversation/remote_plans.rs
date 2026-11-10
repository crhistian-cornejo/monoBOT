//! Host-owned plan previews and their read-only Markdown reader.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Clone, Default, Deserialize, Serialize)]
pub(crate) struct Plan {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub streaming: bool,
    #[serde(default)]
    pub status: String,
}
impl Plan {
    pub(crate) fn can_build(&self, busy: bool) -> bool {
        !busy
            && !self.streaming
            && !self.text.trim().is_empty()
            && !matches!(self.status.as_str(), "streaming" | "building" | "built")
    }
    pub(crate) fn build_label(&self) -> &'static str {
        match self.status.as_str() {
            "building" => "Building…",
            "built" => "Built",
            _ => "Build",
        }
    }
    pub(crate) fn title(&self) -> String {
        monocode_core::plan::title(&self.text)
    }
    pub(crate) fn summary(&self) -> String {
        monocode_core::plan::summary(&self.text)
    }
    fn command(&self) -> Value {
        json!({"type":"send","intent":"build","planBlockId":self.id})
    }
    fn prompt(&self) -> String {
        format!("Build the approved plan:\n\n{}", self.text)
    }
}
struct Reader {
    id: String,
    text: String,
    view: Entity<TranscriptView>,
    /// A local plan, by its row in the transcript.
    local: Option<usize>,
}
#[derive(Default)]
pub(super) struct State {
    plans: HashMap<String, Plan>,
    reader: Option<Reader>,
}

impl Conversation {
    pub(super) fn close_remote_plan(&mut self, cx: &mut Context<Self>) -> bool {
        if self.remote_plans.reader.take().is_some() {
            cx.notify();
            true
        } else {
            false
        }
    }
    pub(super) fn install_remote_plans(&mut self, snapshot: &Value, cx: &mut Context<Self>) {
        self.remote_plans.plans = snapshot["session"]["blocks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|block| block["role"] == "plan")
            .filter_map(|block| {
                Some((
                    block["id"].as_str()?.to_owned(),
                    Plan {
                        id: block["id"].as_str()?.into(),
                        text: block["text"].as_str().unwrap_or("").into(),
                        streaming: block["streaming"] == true,
                        status: block["plan"]["status"].as_str().unwrap_or("ready").into(),
                    },
                ))
            })
            .collect();
        if let Some(reader) = self.remote_plans.reader.as_mut()
            && let Some(plan) = self.remote_plans.plans.get(&reader.id)
            && reader.text != plan.text
        {
            reader.text = plan.text.clone();
            reader.view = cx.new(|cx| TranscriptView::document(plan.text.clone(), cx));
        }
    }
    pub(super) fn remote_plan_request(
        &mut self,
        id: &str,
        text: &str,
        build: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.is_remote() {
            return;
        }
        if !build {
            self.remote_plans.reader = Some(Reader {
                id: id.into(),
                text: text.into(),
                view: cx.new(|cx| TranscriptView::document(text.into(), cx)),
                local: None,
            });
            cx.notify();
            return;
        }
        if !self.loaded
            || !self.remote_state.online
            || self.remote_state.client.is_none()
            || self.remote_state.pending
            || self.busy
            || self.stopping
        {
            self.status =
                "Reconnect and wait for the remote turn to finish before building this plan."
                    .into();
            cx.notify();
            return;
        }
        if !self.remote_state.capabilities.contains("sessions.plan") {
            self.status = "Update monoBOT Host in Connections settings to build plans.".into();
            cx.notify();
            return;
        }
        let Some(plan) = self
            .remote_plans
            .plans
            .get(id)
            .cloned()
            .filter(|plan| plan.can_build(false))
        else {
            return;
        };
        self.settings.remote_action = Some(plan.command());
        self.remote_plans.reader = None;
        // Launch the host command directly: the composer draft and its local
        // attachments belong to the user's next message, not this approval.
        self.launch_prompt(plan.prompt(), cx);
    }
    /// "Open" on a local plan card: the plan in the reader over the chat.
    pub(super) fn open_local_plan(&mut self, at: usize, text: &str, cx: &mut Context<Self>) {
        self.remote_plans.reader = Some(Reader {
            id: format!("local-{at}"),
            text: text.into(),
            view: cx.new(|cx| TranscriptView::document(text.into(), cx)),
            local: Some(at),
        });
        cx.notify();
    }
    fn local_plan_ready(&self, at: usize, cx: &gpui::App) -> Option<String> {
        if self.is_remote() || self.busy || self.stopping {
            return None;
        }
        let rows = self.transcript.read(cx).history_rows();
        let plan = monocode_core::plan::row_at(rows, at)?;
        (monocode_core::plan::status(rows, at, false) == monocode_core::plan::Status::Ready)
            .then_some(plan.text)
    }
    /// "Build" on a local plan, here or with another model: the plan goes to
    /// the provider as the approved source of truth, under a short message.
    pub(super) fn build_local_plan(
        &mut self,
        at: usize,
        target: Option<monocode_core::response::Target>,
        cx: &mut Context<Self>,
    ) {
        let Some(text) = self.local_plan_ready(at, cx) else {
            if self.busy || self.stopping {
                self.status =
                    "Wait for the current turn to finish before building this plan.".into();
                cx.notify();
            }
            return;
        };
        if let Some(target) = target {
            let Some(provider) = Kind::parse(&target.provider) else {
                return;
            };
            if !self.apply_model_pick(&crate::pickers::ModelPicked(provider, target.model), cx) {
                return;
            }
            self.settings.model_settings.extend(target.settings);
            if let Some(effort) = target.effort {
                self.set_effort_choice(&effort, cx);
            }
        }
        self.remote_plans.reader = None;
        let mut message = self.message(
            monocode_core::plan::BUILD_MESSAGE.into(),
            Vec::new(),
            None,
            cx,
        );
        message.prepared_prompt = Some(monocode_core::plan::build_prompt(&text));
        self.launch_message(message, cx);
        cx.notify();
    }
    pub(super) fn remote_plan_reader(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let reader = self.remote_plans.reader.as_ref()?;
        let id = reader.id.clone();
        let local = reader.local;
        let build = match local {
            Some(at) => self.local_plan_ready(at, cx).is_some(),
            None => self.remote_plans.plans.get(&id).is_some_and(|plan| {
                plan.can_build(
                    self.busy
                        || self.stopping
                        || !self.remote_state.online
                        || self.remote_state.pending,
                )
            }),
        };
        let title = match local {
            Some(_) => monocode_core::plan::title(&reader.text),
            None => self
                .remote_plans
                .plans
                .get(&id)
                .map(Plan::title)
                .unwrap_or_else(|| "Plan".into()),
        };
        let mut fill = pane_fill();
        fill.a = 1.;
        Some(
            div()
                .id("remote-plan-reader")
                .absolute()
                .inset_0()
                .bg(fill)
                .flex()
                .flex_col()
                .overflow_hidden()
                .child(
                    div()
                        .h(px(42.))
                        .px_3()
                        .flex()
                        .items_center()
                        .gap_2()
                        .border_b_1()
                        .border_color(content(0.1))
                        .child(icon("file-text", 16.))
                        .child(div().flex_1().truncate().text_size(px(13.)).child(title))
                        .child(
                            crate::settings::widgets::secondary_button(
                                "remote-reader-build",
                                "Build approved plan",
                                false,
                                !build,
                            )
                            .child("Build")
                            .on_click(cx.listener(
                                move |this, _, _, cx| match (build, local) {
                                    (false, _) => {}
                                    (true, Some(at)) => this.build_local_plan(at, None, cx),
                                    (true, None) => this.remote_plan_request(&id, "", true, cx),
                                },
                            )),
                        )
                        .child(
                            icon_button("close-remote-plan", "Close plan", "x").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.remote_plans.reader = None;
                                    cx.notify();
                                }),
                            ),
                        ),
                )
                .child(div().flex_1().min_h_0().child(reader.view.clone()))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approved_plan_actions_keep_host_identity_and_exact_text() {
        let mut plan = Plan {
            id: "host-plan".into(),
            text: "# Plan\n\n- Keep exact text\n".into(),
            status: "ready".into(),
            streaming: false,
        };
        assert!(plan.can_build(false));
        assert_eq!(
            plan.command(),
            json!({"type":"send","intent":"build","planBlockId":"host-plan"})
        );
        assert_eq!(
            plan.prompt(),
            "Build the approved plan:\n\n# Plan\n\n- Keep exact text\n"
        );
        for state in ["streaming", "building", "built"] {
            plan.status = state.into();
            assert!(!plan.can_build(false));
        }
        plan.status = "ready".into();
        assert!(!plan.can_build(true));
        plan.streaming = true;
        assert!(!plan.can_build(false));
    }
}
