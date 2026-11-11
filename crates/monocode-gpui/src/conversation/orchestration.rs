//! The lead's plan card (OrchestrationPreview) and what the shell needs from a
//! worker chat: the request it is blocked on, steering and answers from the
//! lead, and a composer that stays closed while the lead manages it.
use super::*;
use monocode_core::orchestration::{
    Plan,
    control::{NeedsInput, Question, QuestionOption},
};

pub(crate) struct StartOrchestration {
    pub plan: Plan,
    pub workers: usize,
    pub request: String,
}
pub(crate) struct StopOrchestration;
pub(crate) struct ResumeOrchestration;
/// Open every worker of the lead's run beside it.
pub(crate) struct OpenOrchestrationAgents;
/// `/swarm <request>`: the shell opens a run with this chat as its
/// coordinator, then sends `message` (or hands it back with an error).
pub(crate) struct StartSwarm {
    pub request: String,
    pub message: monocode_core::follow_up::Message,
}
impl EventEmitter<StartSwarm> for Conversation {}

/// Tasks shown before «Show N more tasks».
const FOLDED_TASKS: usize = 4;

struct CardTask {
    id: String,
    files: Vec<String>,
    depends_on: Vec<String>,
    title: Entity<input::Input>,
    prompt: Entity<input::Input>,
    provider: String,
    model: Option<String>,
    effort: Option<String>,
    /// The chosen model's effort levels, as (id, label).
    efforts: Vec<(String, String)>,
    anchor: AnchorSlot,
}

/// The editable copy of the lead's plan: per-task harness, model, title and
/// instructions, and how many workers run at once.
pub(crate) struct PlanCard {
    title: String,
    summary: String,
    tasks: Vec<CardTask>,
    workers: usize,
    expanded: Option<usize>,
    show_all: bool,
    picker: Option<(usize, Entity<ModelList>)>,
    _picker_events: Option<Subscription>,
    request: String,
}

impl PlanCard {
    pub(crate) fn new(
        plan: &Plan,
        request: String,
        provider: &str,
        model: Option<String>,
        efforts: Vec<(String, String)>,
        cx: &mut Context<Conversation>,
    ) -> Self {
        let tasks = plan
            .tasks
            .iter()
            .map(|task| {
                let title = cx.new(|cx| {
                    let mut input =
                        input::Input::field("Task title", "Task title", content(0.4), cx);
                    input.set_text(task.title.clone(), cx);
                    input
                });
                let prompt = cx.new(|cx| {
                    let mut input =
                        input::Input::multiline_field("Instructions", "Instructions", cx);
                    input.set_text(task.prompt.clone(), cx);
                    input
                });
                let task_provider = task.provider.clone().unwrap_or_else(|| provider.to_owned());
                let task_model = task.model.clone().or_else(|| model.clone());
                let lead_model = task_provider == provider && task_model == model;
                CardTask {
                    id: task.id.clone(),
                    files: task.files.clone(),
                    depends_on: task.depends_on.clone(),
                    title,
                    prompt,
                    provider: task_provider,
                    model: task_model,
                    effort: task.effort.clone(),
                    efforts: if lead_model {
                        efforts.clone()
                    } else {
                        Vec::new()
                    },
                    anchor: AnchorSlot::default(),
                }
            })
            .collect();
        Self {
            title: plan.title.clone(),
            summary: plan.summary.clone(),
            tasks,
            workers: monocode_core::orchestration::control::MAX_WORKERS,
            expanded: None,
            show_all: plan.tasks.len() <= FOLDED_TASKS + 1,
            picker: None,
            _picker_events: None,
            request,
        }
    }

