//! Lead-supervised orchestration (orchestration.ts). The user starts the
//! lead's reviewed plan; the lead then drives its workers through
//! `monocode-gpui control`, whose calls arrive on the app bridge. This module
//! executes what `monocode_core::orchestration::control` decides: it creates
//! the worker sessions and worktrees, routes their approvals and questions to
//! the lead, applies reviewed changes and saves the run so an interrupted one
//! can resume.
use super::*;
use crate::conversation::composer_features::{
    OpenOrchestrationAgents, ResumeOrchestration, StartOrchestration, StopOrchestration,
};
use monocode_core::{
    agent::Settings,
    app_access as app,
    mono::bridge::Call,
    orchestration::{
        self,
        control::{self, Choice, Effect, NeedsInput, Run, RunStatus, Status},
        isolation,
    },
};
use serde_json::{Value, json};

/// A run the app is executing, with what only lives in memory.
pub(super) struct Live {
    run: Run,
    /// `wait` calls blocked until the run changes or they time out.
    waiters: Vec<(u64, Call)>,
    /// Running workers: the dispatch whose turn was submitted, and whether
    /// that turn has been seen busy.
    turns: HashMap<String, (u64, bool)>,
    lead_busy: bool,
    blocked: Vec<String>,
    /// The run as last saved; a different snapshot is saved and wakes waiters.
    saved: String,
}
impl Live {
    fn new(run: Run) -> Self {
        Self {
            run,
            waiters: Vec::new(),
            turns: HashMap::new(),
            lead_busy: false,
            blocked: Vec::new(),
            saved: String::new(),
        }
    }
    pub(super) fn checkout(&self) -> &str {
        &self.run.checkout
    }
}

/// Saves runs in order on one background loop, so an older snapshot never
/// overwrites a newer one.
#[derive(Default)]
pub(super) struct Control {
    saver: Option<async_channel::Sender<(i64, String)>>,
    next_waiter: u64,
    /// A lead whose agents open beside it on the next render.
    open_agents: Option<i64>,
}

/// Runs saved as active were interrupted when the app closed: their workers
/// stopped with it. They come back paused, ready to resume.
pub(super) fn restore(runs: HashMap<i64, Run>) -> HashMap<i64, Live> {
    runs.into_iter()
        .map(|(lead, mut run)| {
            run.pause("monoBOT closed while this run was working. Its agents were stopped.");
            (lead, Live::new(run))
        })
        .collect()
}

/// The provider a worker's session row is created with. Tests use the
/// unknown `fixture` provider, whose turns fail before any agent starts.
fn stored_provider(provider: &str) -> &str {
    if cfg!(test) && provider == "fixture" {
        "codex"
    } else {
        provider
    }
}

/// The executable the lead calls as `… control ACTION`.
fn cli() -> String {
    std::env::current_exe()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "monocode-gpui".into())
}

impl Shell {
    pub(super) fn observe_orchestration(
        &mut self,
        view: &Entity<Conversation>,
        id: i64,
        cx: &mut Context<Self>,
    ) {
        self.observations.push(cx.subscribe(
            view,
            move |this, _, event: &StartOrchestration, cx| {
                this.start_orchestration(
                    id,
                    event.plan.clone(),
                    event.workers,
                    event.request.clone(),
                    cx,
                )
            },
        ));
        self.observations.push(
            cx.subscribe(view, move |this, _, _: &StopOrchestration, cx| {
                this.stop_orchestration(id, cx)
            }),
        );
        self.observations.push(
            cx.subscribe(view, move |this, _, _: &ResumeOrchestration, cx| {
                this.resume_orchestration(id, cx)
            }),
        );
        // Opening panes needs the window: the next render does it.
        self.observations.push(cx.subscribe(
            view,
            move |this, _, _: &OpenOrchestrationAgents, cx| {
                this.orchestration_control.open_agents = Some(id);
                cx.notify();
            },
        ));
        // A restored run shows on its lead, and its workers stay managed.
        if let Some(live) = self.orchestrations.get(&id) {
            let progress = live.run.progress();
            let paused = live.run.status == RunStatus::Paused;
            view.update(cx, |v, cx| {
                v.set_orchestration_paused(paused, cx);
                v.set_orchestration_progress(Some(progress), cx);
            });
            self.publish_orchestration(id, cx);
        }
        if let Some(lead) = self
            .orchestrations
            .iter()
            .find(|(_, live)| live.run.worker(id).is_some())
            .map(|(lead, _)| *lead)
        {
            view.update(cx, |v, cx| v.set_managed_by(Some(lead), cx));
        }
    }