    /// The plan as edited, validated again before it starts.
    pub(crate) fn plan(&self, cx: &gpui::App) -> Result<Plan, String> {
        let plan = Plan {
            title: self.title.clone(),
            summary: self.summary.clone(),
            tasks: self
                .tasks
                .iter()
                .map(|task| monocode_core::orchestration::Task {
                    id: task.id.clone(),
                    title: task.title.read(cx).content.trim().to_owned(),
                    prompt: task.prompt.read(cx).content.trim().to_owned(),
                    files: task.files.clone(),
                    depends_on: task.depends_on.clone(),
                    provider: Some(task.provider.clone()),
                    model: task.model.clone(),
                    effort: task.effort.clone(),
                })
                .collect(),
        };
        monocode_core::orchestration::validate(&plan)?;
        Ok(plan)
    }
}

impl Conversation {
    pub(super) fn open_plan_card(&mut self, plan: &Plan, request: String, cx: &mut Context<Self>) {
        let provider = self.summary.provider.clone();
        let model = self.model.clone();
        let efforts = self
            .model_info()
            .map(|m| monocode_core::app_access::effort_choices(Kind::from_id(&provider), m))
            .unwrap_or_default();
        self.plan_card = Some(PlanCard::new(plan, request, &provider, model, efforts, cx));
    }

    /// The worker is managed by `lead`: its composer stays closed and the
    /// lead, not the user, answers for it.
    pub(crate) fn set_managed_by(&mut self, lead: Option<i64>, cx: &mut Context<Self>) {
        if self.managed_by != lead {
            self.managed_by = lead;
            self.input
                .update(cx, |input, _| input.set_read_only(lead.is_some()));
            cx.notify();
        }
    }

    pub(crate) fn set_orchestration_paused(&mut self, paused: bool, cx: &mut Context<Self>) {
        if self.orchestration_paused != paused {
            self.orchestration_paused = paused;
            cx.notify();
        }
    }

    /// The approval or question this worker waits on, oldest first.
    pub(crate) fn orchestration_input(&self, cx: &gpui::App) -> Option<NeedsInput> {
        if let Some(approval) = self.approvals.front() {
            return Some(NeedsInput {
                kind: "approval",
                request_id: approval.id.clone(),
                label: approval.title.clone(),
                detail: approval.detail.clone(),
                questions: Vec::new(),
            });
        }
        let (card, _) = self.questions.front()?;
        let card = card.read(cx);
        let request = card.request();
        let id = |id: &str, ix: usize| {
            if id.is_empty() {
                ix.to_string()
            } else {
                id.to_owned()
            }
        };
        Some(NeedsInput {
            kind: "question",
            request_id: request.id.clone(),
            label: card.notification_label().to_owned(),
            detail: String::new(),
            questions: request
                .items
                .iter()
                .enumerate()
                .map(|(ix, item)| Question {
                    id: id(&item.id, ix),
                    prompt: item.prompt.clone(),
                    multiple: item.multiple,
                    allow_custom: item.allow_custom,
                    options: item
                        .options
                        .iter()
                        .enumerate()
                        .map(|(oi, option)| QuestionOption {
                            id: id(&option.id, oi),
                            label: option.label.clone(),
                        })
                        .collect(),
                })
                .collect(),
        })
    }

    /// The lead's answer to the question at the front, as the card's own.
    pub(crate) fn answer_external(
        &mut self,
        request_id: &str,
        answers: Option<Vec<Vec<String>>>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((card, _)) = self
            .questions
            .front()
            .filter(|(card, _)| card.read(cx).id() == request_id)
        else {
            return false;
        };
        card.clone().update(cx, |card, cx| card.reply(answers, cx));
        true
    }

    /// Redirect the running turn without discarding its work.
    pub(crate) fn steer_external(&mut self, text: String, cx: &mut Context<Self>) -> bool {
        if !self.busy || self.stopping || !Kind::from_id(&self.summary.provider).can_steer() {
            return false;
        }
        let mut message = self.message(text, Vec::new(), None, cx);
        message.steer = true;
        self.follow_ups.requested = Some(message.id.clone());
        self.follow_ups.state.messages.push(message);
        self.save_follow_ups(cx);
        cx.notify();
        true
    }

    /// Test hook: the worker waits on `approval`; decisions reach `controls`.
    #[cfg(test)]
    pub(crate) fn wait_on_approval(
        &mut self,
        approval: Approval,
        cx: &mut Context<Self>,
    ) -> std::sync::mpsc::Receiver<Control> {
        let (send, receive) = std::sync::mpsc::channel();
        self.controls = Some(send);
        self.approvals.push_back(approval);
        cx.emit(SessionChanged);
        receive
    }

    #[cfg(test)]
    pub(crate) fn managed_by(&self) -> Option<i64> {
        self.managed_by
    }
    #[cfg(test)]
    pub(crate) fn orchestration_paused(&self) -> bool {
        self.orchestration_paused
    }
    /// Test hook: a provider turn that is running, then ends as `status`.
    #[cfg(test)]
    pub(crate) fn test_turn(&mut self, busy: bool, status: &str, cx: &mut Context<Self>) {
        self.busy = busy;
        self.status = status.into();
        cx.emit(SessionChanged);
    }

    fn managed_notice(&self) -> Option<AnyElement> {
        self.managed_by?;
        Some(
            div()
                .id("orchestration-managed")
                .mb_2()
                .px_3()
                .py_2()
                .rounded_lg()
                .bg(content(0.04))
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(12.))
                .text_color(content(0.6))
                .child(icon("share", 14.).text_color(content(0.45)))
                .child("This agent is managed by the orchestrator. Send instructions through its lead.")
                .into_any_element(),
        )
    }

    fn choose_task_model(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(card) = &self.plan_card else {
            return;
        };
        let Some(task) = card.tasks.get(ix) else {
            return;
        };
        let kind = Kind::from_id(&task.provider);
        let project = self.summary.cwd.clone();
        let profile = crate::accounts::get(cx)
            .map(|a| a.read(cx).selected(kind, &project))
            .unwrap_or_default();
        let current = task.model.clone();
        let picker = cx.new(|cx| ModelList::new_for(kind, current, &project, profile, cx));
        let events = cx.subscribe_in(
            &picker,
            window,
            move |this, picker, picked: &ModelPicked, _, cx| {
                let efforts = picker
                    .read(cx)
                    .catalog_model(picked.0, picked.1.as_deref())
                    .map(|m| monocode_core::app_access::effort_choices(picked.0, m))
                    .unwrap_or_default();
                if let Some(card) = &mut this.plan_card
                    && let Some(task) = card.tasks.get_mut(ix)
                {
                    // Effort levels belong to a model.
                    if task.provider != picked.0.id() || task.model != picked.1 {
                        task.effort = None;
                    }
                    task.efforts = efforts;
                    task.provider = picked.0.id().to_owned();
                    task.model = picked.1.clone();
                    card.picker = None;
                    card._picker_events = None;
                }
                cx.notify();
            },
        );
        window.focus(&picker.read(cx).search_focus(cx), cx);
        if let Some(card) = &mut self.plan_card {
            card.picker = Some((ix, picker));
            card._picker_events = Some(events);
        }
        cx.notify();
    }

    fn start_plan_card(&mut self, cx: &mut Context<Self>) {
        let Some(card) = &self.plan_card else {
            return;
        };
        match card.plan(cx) {
            Ok(plan) => {
                let start = StartOrchestration {
                    plan,
                    workers: card.workers,
                    request: card.request.clone(),
                };
                self.orchestration_error = None;
                cx.emit(start);
            }
            Err(error) => self.orchestration_error = Some(error),
        }
        cx.notify();
    }

    fn plan_task_row(&self, ix: usize, task: &CardTask, cx: &mut Context<Self>) -> AnyElement {
        let card = self.plan_card.as_ref().unwrap();
        let open = card.expanded == Some(ix);
        let kind = Kind::from_id(&task.provider);
        let model = model_name(kind, task.model.as_deref());
        let title = task.title.read(cx).content.to_string();
        let field = |input: &Entity<input::Input>| {
            div()
                .rounded_md()
                .border_1()
                .border_color(content(0.1))
                .bg(background_base())
                .px(px(10.))
                .py(px(6.))
                .child(input.clone())
        };
        div()
            .flex()
            .flex_col()
            .gap_1p5()
            .py_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id(("plan-task", ix))
                            .debug_selector(move || format!("plan-task-{ix}"))
                            .role(Role::Button)
                            .aria_label(SharedString::from(format!("Details for {title}")))
                            .aria_expanded(open)
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .items_center()
                            .gap_1()
                            .cursor_pointer()
                            .child(
                                icon(
                                    if open {
                                        "chevron-down"
                                    } else {
                                        "chevron-right"
                                    },
                                    12.,
                                )
                                .text_color(content(0.45)),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .child(format!("{}. {title}", ix + 1)),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(card) = &mut this.plan_card {
                                    card.expanded = (card.expanded != Some(ix)).then_some(ix);
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id(("plan-task-model", ix))
                            .debug_selector(move || format!("plan-task-model-{ix}"))
                            .role(Role::Button)
                            .aria_label(SharedString::from(format!("Model for {title}")))
                            .relative()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap_1()
                            .h(px(22.))
                            .px(px(6.))
                            .rounded_md()
                            .text_size(px(11.))
                            .text_color(content(0.7))
                            .cursor_pointer()
                            .hover(|s| s.bg(content(0.08)))
                            .child(task.anchor.probe())
                            .child(provider_icon(&task.provider, 12.))
                            .child(model)
                            .child(icon("chevron-down", 10.).text_color(content(0.4)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.choose_task_model(ix, window, cx)
                            })),
                    )
                    .when(!task.efforts.is_empty(), |d| {
                        let label = task
                            .effort
                            .as_ref()
                            .and_then(|e| task.efforts.iter().find(|(id, _)| id == e))
                            .map_or("Default effort".to_owned(), |(_, label)| label.clone());
                        d.child(
                            div()
                                .id(("plan-task-effort", ix))
                                .debug_selector(move || format!("plan-task-effort-{ix}"))
                                .role(Role::Button)
                                .aria_label(SharedString::from(format!("Effort for {title}")))
                                .flex_shrink_0()
                                .h(px(22.))
                                .px(px(6.))
                                .flex()
                                .items_center()
                                .rounded_md()
                                .text_size(px(11.))
                                .text_color(content(0.6))
                                .cursor_pointer()
                                .hover(|s| s.bg(content(0.08)))
                                .child(label)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(task) =
                                        this.plan_card.as_mut().and_then(|c| c.tasks.get_mut(ix))
                                    {
                                        // Default, then each level in turn.
                                        let next = task
                                            .effort
                                            .as_ref()
                                            .and_then(|e| {
                                                task.efforts.iter().position(|(id, _)| id == e)
                                            })
                                            .map_or(0, |i| i + 1);
                                        task.effort =
                                            task.efforts.get(next).map(|(id, _)| id.clone());
                                    }
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .pl(px(16.))
                    .text_size(px(10.))
                    .text_color(content(0.4))
                    .child(format!(
                        "{}{}",
                        task.files.join(", "),
                        if task.depends_on.is_empty() {
                            String::new()
                        } else {
                            format!(" · after {}", task.depends_on.join(", "))
                        }
                    )),
            )
            .when(open, |d| {
                d.child(
                    div()
                        .pl(px(16.))
                        .flex()
                        .flex_col()
                        .gap_1p5()
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(content(0.7))
                                .child("Task"),
                        )
                        .child(field(&task.title))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(content(0.7))
                                .child("Instructions"),
                        )
                        .child(field(&task.prompt)),
                )
            })
            .into_any_element()
    }

    fn plan_card_view(&self, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let card = self.plan_card.as_ref()?;
        let visible = if card.show_all {
            card.tasks.len()
        } else {
            FOLDED_TASKS.min(card.tasks.len())
        };
        let hidden = card.tasks.len() - visible;
        let workers = card.workers;
        let stepper = |id: &'static str,
                       label: &'static str,
                       glyph: &'static str,
                       delta: isize,
                       cx: &mut Context<Self>| {
            let next = workers as isize + delta;
            let enabled =
                (1..=monocode_core::orchestration::control::MAX_WORKERS as isize).contains(&next);
            div()
                .id(id)
                .debug_selector(move || id.to_owned())
                .role(Role::Button)
                .aria_label(label)
                .tooltip(crate::theme::chrome_tip(label, None))
                .size(px(22.))
                .rounded_md()
                .flex()
                .items_center()
                .justify_center()
                .text_color(content(if enabled { 0.75 } else { 0.25 }))
                .when(enabled, |d| {
                    d.cursor_pointer().hover(|s| s.bg(content(0.08)))
                })
                .child(glyph)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if enabled && let Some(card) = &mut this.plan_card {
                        card.workers = next as usize;
                    }
                    cx.notify();
                }))
        };
        let (start_fill, start_ink) =
            primary_action(true).unwrap_or((content(0.9), background_base()));
        let mut view = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_color(content(0.6))
                    .child(monocode_core::response::limit(&card.summary, 600)),
            )
            .child(
                div()
                    .id("orchestration-tasks")
                    .max_h(px(320.))
                    .overflow_y_scroll()
                    .children(
                        card.tasks
                            .iter()
                            .take(visible)
                            .enumerate()
                            .map(|(ix, task)| self.plan_task_row(ix, task, cx)),
                    ),
            );
        if card.tasks.len() > FOLDED_TASKS + 1 {
            view = view.child(
                div()
                    .id("orchestration-show-tasks")
                    .debug_selector(|| "orchestration-show-tasks".into())
                    .role(Role::Button)
                    .cursor_pointer()
                    .text_size(px(11.))
                    .text_color(content(0.55))
                    .hover(|s| s.text_color(content(0.85)))
                    .child(if hidden == 0 {
                        "Show fewer tasks".to_owned()
                    } else {
                        format!("Show {hidden} more tasks")
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(card) = &mut this.plan_card {
                            card.show_all = !card.show_all;
                        }
                        cx.notify();
                    })),
            );
        }
        view = view.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .id("start-orchestration")
                        .debug_selector(|| "start-orchestration".into())
                        .role(Role::Button)
                        .aria_label("Start agents")
                        .h(px(26.))
                        .px(px(10.))
                        .rounded_md()
                        .flex()
                        .items_center()
                        .bg(start_fill)
                        .text_color(start_ink)
                        .cursor_pointer()
                        .child("Start agents")
                        .on_click(cx.listener(|this, _, _, cx| this.start_plan_card(cx))),
                )
                .child(
                    tool_pill("dismiss-orchestration", "Dismiss plan")
                        .child("Dismiss")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.orchestration_plan = None;
                            this.plan_card = None;
                            cx.notify();
                        })),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_size(px(11.))
                        .text_color(content(0.6))
                        .child("Parallel workers")
                        .child(stepper(
                            "workers-fewer",
                            "Fewer parallel workers",
                            "−",
                            -1,
                            cx,
                        ))
                        .child(
                            div()
                                .min_w(px(14.))
                                .text_center()
                                .text_color(content(0.9))
                                .child(workers.to_string()),
                        )
                        .child(stepper("workers-more", "More parallel workers", "+", 1, cx)),
                ),
        );
        if let Some((ix, picker)) = &card.picker
            && let Some(task) = card.tasks.get(*ix)
        {
            let weak = cx.weak_entity();
            view = view.child(
                popover("plan-task-model-picker", Anchor::Slot(task.anchor.clone()))
                    .width(340.)
                    .on_dismiss(move |_, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            if let Some(card) = &mut this.plan_card {
                                card.picker = None;
                                card._picker_events = None;
                            }
                            cx.notify();
                        });
                    })
                    .child(div().h(px(440.)).child(picker.clone())),
            );
        }
        Some(view)
    }

    pub(super) fn orchestration_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if let Some(notice) = self.managed_notice() {
            return Some(notice);
        }
        let planning = self.orchestration_plan.is_some() && self.plan_card.is_some();
        if !planning && self.orchestration_error.is_none() && self.orchestration_progress.is_none()
        {
            return None;
        }
        let mut card = div()
            .id("orchestration-card")
            .mb_2()
            .p_3()
            .rounded_lg()
            .border_1()
            .border_color(accent_color().opacity(0.2))
            .bg(accent_color().opacity(0.04))
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icon("share", 16.).text_color(accent_color().opacity(0.8)))
                    .child(
                        self.orchestration_plan
                            .as_ref()
                            .map(|p| p.title.clone())
                            .unwrap_or_else(|| "Orchestrator".into()),
                    ),
            );
        if planning && self.orchestration_progress.is_none() {
            card = card.children(self.plan_card_view(cx));
        }
        if let Some(error) = &self.orchestration_error {
            card = card
                .child(div().text_color(tint(AMBER_400, 0.9)).child(error.clone()))
                .child(
                    tool_pill("dismiss-plan-error", "Dismiss")
                        .child("Dismiss")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.orchestration_error = None;
                            cx.notify();
                        })),
                );
        }
        if let Some(progress) = &self.orchestration_progress {
            if self.orchestration_paused {
                card = card.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py_1p5()
                        .rounded_md()
                        .bg(tint(AMBER_400, 0.08))
                        .child(icon("circle-alert", 14.).text_color(tint(AMBER_400, 0.9)))
                        .child(div().flex_1().child("Continue interrupted and queued work"))
                        .child(
                            tool_pill("resume-orchestration", "Resume")
                                .child("Resume")
                                .on_click(cx.listener(|_, _, _, cx| cx.emit(ResumeOrchestration))),
                        ),
                );
            }
            card = card
                .child(
                    div()
                        .id("orchestration-progress")
                        .max_h(px(200.))
                        .overflow_y_scroll()
                        .text_color(content(0.6))
                        .child(progress.clone()),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            tool_pill("open-orchestration-agents", "Open agents")
                                .child("Open agents")
                                .on_click(
                                    cx.listener(|_, _, _, cx| cx.emit(OpenOrchestrationAgents)),
                                ),
                        )
                        .child(
                            tool_pill("stop-orchestration", "Stop agents")
                                .child("Stop agents")
                                .on_click(cx.listener(|_, _, _, cx| cx.emit(StopOrchestration))),
                        ),
                );
        }
        Some(card.into_any_element())
    }
}