    /// What each running worker is blocked on, read from its chat.
    fn needs(&self, lead: i64, cx: &App) -> HashMap<String, NeedsInput> {
        let Some(live) = self.orchestrations.get(&lead) else {
            return HashMap::new();
        };
        live.run
            .tasks
            .iter()
            .filter(|t| t.status == Status::Running)
            .filter_map(|t| {
                let view = self.sessions.get(&t.session?)?;
                Some((t.id.clone(), view.read(cx).orchestration_input(cx)?))
            })
            .collect()
    }

    fn start_orchestration(
        &mut self,
        lead: i64,
        plan: orchestration::Plan,
        workers: usize,
        request: String,
        cx: &mut Context<Self>,
    ) {
        let fail = |this: &mut Self, error: String, cx: &mut Context<Self>| {
            this.error = Some(error);
            cx.notify();
        };
        if self.orchestrations.contains_key(&lead) {
            return fail(
                self,
                "Resume or stop this chat's current run before starting another.".into(),
                cx,
            );
        }
        if let Err(error) = orchestration::validate(&plan) {
            return fail(self, error, cx);
        }
        let Some(view) = self.sessions.get(&lead) else {
            return;
        };
        let conversation = view.read(cx);
        if conversation.busy {
            return fail(
                self,
                "Wait for the lead's current turn to finish.".into(),
                cx,
            );
        }
        if self.app_bridge.is_none() && !cx.is_test() {
            return fail(
                self,
                "monoBOT app access is unavailable, so the lead cannot supervise workers.".into(),
                cx,
            );
        }
        let project = conversation.summary.cwd.clone();
        let checkout = conversation
            .settings
            .worktree_cwd
            .clone()
            .unwrap_or_else(|| project.clone());
        if self.orchestrations.values().any(|live| {
            monocode_core::files::path_key(live.checkout())
                == monocode_core::files::path_key(&checkout)
        }) {
            return fail(
                self,
                "An orchestration is already using this checkout.".into(),
                cx,
            );
        }
        let provider = conversation.summary.provider.clone();
        let model = conversation.settings.model.clone();
        // The harnesses the lead may assign: the installed ones, with their
        // models, plus whatever the card already assigned.
        let profiles: Vec<_> = if cx.is_test() {
            Vec::new()
        } else {
            Kind::ALL
                .into_iter()
                .map(|kind| {
                    let profile = crate::accounts::get(cx)
                        .map(|center| center.read(cx).selected(kind, &project))
                        .unwrap_or_default();
                    (kind, profile)
                })
                .collect()
        };
        let mut assigned: Vec<(String, Option<String>)> = plan
            .tasks
            .iter()
            .map(|t| {
                (
                    t.provider.clone().unwrap_or_else(|| provider.clone()),
                    t.model.clone().or_else(|| model.clone()),
                )
            })
            .collect();
        assigned.push((provider.clone(), model.clone()));
        let path = checkout.clone();
        let work = cx.background_executor().spawn(async move {
            let isolated = isolation::available(std::path::Path::new(&path));
            let mut choices: Vec<Choice> = profiles
                .into_iter()
                .filter(|(kind, _)| monocode_core::provider::resolve(*kind).is_some())
                .map(|(kind, profile)| Choice {
                    provider: kind.id().into(),
                    models: monocode_core::catalog::models_for(kind, &profile)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|m| m.id)
                        .collect(),
                })
                .collect();
            for (provider, model) in assigned {
                let index = match choices.iter().position(|c| c.provider == provider) {
                    Some(ix) => ix,
                    None => {
                        choices.push(Choice {
                            provider,
                            models: Vec::new(),
                        });
                        choices.len() - 1
                    }
                };
                if let Some(model) = model
                    && !choices[index].models.contains(&model)
                {
                    choices[index].models.insert(0, model);
                }
            }
            (isolated, choices)
        });
        cx.spawn(async move |this, cx| {
            let (isolated, choices) = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.orchestrations.contains_key(&lead) {
                    return;
                }
                let run = match Run::from_plan(
                    lead,
                    &project,
                    &checkout,
                    isolated,
                    &request,
                    &plan,
                    &provider,
                    model.as_deref(),
                    choices,
                    workers,
                ) {
                    Ok(run) => run,
                    Err(error) => {
                        this.error = Some(error);
                        cx.notify();
                        return;
                    }
                };
                let prompt = run.start_prompt(&cli());
                this.orchestrations.insert(lead, Live::new(run));
                if let Some(view) = this.sessions.get(&lead) {
                    view.update(cx, |v, cx| {
                        v.orchestration_plan = None;
                        v.send_coordinator_report(prompt, cx);
                    });
                }
                if let Some(live) = this.orchestrations.get_mut(&lead) {
                    live.lead_busy = true;
                }
                this.pump_orchestration(lead, cx);
            });
        })
        .detach();
    }

    fn stop_orchestration(&mut self, lead: i64, cx: &mut Context<Self>) {
        let Some(live) = self.orchestrations.get_mut(&lead) else {
            return;
        };
        let sessions = live.run.stop();
        let (_, unreviewed) = live.run.release_workers();
        for session in sessions {
            if let Some(view) = self.sessions.get(&session) {
                view.update(cx, |v, cx| v.stop_if_busy(cx));
            }
        }
        // Worktrees with work in them stay for the user to inspect.
        let checkout = live.run.checkout.clone();
        cx.background_executor()
            .spawn(async move {
                for worker in unreviewed {
                    if isolation::changed(&worker).is_ok_and(|files| files.is_empty()) {
                        let _ = isolation::remove(std::path::Path::new(&checkout), &worker);
                    }
                }
            })
            .detach();
        self.publish_orchestration(lead, cx);
    }

    fn resume_orchestration(&mut self, lead: i64, cx: &mut Context<Self>) {
        self.ensure_session(lead, cx);
        let busy = self
            .sessions
            .get(&lead)
            .is_none_or(|v| !v.read(cx).ready_for_external(cx));
        let Some(live) = self.orchestrations.get_mut(&lead) else {
            return;
        };
        if busy {
            self.error = Some("Wait for the lead's turn to finish before resuming.".into());
            cx.notify();
            return;
        }
        if let Err(error) = live.run.resume() {
            self.error = Some(error);
            cx.notify();
            return;
        }
        let prompt = live.run.resume_prompt(&cli());
        live.lead_busy = true;
        if let Some(view) = self.sessions.get(&lead) {
            view.update(cx, |v, cx| v.send_coordinator_report(prompt, cx));
        }
        self.pump_orchestration(lead, cx);
    }

    /// OrchestrationActions.openAgents: every worker beside its lead.
    pub(super) fn open_orchestration_agents_requested(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(lead) = self.orchestration_control.open_agents.take() {
            self.open_orchestration_agents(lead, window, cx);
        }
    }

    fn open_orchestration_agents(
        &mut self,
        lead: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let workers: Vec<i64> = self
            .orchestrations
            .get(&lead)
            .map(|live| live.run.summary())
            .or_else(|| self.orchestration_summaries.get(&lead).cloned())
            .map(|summary| summary.tasks.iter().filter_map(|t| t.session).collect())
            .unwrap_or_default();
        let mut beside = lead;
        for worker in workers {
            if self.ensure_session(worker, cx).is_some() {
                self.open_beside(worker, beside, window, cx);
                beside = worker;
            }
        }
    }

    /// The rail's «Cancel task»: through the run when it is live, so the
    /// lead sees the task cancelled rather than failed.
    pub(super) fn cancel_orchestration_worker(
        &mut self,
        lead: i64,
        worker: i64,
        cx: &mut Context<Self>,
    ) {
        if let Some(live) = self.orchestrations.get_mut(&lead)
            && let Some(task) = live.run.worker(worker).map(|t| t.id.clone())
        {
            live.run.cancel(&task);
            live.turns.remove(&task);
        }
        if let Some(view) = self.sessions.get(&worker) {
            view.update(cx, |v, cx| v.stop_if_busy(cx));
        }
        self.pump_orchestration(lead, cx);
    }

    /// «Open this agent beside the orchestrator».
    pub(super) fn open_beside(
        &mut self,
        worker: i64,
        lead: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.tab_of(worker).is_some() {
            self.open(worker, window, cx);
            return;
        }
        if self.workspace.tab_of(lead).is_none() {
            self.open(lead, window, cx);
        }
        self.app_place_launch(worker, Some((lead, Edge::Right)), true, window, cx);
    }

    pub(super) fn orchestration_session_changed(&mut self, id: i64, cx: &mut Context<Self>) {
        let leads: Vec<i64> = self
            .orchestrations
            .iter()
            .filter(|(lead, live)| **lead == id || live.run.worker(id).is_some())
            .map(|(lead, _)| *lead)
            .collect();
        for lead in leads {
            if lead == id {
                self.lead_changed(lead, cx);
            } else {
                self.worker_changed(lead, id, cx);
            }
            self.pump_orchestration(lead, cx);
        }
    }

    fn lead_changed(&mut self, lead: i64, cx: &mut Context<Self>) {
        let Some(view) = self.sessions.get(&lead) else {
            return;
        };
        let view = view.read(cx);
        let busy = view.busy || view.external_submission_pending();
        let (success, _) = view.external_outcome(cx);
        let Some(live) = self.orchestrations.get_mut(&lead) else {
            return;
        };
        let finished = live.lead_busy && !busy;
        live.lead_busy = busy;
        if finished && !success && live.run.status == RunStatus::Active {
            let sessions = live.run.pause(
                "The lead was interrupted. Its agents were stopped; review and resume the run.",
            );
            for session in sessions {
                if let Some(view) = self.sessions.get(&session) {
                    view.update(cx, |v, cx| v.stop_if_busy(cx));
                }
            }
        }
    }

    fn worker_changed(&mut self, lead: i64, worker: i64, cx: &mut Context<Self>) {
        let Some(view) = self.sessions.get(&worker) else {
            return;
        };
        let view = view.read(cx);
        let busy = view.busy || view.external_submission_pending();
        let outcome = (!busy).then(|| view.external_outcome(cx));
        let Some(live) = self.orchestrations.get_mut(&lead) else {
            return;
        };
        let Some(task) = live
            .run
            .worker(worker)
            .filter(|t| t.status == Status::Running)
        else {
            return;
        };
        let id = task.id.clone();
        let Some((dispatch, seen)) = live.turns.get_mut(&id) else {
            return;
        };
        if busy {
            *seen = true;
            return;
        }
        if !*seen {
            return;
        }
        let dispatch = *dispatch;
        live.turns.remove(&id);
        let (success, report) = outcome.unwrap_or_default();
        let error = (!success).then(|| report.lines().next().unwrap_or_default().to_owned());
        live.run.settle(&id, dispatch, success, &report, error);
    }

    /// Start what can start, then publish the run's state.
    fn pump_orchestration(&mut self, lead: i64, cx: &mut Context<Self>) {
        let started = self
            .orchestrations
            .get_mut(&lead)
            .map(|live| live.run.schedule())
            .unwrap_or_default();
        for task in started {
            self.dispatch_worker(lead, task, cx);
        }
        self.continue_lead(lead, cx);
        self.publish_orchestration(lead, cx);
    }

    /// Worker settings: the lead's policy, with the task's harness and model.
    fn worker_settings(&self, lead: i64, task: &control::Task, cx: &App) -> Option<Settings> {
        let view = self.sessions.get(&lead)?.read(cx);
        let mut settings = view.settings.clone();
        settings.plan = false;
        settings.orchestrator = false;
        settings.operator = false;
        settings.operator_enabled = false;
        settings.handoff = None;
        settings.turns.clear();
        settings.environment.clear();
        settings.attachments.clear();
        settings.new_worktree = false;
        settings.worktree_removed = false;
        settings.internal_message = false;
        if task.provider != view.summary.provider {
            settings.account = crate::accounts::get(cx)
                .map(|center| {
                    center
                        .read(cx)
                        .selected(Kind::from_id(&task.provider), &view.summary.cwd)
                })
                .unwrap_or_default();
            settings.model_settings.clear();
            settings.fast = false;
            settings.thinking = None;
            settings.service_tier = None;
        }
        settings.model = task.model.clone();
        if task.effort.is_some() || task.provider != view.summary.provider {
            settings.effort = task.effort.clone();
        }
        Some(settings)
    }

    fn dispatch_worker(&mut self, lead: i64, id: String, cx: &mut Context<Self>) {
        let Some(live) = self.orchestrations.get(&lead) else {
            return;
        };
        let Some(task) = live.run.task(&id).cloned() else {
            return;
        };
        let (checkout, isolated, project) = (
            live.run.checkout.clone(),
            live.run.isolated,
            live.run.project.clone(),
        );
        // A worker keeps its session across retries, loaded again if needed.
        let existing = task
            .session
            .filter(|s| self.ensure_session(*s, cx).is_some());
        let Some(mut settings) = (match existing {
            Some(session) => Some(self.sessions[&session].read(cx).settings.clone()),
            None => self.worker_settings(lead, &task, cx),
        }) else {
            return;
        };
        let store = self.store.clone();
        let prompt = control::worker_prompt(
            task.recovery.as_deref().unwrap_or(&task.prompt),
            &task.files,
            isolated,
        );
        let name = format!("{lead}-{}-{}", task.id, monocode_core::time::now_ms());
        let dispatch = task.dispatch;
        let work = cx.background_executor().spawn(async move {
            let workspace = if isolated {
                Some(match task.workspace.clone() {
                    Some(worker) if std::path::Path::new(&worker.checkout).is_dir() => worker,
                    _ => isolation::create(std::path::Path::new(&checkout), &name)?,
                })
            } else {
                None
            };
            settings.worktree_cwd = Some(
                workspace
                    .as_ref()
                    .map_or(checkout.clone(), |w| w.checkout.clone()),
            );
            settings.worktree_removed = false;
            let created = match existing {
                Some(_) => Ok(None),
                None => (|| {
                    let store = locked(&store)?;
                    let mut summary = store.create(stored_provider(&task.provider), &project)?;
                    summary.provider = task.provider.clone();
                    summary.title = format!("Agent · {}", task.title);
                    store.title(summary.id, &summary.title)?;
                    store.save_agent_settings(summary.id, &settings)?;
                    Ok::<_, String>(Some(summary))
                })(),
            };
            let created = match created {
                Ok(created) => created,
                Err(error) => {
                    // A worktree made for this attempt must not be orphaned.
                    if let Some(worker) = &workspace
                        && task.workspace.as_ref() != Some(worker)
                    {
                        let _ = isolation::remove(std::path::Path::new(&checkout), worker);
                    }
                    return Err(error);
                }
            };
            Ok::<_, String>((created, settings, workspace))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((created, settings, workspace)) => {
                        let session = match created {
                            Some(summary) => {
                                let session = summary.id;
                                this.session_worktrees
                                    .insert(session, settings.worktree_cwd.clone());
                                this.add(summary, cx);
                                session
                            }
                            None => existing.unwrap_or_default(),
                        };
                        let prepared = this.orchestrations.get_mut(&lead).is_some_and(|live| {
                            live.run.prepared(&id, dispatch, session, workspace)
                        });
                        if prepared && let Some(view) = this.sessions.get(&session) {
                            this.session_worktrees
                                .insert(session, settings.worktree_cwd.clone());
                            view.update(cx, |v, cx| {
                                v.set_managed_by(Some(lead), cx);
                                v.install_transfer(settings, Some(prompt), cx);
                            });
                            let busy = view.read(cx).busy;
                            if let Some(live) = this.orchestrations.get_mut(&lead) {
                                live.turns.insert(id.clone(), (dispatch, busy));
                            }
                        }
                    }
                    Err(error) => {
                        if let Some(live) = this.orchestrations.get_mut(&lead) {
                            live.run.settle(&id, dispatch, false, "", Some(error));
                        }
                    }
                }
                this.invalidate_chrome(cx);
                this.pump_orchestration(lead, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Once the lead is idle, hand it worker results and requests it has not
    /// seen. Never interrupts the user's own turn or draft.
    fn continue_lead(&mut self, lead: i64, cx: &mut Context<Self>) {
        let ready = self
            .sessions
            .get(&lead)
            .is_some_and(|v| v.read(cx).ready_for_external(cx));
        let needs = self.needs(lead, cx);
        let Some(live) = self.orchestrations.get_mut(&lead) else {
            return;
        };
        if !ready || live.lead_busy || live.run.status != RunStatus::Active {
            return;
        }
        match live.run.continuation(&|t| needs.get(&t.id).cloned()) {
            Ok(Some(prompt)) => {
                live.lead_busy = true;
                let prompt = format!("{prompt}\n\n{}", control::lead_instructions(&cli()));
                if let Some(view) = self.sessions.get(&lead) {
                    view.update(cx, |v, cx| v.send_coordinator_report(prompt, cx));
                }
            }
            Ok(None) => {}
            Err(reason) => {
                for session in live.run.pause(&reason) {
                    if let Some(view) = self.sessions.get(&session) {
                        view.update(cx, |v, cx| v.stop_if_busy(cx));
                    }
                }
            }
        }
    }

    /// Save a changed run, wake its waiters and update the lead's card, the
    /// sidebar and its workers. A run that ended leaves memory.
    fn publish_orchestration(&mut self, lead: i64, cx: &mut Context<Self>) {
        let needs = self.needs(lead, cx);
        let Some(live) = self.orchestrations.get_mut(&lead) else {
            return;
        };
        let snapshot = serde_json::to_string(&live.run).unwrap_or_default();
        let blocked = live.run.blocked_keys(&|t| needs.get(&t.id).cloned());
        let changed = snapshot != live.saved || blocked != live.blocked;
        if snapshot != live.saved {
            live.saved = snapshot.clone();
            self.orchestration_control
                .save(lead, snapshot, &self.store, cx);
        }
        live.blocked = blocked;
        if changed {
            let view = live.run.view(&|t| needs.get(&t.id).cloned());
            for (_, call) in live.waiters.drain(..) {
                let _ = call.reply.send(Ok(view.clone()));
            }
        }
        let summary = live.run.summary();
        let progress = live.run.progress();
        let paused = live.run.status == RunStatus::Paused;
        let ended = matches!(live.run.status, RunStatus::Finished | RunStatus::Stopped);
        let workers: Vec<i64> = live.run.tasks.iter().filter_map(|t| t.session).collect();
        if ended {
            self.orchestrations.remove(&lead);
        }
        if self.orchestration_summaries.get(&lead) != Some(&summary) {
            let store = self.store.clone();
            let saved = summary.clone();
            cx.background_executor()
                .spawn(async move {
                    let _ = locked(&store).and_then(|s| s.save_orchestration_summary(lead, &saved));
                })
                .detach();
            self.orchestration_summaries.insert(lead, summary);
            self.invalidate_chrome(cx);
        }
        if let Some(view) = self.sessions.get(&lead) {
            view.update(cx, |v, cx| {
                v.set_orchestration_paused(paused, cx);
                v.set_orchestration_progress((!ended).then_some(progress), cx);
            });
        }
        for worker in workers {
            if let Some(view) = self.sessions.get(&worker) {
                view.update(cx, |v, cx| v.set_managed_by((!ended).then_some(lead), cx));
            }
        }
    }

    /// One `control.*` call from a lead's provider process.
    pub(super) fn handle_control_call(&mut self, call: Call, cx: &mut Context<Self>) {
        let action = call
            .action
            .strip_prefix("control.")
            .unwrap_or_default()
            .to_owned();
        let lead = call.session;
        if !self.orchestrations.contains_key(&lead) {
            let _ = call.reply.send(Err(
                "No orchestration run is open for this chat. The user starts one from the plan card."
                    .into(),
            ));
            return;
        }
        if call.request_id.is_empty()
            || call.request_id.len() > 128
            || !call
                .request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            let _ = call.reply.send(Err(
                "request ID must use letters, digits, underscores or hyphens".into(),
            ));
            return;
        }
        let mutating = !matches!(action.as_str(), "list" | "get" | "wait");
        let fingerprint = app::fingerprint(&action, &call.input);
        let needs = self.needs(lead, cx);
        let needs = |t: &control::Task| needs.get(&t.id).cloned();
        let live = self.orchestrations.get_mut(&lead).unwrap();
        if mutating && let Some(result) = live.run.receipt(&call.request_id, &fingerprint) {
            let _ = call.reply.send(result);
            return;
        }
        let (value, effect) = match live.run.execute(&action, &call.input, &needs) {
            Ok(done) => done,
            Err(error) => {
                let _ = call.reply.send(Err(error));
                return;
            }
        };
        if action == "wait" {
            if !live.run.waitable(&needs) {
                let _ = call.reply.send(Ok(value));
                return;
            }
            let seconds = call
                .input
                .get("timeoutSeconds")
                .and_then(Value::as_f64)
                .unwrap_or(20.);
            self.orchestration_control.next_waiter += 1;
            let waiter = self.orchestration_control.next_waiter;
            live.waiters.push((waiter, call));
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs_f64(seconds))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    let needs = this.needs(lead, cx);
                    if let Some(live) = this.orchestrations.get_mut(&lead)
                        && let Some(ix) = live.waiters.iter().position(|(id, _)| *id == waiter)
                    {
                        let (_, call) = live.waiters.remove(ix);
                        let _ = call
                            .reply
                            .send(Ok(live.run.view(&|t| needs.get(&t.id).cloned())));
                    }
                });
            })
            .detach();
            return;
        }
        let settled =
            move |this: &mut Self, value: Result<Value, String>, cx: &mut Context<Self>| {
                if let (true, Ok(result)) = (mutating, &value)
                    && let Some(live) = this.orchestrations.get_mut(&lead)
                {
                    live.run.record(&call.request_id, &fingerprint, result);
                }
                let _ = call.reply.send(value);
                this.pump_orchestration(lead, cx);
            };
        let session = |this: &Self, id: i64| this.sessions.get(&id).cloned();
        match effect {
            Effect::None => settled(self, Ok(value), cx),
            Effect::Steer { session: id, text } => {
                let steered = session(self, id)
                    .is_some_and(|view| view.update(cx, |v, cx| v.steer_external(text, cx)));
                settled(
                    self,
                    if steered {
                        Ok(value)
                    } else {
                        Err("This agent cannot take guidance right now. Retry, or message it once it stops.".into())
                    },
                    cx,
                );
            }
            Effect::Stop { session: id } => {
                if let Some(view) = session(self, id) {
                    view.update(cx, |v, cx| v.stop_if_busy(cx));
                }
                settled(self, Ok(value), cx);
            }
            Effect::Respond {
                session: id,
                request_id,
                allow,
            } => {
                let decision = if allow {
                    monocode_core::agent::Decision::Allow
                } else {
                    monocode_core::agent::Decision::Deny
                };
                let done = session(self, id).is_some_and(|view| {
                    view.update(cx, |v, cx| {
                        v.decide_notification_request(&request_id, decision, cx)
                    })
                });
                settled(
                    self,
                    if done {
                        Ok(value)
                    } else {
                        Err("The agent is no longer waiting for this approval.".into())
                    },
                    cx,
                );
            }
            Effect::Answer {
                session: id,
                request_id,
                answers,
            } => {
                let done = session(self, id).is_some_and(|view| {
                    view.update(cx, |v, cx| v.answer_external(&request_id, answers, cx))
                });
                settled(
                    self,
                    if done {
                        Ok(value)
                    } else {
                        Err("The agent is no longer waiting for this answer.".into())
                    },
                    cx,
                );
            }
            Effect::Integrate { task } => {
                let live = &self.orchestrations[&lead];
                let target = live.run.task(&task).cloned().unwrap_or_default();
                let checkout = live.run.checkout.clone();
                let Some(worker) = target.workspace.clone() else {
                    return settled(
                        self,
                        Err("This worker's worktree is unavailable.".into()),
                        cx,
                    );
                };
                let work = cx.background_executor().spawn(async move {
                    let lead = std::path::Path::new(&checkout);
                    let files = isolation::integrate(&worker, lead, &target.files)?;
                    let removed = isolation::remove(lead, &worker).is_ok();
                    Ok::<_, String>((files, removed))
                });
                cx.spawn(async move |this, cx| {
                    let result = work.await;
                    let _ = this.update(cx, |this, cx| {
                        let value = result.map(|(files, removed)| {
                            let Some(live) = this.orchestrations.get_mut(&lead) else {
                                return json!({"accepted": false});
                            };
                            let mut value = live.run.accept(&task, files);
                            let worker = live.run.task_mut(&task).and_then(|t| {
                                if removed {
                                    t.workspace.take();
                                }
                                t.session
                            });
                            value["cleaned"] = removed.into();
                            if removed
                                && let Some(view) = worker.and_then(|w| this.sessions.get(&w))
                            {
                                view.update(cx, |v, _| v.settings.worktree_removed = true);
                            }
                            value
                        });
                        settled(this, value, cx);
                    });
                })
                .detach();
            }
            Effect::Cleanup {
                accepted,
                unreviewed,
            } => {
                let checkout = self.orchestrations[&lead].run.checkout.clone();
                let work = cx.background_executor().spawn(async move {
                    let lead = std::path::Path::new(&checkout);
                    let mut kept = Vec::new();
                    for worker in accepted {
                        if isolation::remove(lead, &worker).is_err() {
                            kept.push(worker.checkout);
                        }
                    }
                    for worker in unreviewed {
                        let unchanged =
                            isolation::changed(&worker).is_ok_and(|files| files.is_empty());
                        if !unchanged || isolation::remove(lead, &worker).is_err() {
                            kept.push(worker.checkout);
                        }
                    }
                    kept
                });
                cx.spawn(async move |this, cx| {
                    let kept = work.await;
                    let _ = this.update(cx, |this, cx| {
                        let mut value = value;
                        if !kept.is_empty() {
                            value["cleanupPending"] = json!(kept);
                        }
                        settled(this, Ok(value), cx);
                    });
                })
                .detach();
            }
        }
    }
}

impl Control {
    fn save(&mut self, lead: i64, snapshot: String, store: &Database, cx: &mut Context<Shell>) {
        let saver = self.saver.get_or_insert_with(|| {
            let (tx, rx) = async_channel::unbounded::<(i64, String)>();
            let store = store.clone();
            cx.background_executor()
                .spawn(async move {
                    while let Ok((lead, snapshot)) = rx.recv().await {
                        let _ = locked(&store)
                            .and_then(|s| s.save_setting(&control::setting_key(lead), &snapshot));
                    }
                })
                .detach();
            tx
        });
        let _ = saver.try_send((lead, snapshot));
    }
}

#[cfg(test)]
mod tests;