impl Conversation {
    /// The run for a /swarm message is ready: send it.
    pub(crate) fn launch_swarm(
        &mut self,
        message: monocode_core::follow_up::Message,
        cx: &mut Context<Self>,
    ) {
        self.launch_message(message, cx);
    }
    /// The run could not start: the message goes back to the composer.
    pub(crate) fn swarm_failed(
        &mut self,
        message: monocode_core::follow_up::Message,
        error: String,
        cx: &mut Context<Self>,
    ) {
        if self.input.read(cx).content.trim().is_empty() {
            self.input
                .update(cx, |input, cx| input.set_text(message.text.clone(), cx));
            self.attachments = message.attachments;
        }
        self.status = error;
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext};
    use monocode_core::store::Store;
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::{Arc, Mutex},
    };

    fn plan(tasks: usize) -> Plan {
        Plan {
            title: "Split the work".into(),
            summary: "Six independent assignments.".into(),
            tasks: (0..tasks)
                .map(|ix| monocode_core::orchestration::Task {
                    id: format!("task-{ix}"),
                    title: format!("Assignment {ix}"),
                    prompt: format!("Do part {ix}"),
                    files: vec![format!("src/part{ix}")],
                    ..Default::default()
                })
                .collect(),
        }
    }

    #[gpui::test]
    fn the_plan_card_edits_each_task_and_the_worker_count_before_it_starts(
        cx: &mut TestAppContext,
    ) {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let summary = store.create("claude", "/tmp").unwrap();
        let window = cx.update(|cx| {
            input::bind(cx);
            cx.set_global(crate::settings::Preferences {
                composer_mascot: false,
                ..Default::default()
            });
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(summary, Arc::new(Mutex::new(store)), cx);
                    view.loaded = true;
                    view.summary.has_messages = true;
                    view.model = Some("sonnet".into());
                    view
                })
            })
            .unwrap()
        });
        let started = Rc::new(RefCell::new(Vec::new()));
        let sink = started.clone();
        window
            .update(cx, |view, _, cx| {
                cx.subscribe(&cx.entity(), move |_, _, event: &StartOrchestration, _| {
                    sink.borrow_mut().push((
                        event.plan.clone(),
                        event.workers,
                        event.request.clone(),
                    ))
                })
                .detach();
                let plan = plan(6);
                view.open_plan_card(&plan, "Fix the bug".into(), cx);
                view.orchestration_plan = Some(plan);
                cx.notify();
            })
            .unwrap();
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        let click = |visual: &mut VisualTestContext, selector: &'static str| {
            let bounds = visual
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} is drawn"));
            visual.simulate_click(bounds.center(), Default::default());
            visual.run_until_parked();
            visual.refresh().unwrap();
        };
        assert!(visual.debug_bounds("plan-task-3").is_some());
        assert!(
            visual.debug_bounds("plan-task-4").is_none(),
            "long plans fold"
        );
        click(visual, "orchestration-show-tasks");
        assert!(
            visual.debug_bounds("plan-task-5").is_some(),
            "Show 2 more tasks"
        );
        click(visual, "plan-task-0");
        click(visual, "workers-fewer");
        click(visual, "plan-task-model-0");
        window
            .update(visual, |view, _, cx| {
                let card = view.plan_card.as_ref().unwrap();
                assert_eq!(card.expanded, Some(0), "its Task and Instructions are open");
                assert_eq!(card.workers, 3);
                card.tasks[0]
                    .title
                    .update(cx, |input, cx| input.set_text("Renamed task".into(), cx));
                card.tasks[0].prompt.update(cx, |input, cx| {
                    input.set_text("Edited instructions".into(), cx)
                });
                let (ix, picker) = card.picker.clone().expect("the model picker opens");
                assert_eq!(ix, 0);
                picker.update(cx, |_, cx| {
                    cx.emit(ModelPicked(Kind::Codex, Some("gpt-5.1".into())))
                });
            })
            .unwrap();
        visual.run_until_parked();
        window
            .update(visual, |view, _, cx| {
                let card = view.plan_card.as_mut().unwrap();
                card.tasks[1].efforts =
                    vec![("low".into(), "Low".into()), ("high".into(), "High".into())];
                cx.notify();
            })
            .unwrap();
        visual.refresh().unwrap();
        click(visual, "plan-task-effort-1");
        click(visual, "plan-task-effort-1");
        click(visual, "start-orchestration");
        let started = started.borrow();
        let (plan, workers, request) = started.first().expect("Start agents emits the plan");
        assert_eq!(*workers, 3);
        assert_eq!(request, "Fix the bug");
        assert_eq!(plan.tasks[0].title, "Renamed task");
        assert_eq!(plan.tasks[0].prompt, "Edited instructions");
        assert_eq!(plan.tasks[0].provider.as_deref(), Some("codex"));
        assert_eq!(plan.tasks[0].model.as_deref(), Some("gpt-5.1"));
        assert_eq!(plan.tasks[1].provider.as_deref(), Some("claude"));
        assert_eq!(
            plan.tasks[1].model.as_deref(),
            Some("sonnet"),
            "the lead's by default"
        );
        assert_eq!(plan.tasks[1].effort.as_deref(), Some("high"));
        assert_eq!(plan.tasks[2].effort, None);
    }
}
