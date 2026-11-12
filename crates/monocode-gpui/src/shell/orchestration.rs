//! Lead-supervised orchestration (orchestration.ts). The user starts the
//! lead's reviewed plan; the lead then drives its workers through
//! `monocode-gpui control`, whose calls arrive on the app bridge. This module
//! executes what `monocode_core::orchestration::control` decides: it creates
//! the worker sessions and worktrees, routes their approvals and questions to
//! the lead, applies reviewed changes and saves the run so an interrupted one
//! can resume. Each run keeps a board (`orchestration::board`) the lead and
//! its workers post on; the Swarm tab draws the board of the chat in view.
//! `/swarm` starts a run with no plan card: the lead delegates by itself.
use super::*;
use crate::conversation::composer_features::{
    OpenOrchestrationAgents, ResumeOrchestration, StartOrchestration, StartSwarm, StopOrchestration,
};
use monocode_core::{
    agent::Settings,
    app_access as app,
    mono::bridge::Call,
    orchestration::{
        self,
        board::{self, Board},
        control::{self, Choice, Effect, NeedsInput, Run, RunStatus, Status},
        isolation,
    },
};
use serde_json::{Value, json};
use std::sync::Arc;

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
    board: Board,
    /// A restored run's board comes from disk; until then it is not saved.
    board_loaded: bool,
    /// The board changed in a way `Board::sync` does not report.
    board_dirty: bool,
    /// Results of recent posts by request ID, so a retried post lands once.
    posted: HashMap<String, Value>,
    /// Code monos this run made, up to `control::MAX_NEW_MONOS`.
    new_monos: usize,
}
impl Live {
    fn new(run: Run) -> Self {
        Self {
            board: Board::new(&run.request),
            run,
            waiters: Vec::new(),
            turns: HashMap::new(),
            lead_busy: false,
            blocked: Vec::new(),
            saved: String::new(),
            board_loaded: true,
            board_dirty: true,
            posted: HashMap::new(),
            new_monos: 0,
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
    /// Saves (setting key, value) in order.
    saver: Option<async_channel::Sender<(String, String)>>,
    /// Open the Swarm tab, and the panel or sidebar it is in, on the next
    /// render, which has the window.
    show_swarm: bool,
    next_waiter: u64,
    /// A lead whose agents open beside it on the next render.
    open_agents: Option<i64>,
    /// Boards by lead, for the Swarm tab: None when the lead has none.
    boards: HashMap<i64, Option<Arc<Board>>>,
    /// Leads whose saved board is being read.
    loading: HashSet<i64>,
}

/// Runs saved as active were interrupted when the app closed: their workers
/// stopped with it. They come back paused, ready to resume.
pub(super) fn restore(runs: HashMap<i64, Run>) -> HashMap<i64, Live> {
    runs.into_iter()
        .map(|(lead, mut run)| {
            run.pause("monoBOT closed while this run was working. Its agents were stopped.");
            let mut live = Live::new(run);
            live.board_loaded = false;
            (lead, live)
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
        self.observations
            .push(cx.subscribe(view, move |this, _, event: &StartSwarm, cx| {
                this.start_swarm(
                    id,
                    event.request.clone(),
                    event.roles.clone(),
                    event.message.clone(),
                    cx,
                )
            }));
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
            let swarm = live.run.swarm;
            view.update(cx, |v, cx| {
                v.set_orchestration_swarm(swarm, cx);
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
        let profiles = self.run_profiles(&project, cx);
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
            let choices = run_choices(profiles, assigned);
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
                this.sweep_worktrees(&run, cx);
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
        if std::mem::take(&mut self.orchestration_control.show_swarm) {
            self.show_swarm(window, cx);
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
        // A swarm's board shows even before its coordinator delegates.
        self.show_swarm(window, cx);
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
        self.deliver_chief_reports(id, cx);
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
        // A swarm whose lead answered without delegating anything is over.
        if finished
            && success
            && live.run.swarm
            && live.run.tasks.is_empty()
            && live.run.status == RunStatus::Active
        {
            live.run.status = RunStatus::Finished;
        }
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
        if !started.is_empty() {
            let needs = self.needs(lead, cx);
            if let Some(live) = self.orchestrations.get_mut(&lead) {
                live.board.sync(
                    &live.run,
                    &|t| needs.contains_key(&t.id),
                    monocode_core::time::now_ms(),
                );
                live.board_dirty = true;
            }
        }
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
        let member = self.bind_mono(lead, &id, cx);
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
        // What was said to it on the board, and how to use it.
        let prompt = match self.orchestrations.get_mut(&lead).and_then(|live| {
            let handle = live
                .board
                .members
                .iter()
                .find(|m| m.task.as_deref() == Some(id.as_str()))?
                .handle
                .clone();
            let members: Vec<String> = live
                .board
                .members
                .iter()
                .map(|m| m.handle.clone())
                .collect();
            let inbox = live.board.inbox(&handle);
            live.board_dirty |= inbox.is_some();
            let rules = control::board_instructions(&cli(), &handle, &members);
            Some(match inbox {
                Some(inbox) => format!("{inbox}\n\n{rules}"),
                None => rules,
            })
        }) {
            Some(note) => format!("{prompt}\n\n{note}"),
            None => prompt,
        };
        let name = format!("{lead}-{}-{}", task.id, monocode_core::time::now_ms());
        let dispatch = task.dispatch;
        // A mono's worker is titled with its name and starts as that mono.
        let title = match &member {
            Some((_, mono)) => format!("{} · {}", mono.name, task.title),
            None => format!("Agent · {}", task.title),
        };
        let first_turn = existing.is_none();
        let work = cx.background_executor().spawn(async move {
            let identity = member
                .filter(|_| first_turn)
                .map(|(root, mono)| monocode_core::mono::crew::identity(&root, &mono));
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
                    summary.title = title;
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
            Ok::<_, String>((created, settings, workspace, identity))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((created, settings, workspace, identity)) => {
                        let prompt = match identity {
                            Some(identity) => format!(
                                "{identity}

{prompt}"
                            ),
                            None => prompt,
                        };
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

    /// In a Chief run, the persistent mono that does task `id`: the one
    /// Chief named, else a free one that fits, else a new one while the run
    /// may still make one. Returns where its files are and the mono.
    fn bind_mono(
        &mut self,
        lead: i64,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Option<(std::path::PathBuf, monocode_core::mono::Mono)> {
        let live = self.orchestrations.get(&lead)?;
        live.run.requester?;
        let task = live.run.task(id)?.clone();
        let may_create = live.new_monos < control::MAX_NEW_MONOS;
        // Busy in any run: a mono does one task at a time.
        let busy: HashSet<String> = self
            .orchestrations
            .values()
            .flat_map(|live| live.run.tasks.iter())
            .filter(|t| {
                t.id != task.id
                    && matches!(t.status, Status::Queued | Status::Running)
                    && t.mono.is_some()
            })
            .filter_map(|t| t.mono.clone())
            .collect();
        let monos = self.monos.clone()?;
        let (mono, made) = monos.update(cx, |m, cx| {
            m.crew_for_task(
                task.mono.as_deref(),
                task.handle.as_deref(),
                &task.title,
                &|mono| busy.contains(mono),
                may_create,
                cx,
            )
        })?;
        let member = monos.read(cx).crew_member(&mono)?;
        let live = self.orchestrations.get_mut(&lead)?;
        if made {
            live.new_monos += 1;
            live.run.crew.push(control::CrewMember {
                id: mono.clone(),
                name: member.1.name.clone(),
                specialty: member.1.specialty.clone().unwrap_or_default(),
            });
        }
        if let Some(task) = live.run.task_mut(id) {
            task.mono = Some(mono);
        }
        Some(member)
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
                let posts = live.board.inbox(board::COORDINATOR);
                live.board_dirty |= posts.is_some();
                let prompt = match posts {
                    Some(posts) => format!(
                        "{prompt}\n\n{posts}\n\n{}",
                        control::lead_instructions(&cli())
                    ),
                    None => format!("{prompt}\n\n{}", control::lead_instructions(&cli())),
                };
                if let Some(view) = self.sessions.get(&lead) {
                    view.update(cx, |v, cx| v.send_coordinator_report(prompt, cx));
                }
            }
            // Workers addressed the coordinator on the board.
            Ok(None)
                if live.board.pending(board::COORDINATOR)
                    && live.run.continuations < control::MAX_CONTINUATIONS =>
            {
                live.run.continuations += 1;
                live.lead_busy = true;
                live.board_dirty = true;
                let posts = live.board.inbox(board::COORDINATOR).unwrap_or_default();
                let prompt = format!(
                    "{posts}\n\nAnswer the board when it helps the workers, steer or message them, and keep supervising the run.\n\n{}",
                    control::lead_instructions(&cli())
                );
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
        let board_changed = live.board.sync(
            &live.run,
            &|t| needs.contains_key(&t.id),
            monocode_core::time::now_ms(),
        ) || std::mem::take(&mut live.board_dirty);
        let board_loaded = live.board_loaded;
        let mut swarm_changed = false;
        if board_loaded && board_changed {
            let snapshot = serde_json::to_string(&live.board).unwrap_or_default();
            self.orchestration_control
                .boards
                .insert(lead, Some(Arc::new(live.board.clone())));
            self.orchestration_control
                .save(board::setting_key(lead), snapshot, &self.store, cx);
            swarm_changed = true;
        }
        let snapshot = serde_json::to_string(&live.run).unwrap_or_default();
        let blocked = live.run.blocked_keys(&|t| needs.get(&t.id).cloned());
        let changed = snapshot != live.saved || blocked != live.blocked;
        if snapshot != live.saved {
            live.saved = snapshot.clone();
            self.orchestration_control
                .save(control::setting_key(lead), snapshot, &self.store, cx);
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
        let swarm = live.run.swarm;
        let ended = matches!(live.run.status, RunStatus::Finished | RunStatus::Stopped);
        let workers: Vec<i64> = live.run.tasks.iter().filter_map(|t| t.session).collect();
        let report = ended
            .then(|| live.run.requester.zip(live.run.chief_report()))
            .flatten();
        if ended {
            self.orchestrations.remove(&lead);
        }
        if let Some((requester, report)) = report {
            self.queue_chief_report(requester, report, cx);
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
                v.set_orchestration_swarm(swarm, cx);
                v.set_orchestration_paused(paused, cx);
                v.set_orchestration_progress((!ended).then_some(progress), cx);
            });
        }
        for worker in workers {
            if let Some(view) = self.sessions.get(&worker) {
                view.update(cx, |v, cx| v.set_managed_by((!ended).then_some(lead), cx));
            }
        }
        if !board_loaded {
            self.load_board(lead, cx);
        }
        if swarm_changed {
            self.sync_swarm(cx);
        }
    }

    /// One `control.*` call from a lead's provider process.
    pub(super) fn handle_control_call(&mut self, call: Call, cx: &mut Context<Self>) {
        let action = call
            .action
            .strip_prefix("control.")
            .unwrap_or_default()
            .to_owned();
        // A lead's own `chief` call falls through to its run, which refuses it.
        if action == "swarm"
            || (action == "chief" && !self.orchestrations.contains_key(&call.session))
        {
            if !valid_request_id(&call.request_id) {
                let _ = call.reply.send(Err(
                    "request ID must use letters, digits, underscores or hyphens".into(),
                ));
                return;
            }
            if action == "chief" {
                self.agent_chief(call, cx);
            } else {
                self.agent_swarm(call, cx);
            }
            return;
        }
        // The lead, or one of its workers: those only reach the board.
        let (lead, worker) = if self.orchestrations.contains_key(&call.session) {
            (call.session, None)
        } else if let Some((lead, live)) = self
            .orchestrations
            .iter()
            .find(|(_, live)| live.run.worker(call.session).is_some())
        {
            let handle = live
                .board
                .handle_of(call.session)
                .unwrap_or_default()
                .to_owned();
            (*lead, Some(handle))
        } else {
            let _ = call.reply.send(Err(
                "No orchestration run is open for this chat. The user starts one with /swarm or from the plan card; with Settings > Chat allowing it, control swarm starts one."
                    .into(),
            ));
            return;
        };
        if worker.is_some() && !control::WORKER_ACTIONS.contains(&action.as_str()) {
            let _ = call.reply.send(Err(
                "Workers can only use post and board. Ask @coordinator on the board for anything else."
                    .into(),
            ));
            return;
        }
        if !valid_request_id(&call.request_id) {
            let _ = call.reply.send(Err(
                "request ID must use letters, digits, underscores or hyphens".into(),
            ));
            return;
        }
        if matches!(action.as_str(), "post" | "board") {
            let from = worker.unwrap_or_else(|| board::COORDINATOR.to_owned());
            self.board_call(lead, from, &action, call, cx);
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
        let (mut value, effect) = match live.run.execute(&action, &call.input, &needs) {
            Ok(done) => done,
            Err(error) => {
                let _ = call.reply.send(Err(error));
                return;
            }
        };
        // A delegated task joins the board at once, so the lead learns its handle.
        if action == "delegate" {
            live.board.sync(
                &live.run,
                &|t| needs(t).is_some(),
                monocode_core::time::now_ms(),
            );
            live.board_dirty = true;
            let handle = value["taskId"].as_str().and_then(|id| {
                live.board
                    .members
                    .iter()
                    .find(|m| m.task.as_deref() == Some(id))
                    .map(|m| m.handle.clone())
            });
            if let Some(handle) = handle {
                value["handle"] = handle.into();
            }
        }
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
    fn save(&mut self, key: String, snapshot: String, store: &Database, cx: &mut Context<Shell>) {
        let saver = self.saver.get_or_insert_with(|| {
            let (tx, rx) = async_channel::unbounded::<(String, String)>();
            let store = store.clone();
            cx.background_executor()
                .spawn(async move {
                    while let Ok((key, snapshot)) = rx.recv().await {
                        let _ = locked(&store).and_then(|s| s.save_setting(&key, &snapshot));
                    }
                })
                .detach();
            tx
        });
        let _ = saver.try_send((key, snapshot));
    }
}

/// The harnesses a lead may assign: the installed ones with their models,
/// plus whatever was already assigned. Reads catalogs: run it off the UI.
/// A control call's request ID: 1–128 letters, digits, `_` or `-`.
fn valid_request_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn run_choices(
    profiles: Vec<(Kind, monocode_core::accounts::Profile)>,
    assigned: Vec<(String, Option<String>)>,
) -> Vec<Choice> {
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
    choices
}

impl Shell {
    /// The account profile of each harness for `project`.
    fn run_profiles(
        &self,
        project: &str,
        cx: &App,
    ) -> Vec<(Kind, monocode_core::accounts::Profile)> {
        if cx.is_test() {
            return Vec::new();
        }
        Kind::ALL
            .into_iter()
            .map(|kind| {
                let profile = crate::accounts::get(cx)
                    .map(|center| center.read(cx).selected(kind, project))
                    .unwrap_or_default();
                (kind, profile)
            })
            .collect()
    }

    /// `/swarm`: open a run with no tasks for this chat, then send its
    /// message; the lead plans and delegates from its first turn.
    fn start_swarm(
        &mut self,
        lead: i64,
        request: String,
        roles: Vec<control::Role>,
        message: monocode_core::follow_up::Message,
        cx: &mut Context<Self>,
    ) {
        self.open_swarm(lead, request, roles, cx, move |this, opened, cx| {
            let Some(view) = this.sessions.get(&lead).cloned() else {
                return;
            };
            match opened {
                Ok(()) => view.update(cx, |v, cx| v.launch_swarm(message, cx)),
                Err(error) => view.update(cx, |v, cx| v.swarm_failed(message, error, cx)),
            }
        });
    }

    /// `control swarm` from a coding chat's own turn: the user lets agents
    /// start swarms, so the chat becomes the coordinator and is told the
    /// rules in the answer, to delegate in the same turn.
    fn agent_swarm(&mut self, call: Call, cx: &mut Context<Self>) {
        let session = call.session;
        let cli = std::env::current_exe()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "monocode-gpui".into());
        // A retried call finds the swarm it opened.
        if let Some(live) = self.orchestrations.get(&session) {
            let _ = call.reply.send(if live.run.swarm {
                Ok(control::swarm_started(
                    &live.run.request,
                    &cli,
                    &live.run.roles,
                ))
            } else {
                Err("This chat already coordinates a run; delegate within it.".into())
            });
            return;
        }
        if self.swarm_lead(session).is_some() {
            let _ = call.reply.send(Err(
                "Workers cannot start a swarm. Ask @coordinator on the board instead.".into(),
            ));
            return;
        }
        let allowed = cx
            .try_global::<crate::settings::Preferences>()
            .is_some_and(|p| p.swarm_offer);
        if !allowed {
            let _ = call.reply.send(Err(
                "The user has not let agents start swarms (Settings > Chat). Work on the task here, or suggest /swarm to the user."
                    .into(),
            ));
            return;
        }
        let eligible = self.sessions.get(&session).is_some_and(|view| {
            let view = view.read(cx);
            view.settings.mono.is_none() && !view.is_remote() && !view.settings.worktree_removed
        });
        if !eligible {
            let _ = call
                .reply
                .send(Err("A swarm is not available in this chat.".into()));
            return;
        }
        let title = call
            .input
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| t.chars().take(160).collect::<String>());
        let Some(title) = title else {
            let _ = call
                .reply
                .send(Err("Give the swarm a short \"title\" for the task.".into()));
            return;
        };
        let reply = call.reply;
        self.open_swarm(
            session,
            title.clone(),
            Vec::new(),
            cx,
            move |this, opened, cx| {
                let _ = reply.send(opened.map(|()| control::swarm_started(&title, &cli, &[])));
                if let Some(view) = this.sessions.get(&session) {
                    view.update(cx, |v, cx| v.swarm_opened_by_agent(cx));
                }
            },
        );
    }

    /// `control chief` from a coding chat's own turn: Chief, a mono in its
    /// own chat, leads a run for the brief, giving tasks to the user's code
    /// monos; the chat that asked gets Chief's summary once it is over.
    fn agent_chief(&mut self, call: Call, cx: &mut Context<Self>) {
        let requester = call.session;
        let text = |key: &str, max: usize| {
            call.input
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(|t| t.chars().take(max).collect::<String>())
        };
        let (Some(title), Some(brief)) = (text("title", 160), text("brief", 30_000)) else {
            let _ = call.reply.send(Err(
                "Give Chief a short \"title\" and a self-contained \"brief\".".into(),
            ));
            return;
        };
        // A retried call finds the run it opened, even paused.
        if let Some(lead) = self
            .orchestrations
            .iter()
            .find(|(_, live)| live.run.requester == Some(requester))
            .map(|(lead, _)| *lead)
        {
            let _ = call.reply.send(Ok(control::chief_started(&title, lead)));
            return;
        }
        let refusal = if self.orchestrations.contains_key(&requester) {
            Some("This chat already coordinates a run; delegate within it.")
        } else if self
            .orchestrations
            .values()
            .any(|live| live.run.worker(requester).is_some())
        {
            Some("A mono cannot hand work to Chief. Ask @coordinator on the board instead.")
        } else if !cx
            .try_global::<crate::settings::Preferences>()
            .is_some_and(|p| p.chief_offer)
        {
            Some(
                "The user has turned off handing work to monos (Settings > Chat). Work on the task here.",
            )
        } else if !self.sessions.get(&requester).is_some_and(|view| {
            let view = view.read(cx);
            view.settings.mono.is_none() && !view.is_remote() && !view.settings.worktree_removed
        }) {
            Some("Chief is not available in this chat.")
        } else if self.app_bridge.is_none() && !cx.is_test() {
            Some("monoBOT app access is unavailable, so Chief cannot launch monos.")
        } else {
            None
        };
        if let Some(refusal) = refusal {
            let _ = call.reply.send(Err(refusal.into()));
            return;
        }
        let Some((_, crew)) = self
            .monos
            .clone()
            .and_then(|monos| monos.update(cx, |m, cx| m.chief_crew(cx)))
        else {
            let _ = call.reply.send(Err(
                "Your monos are still loading; try again in a moment.".into()
            ));
            return;
        };
        let view = self.sessions[&requester].read(cx);
        let project = view.summary.cwd.clone();
        let checkout = view
            .settings
            .worktree_cwd
            .clone()
            .unwrap_or_else(|| project.clone());
        if self.orchestrations.values().any(|live| {
            monocode_core::files::path_key(live.checkout())
                == monocode_core::files::path_key(&checkout)
        }) {
            let _ = call
                .reply
                .send(Err("Another run is already using this checkout.".into()));
            return;
        }
        let assigned = vec![(view.summary.provider.clone(), view.settings.model.clone())];
        let fallback = view.summary.provider.clone();
        let profiles = self.run_profiles(&project, cx);
        let claude = profiles
            .iter()
            .find(|(kind, _)| *kind == Kind::Claude)
            .map(|(_, profile)| profile.clone())
            .unwrap_or_default();
        let path = checkout.clone();
        let test = cx.is_test();
        let reply = call.reply;
        let work = cx.background_executor().spawn(async move {
            let isolated = isolation::available(std::path::Path::new(&path));
            // Opus at medium when Claude is installed; otherwise the
            // requester's own harness and model.
            // Tests keep the requester's fixture harness, which starts no agent.
            let defaults = if !test && monocode_core::provider::resolve(Kind::Claude).is_some() {
                control::Defaults::chief(
                    &monocode_core::catalog::models_for(Kind::Claude, &claude).unwrap_or_default(),
                )
            } else {
                control::Defaults {
                    provider: fallback,
                    ..Default::default()
                }
            };
            let mut assigned = assigned;
            assigned.push((defaults.provider.clone(), defaults.model.clone()));
            (isolated, run_choices(profiles, assigned), defaults)
        });
        cx.spawn(async move |this, cx| {
            let (isolated, choices, defaults) = work.await;
            let _ = this.update(cx, |this, cx| {
                let template = control::Task {
                    provider: defaults.provider.clone(),
                    model: defaults.model.clone(),
                    effort: defaults.effort.clone(),
                    ..Default::default()
                };
                let Some(mut settings) = this.worker_settings(requester, &template, cx) else {
                    let _ = reply.send(Err("The chat that asked is gone.".into()));
                    return;
                };
                settings.worktree_cwd = Some(checkout.clone());
                let store = this.store.clone();
                let provider = defaults.provider.clone();
                let chief_title = format!("Chief · {title}");
                let saved = settings.clone();
                let project_dir = project.clone();
                let created = cx.background_executor().spawn(async move {
                    let store = locked(&store)?;
                    let mut summary = store.create(stored_provider(&provider), &project_dir)?;
                    summary.provider = provider;
                    summary.title = chief_title;
                    store.title(summary.id, &summary.title)?;
                    store.save_agent_settings(summary.id, &saved)?;
                    store.save_started_link(summary.id, requester)?;
                    Ok::<_, String>(summary)
                });
                cx.spawn(async move |this, cx| {
                    let created = created.await;
                    let _ = this.update(cx, |this, cx| {
                        let summary = match created {
                            Ok(summary) => summary,
                            Err(error) => {
                                let _ = reply.send(Err(error));
                                return;
                            }
                        };
                        if this
                            .orchestrations
                            .values()
                            .any(|live| live.run.requester == Some(requester))
                        {
                            let _ =
                                reply.send(Err("Chief is already working for this chat.".into()));
                            return;
                        }
                        let chief = summary.id;
                        this.session_worktrees
                            .insert(chief, settings.worktree_cwd.clone());
                        this.started_by.insert(chief, requester);
                        this.add(summary, cx);
                        let run = Run::chief(
                            chief,
                            requester,
                            &project,
                            &checkout,
                            isolated,
                            &brief,
                            choices,
                            Some(defaults),
                            crew,
                        );
                        this.sweep_worktrees(&run, cx);
                        let prompt = control::chief_prompt(&brief, &cli(), &run.crew);
                        let mut live = Live::new(run);
                        live.lead_busy = true;
                        this.orchestrations.insert(chief, live);
                        if let Some(view) = this.sessions.get(&chief) {
                            view.update(cx, |v, cx| v.install_transfer(settings, Some(prompt), cx));
                        }
                        let _ = reply.send(Ok(control::chief_started(&title, chief)));
                        this.invalidate_chrome(cx);
                        this.pump_orchestration(chief, cx);
                        this.reveal_swarm(cx);
                        cx.notify();
                    });
                })
                .detach();
            });
        })
        .detach();
    }

    /// Queue Chief's report for the chat that asked, saved until it lands.
    fn queue_chief_report(&mut self, requester: i64, report: String, cx: &mut Context<Self>) {
        let queue = self.chief_reports.entry(requester).or_default();
        queue.push(report);
        self.save_chief_reports(requester, cx);
        self.deliver_chief_reports(requester, cx);
    }

    fn save_chief_reports(&mut self, requester: i64, cx: &mut Context<Self>) {
        let reports = self
            .chief_reports
            .get(&requester)
            .cloned()
            .unwrap_or_default();
        let store = self.store.clone();
        cx.background_executor()
            .spawn(async move {
                let _ = locked(&store).and_then(|s| s.save_chief_reports(requester, &reports));
            })
            .detach();
    }

    /// Hand the chat that asked Chief's reports once it is idle, as one
    /// internal turn; never over the user's own turn or draft.
    pub(super) fn deliver_chief_reports(&mut self, requester: i64, cx: &mut Context<Self>) {
        if !self.chief_reports.contains_key(&requester) {
            return;
        }
        if self.ensure_session(requester, cx).is_none() {
            // The chat is gone: nobody is left to read them.
            self.chief_reports.remove(&requester);
            self.save_chief_reports(requester, cx);
            return;
        }
        let Some(view) = self.sessions.get(&requester).cloned() else {
            return;
        };
        if !view.read(cx).ready_for_external(cx) {
            return;
        }
        let Some(reports) = self.chief_reports.remove(&requester) else {
            return;
        };
        self.save_chief_reports(requester, cx);
        view.update(cx, |v, cx| {
            v.send_coordinator_report(reports.join("\n\n"), cx)
        });
    }

    /// Open a run with no tasks for chat `lead`, then call `done` with the
    /// outcome; on success the lead counts as busy, as it is mid-turn or
    /// about to start one.
    fn open_swarm(
        &mut self,
        lead: i64,
        request: String,
        roles: Vec<control::Role>,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<(), String>, &mut Context<Self>) + 'static,
    ) {
        if self.orchestrations.contains_key(&lead) {
            return done(
                self,
                Err("Resume or stop this chat's current run before starting another.".into()),
                cx,
            );
        }
        if self.app_bridge.is_none() && !cx.is_test() {
            return done(
                self,
                Err(
                    "monoBOT app access is unavailable, so the coordinator cannot launch agents."
                        .into(),
                ),
                cx,
            );
        }
        let Some(view) = self.sessions.get(&lead) else {
            return;
        };
        let conversation = view.read(cx);
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
            return done(
                self,
                Err("Another run is already using this checkout.".into()),
                cx,
            );
        }
        let provider = conversation.summary.provider.clone();
        let model = conversation.settings.model.clone();
        let profiles = self.run_profiles(&project, cx);
        let assigned = vec![(provider.clone(), model.clone())];
        let path = checkout.clone();
        let work = cx.background_executor().spawn(async move {
            let isolated = isolation::available(std::path::Path::new(&path));
            (isolated, run_choices(profiles, assigned))
        });
        cx.spawn(async move |this, cx| {
            let (isolated, choices) = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.orchestrations.contains_key(&lead) {
                    return done(this, Err("This chat already has a run.".into()), cx);
                }
                let mut run = Run::swarm(
                    lead,
                    &project,
                    &checkout,
                    isolated,
                    &request,
                    choices,
                    control::MAX_WORKERS,
                );
                run.roles = roles;
                if let Err(error) = run.check_roles() {
                    return done(this, Err(error), cx);
                }
                this.sweep_worktrees(&run, cx);
                let mut live = Live::new(run);
                live.lead_busy = true;
                this.orchestrations.insert(lead, live);
                done(this, Ok(()), cx);
                this.pump_orchestration(lead, cx);
                this.reveal_swarm(cx);
            });
        })
        .detach();
    }

    /// Clear worker worktrees and branches earlier runs left in this
    /// repository, keeping those of every open run.
    fn sweep_worktrees(&self, run: &Run, cx: &mut Context<Self>) {
        if !run.isolated {
            return;
        }
        let keep: Vec<String> = self
            .orchestrations
            .values()
            .flat_map(|live| live.run.tasks.iter())
            .filter_map(|t| t.workspace.as_ref().map(|w| w.checkout.clone()))
            .collect();
        let checkout = run.checkout.clone();
        cx.background_executor()
            .spawn(async move {
                let _ = isolation::sweep(std::path::Path::new(&checkout), &keep);
            })
            .detach();
    }

    /// `control post` and `control board`, from the lead or a worker.
    fn board_call(
        &mut self,
        lead: i64,
        from: String,
        action: &str,
        call: Call,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = control::check_fields(action, &call.input) {
            let _ = call.reply.send(Err(error));
            return;
        }
        let Some(live) = self.orchestrations.get_mut(&lead) else {
            return;
        };
        if action == "board" {
            let since = call.input.get("since").and_then(Value::as_u64).unwrap_or(0);
            let limit = call
                .input
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(50)
                .clamp(1, 200) as usize;
            let _ = call.reply.send(Ok(live.board.view(since, limit)));
            return;
        }
        let key = format!("{from}:{}", call.request_id);
        if let Some(value) = live.posted.get(&key) {
            let _ = call.reply.send(Ok(value.clone()));
            return;
        }
        if live.run.status != RunStatus::Active {
            let _ = call.reply.send(Err(live.run.inactive_reason()));
            return;
        }
        if from.is_empty() {
            let _ = call.reply.send(Err(
                "This agent is still joining the board; retry shortly.".into()
            ));
            return;
        }
        let text = call
            .input
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let post = match live.board.post(&from, text, monocode_core::time::now_ms()) {
            Ok(post) => post,
            Err(error) => {
                let _ = call.reply.send(Err(error));
                return;
            }
        };
        live.board_dirty = true;
        let value = json!({"seq": post.seq, "to": post.to, "all": post.all});
        if live.posted.len() >= 1_000 {
            live.posted.clear();
        }
        live.posted.insert(key, value.clone());
        let _ = call.reply.send(Ok(value));
        self.deliver_posts(lead, cx);
        self.pump_orchestration(lead, cx);
    }

    /// Hand running workers the posts addressed to them, as guidance on
    /// their current turn. Stopped ones get them with their next turn, the
    /// coordinator when it is idle.
    fn deliver_posts(&mut self, lead: i64, cx: &mut Context<Self>) {
        let Some(live) = self.orchestrations.get(&lead) else {
            return;
        };
        let targets: Vec<(String, i64)> = live
            .run
            .tasks
            .iter()
            .filter(|t| t.status == Status::Running)
            .filter_map(|t| {
                let member = live
                    .board
                    .members
                    .iter()
                    .find(|m| m.task.as_deref() == Some(t.id.as_str()))?;
                live.board
                    .pending(&member.handle)
                    .then(|| (member.handle.clone(), t.session))
                    .and_then(|(handle, session)| Some((handle, session?)))
            })
            .collect();
        for (handle, session) in targets {
            let Some(view) = self.sessions.get(&session).cloned() else {
                continue;
            };
            let Some(live) = self.orchestrations.get_mut(&lead) else {
                return;
            };
            let before = live.board.member(&handle).map_or(0, |m| m.delivered);
            let Some(text) = live.board.inbox(&handle) else {
                continue;
            };
            live.board_dirty = true;
            let steered = view.update(cx, |v, cx| v.steer_external(text, cx));
            if !steered
                && let Some(live) = self.orchestrations.get_mut(&lead)
                && let Some(member) = live.board.members.iter_mut().find(|m| m.handle == handle)
            {
                member.delivered = before;
            }
        }
    }

    /// The lead whose board goes with chat `id`: its own, or its lead's.
    fn swarm_lead(&self, id: i64) -> Option<i64> {
        if self.orchestrations.contains_key(&id) || self.orchestration_summaries.contains_key(&id) {
            return Some(id);
        }
        self.orchestrations
            .iter()
            .find(|(_, live)| live.run.worker(id).is_some() || live.run.requester == Some(id))
            .map(|(lead, _)| *lead)
            .or_else(|| {
                self.orchestration_summaries
                    .iter()
                    .find(|(_, summary)| summary.tasks.iter().any(|t| t.session == Some(id)))
                    .map(|(lead, _)| *lead)
            })
    }

    /// In a Chief run, the members that are monos: Chief as the
    /// coordinator, and each task's mono.
    fn member_looks(
        &self,
        lead: i64,
        board: &Board,
        cx: &App,
    ) -> HashMap<String, crate::swarm::MemberLook> {
        let (Some(live), Some(monos)) = (self.orchestrations.get(&lead), self.monos.as_ref())
        else {
            return HashMap::new();
        };
        let roster = &monos.read(cx).roster;
        let look = |id: &str| {
            roster
                .monos
                .iter()
                .find(|m| m.id == id)
                .map(|m| crate::swarm::MemberLook {
                    face: m.face().into_owned(),
                    color: m.color,
                    name: m.label(),
                })
        };
        board
            .members
            .iter()
            .filter_map(|member| {
                let mono = if member.handle == board::COORDINATOR {
                    live.run.requester?;
                    // There is one Chief; the roster keeps it across restarts.
                    roster
                        .monos
                        .iter()
                        .find(|m| monocode_core::mono::crew::is_chief(m))?
                        .id
                        .clone()
                } else {
                    live.run.task(member.task.as_deref()?)?.mono.clone()?
                };
                Some((member.handle.clone(), look(&mono)?))
            })
            .collect()
    }

    /// Show the board of the chat in view in the Swarm tab, or drop the tab.
    pub(super) fn sync_swarm(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = self.right_panel.clone() else {
            return;
        };
        let lead = self.workspace.active.and_then(|id| self.swarm_lead(id));
        let board = lead.and_then(|lead| match self.orchestration_control.boards.get(&lead) {
            Some(board) => board.clone(),
            None => {
                self.load_board(lead, cx);
                None
            }
        });
        let swarm = panel.read(cx).swarm.clone();
        let had = panel.read(cx).swarm_tab();
        match (lead, board) {
            (Some(lead), Some(board)) => {
                let looks = self.member_looks(lead, &board, cx);
                swarm.update(cx, |s, cx| {
                    s.set_board(lead, board, cx);
                    s.set_looks(looks, cx);
                });
                panel.update(cx, |p, cx| p.set_swarm_tab(true, cx));
            }
            _ => {
                panel.update(cx, |p, cx| p.set_swarm_tab(false, cx));
                swarm.update(cx, |s, cx| s.clear(cx));
            }
        }
        let has = panel.read(cx).swarm_tab();
        if had != has && self.workspace.classic_layout {
            // Classic shows the board as a sidebar tab; without a board that
            // tab gives way to Sessions.
            if !has && self.classic_sidebar_tab == classic_sidebar::ClassicSidebarTab::Swarm {
                self.classic_sidebar_tab = classic_sidebar::ClassicSidebarTab::Sessions;
                panel.update(cx, |p, cx| p.set_sidebar_tab(None, cx));
            }
            self.notify_sidebar(cx);
            self.projects_frame.update(cx, |_, cx| cx.notify());
        }
        self.sync_agents(cx);
    }

    /// The Agents tab follows the chat in view.
    pub(super) fn sync_agents(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = self.right_panel.clone() else {
            return;
        };
        let chat = self
            .workspace
            .active
            .and_then(|id| self.sessions.get(&id).map(|chat| (id, chat.downgrade())));
        let agents = panel.read(cx).agents.clone();
        agents.update(cx, |pane, cx| pane.set_chat(chat, cx));
    }

    /// The Agents tab came or went; in Classic its sidebar tab gives way to
    /// Sessions when it goes.
    pub(super) fn agents_tab_changed(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = self.right_panel.clone() else {
            return;
        };
        if !self.workspace.classic_layout {
            return;
        }
        if !panel.read(cx).agents_tab()
            && self.classic_sidebar_tab == classic_sidebar::ClassicSidebarTab::Agents
        {
            self.classic_sidebar_tab = classic_sidebar::ClassicSidebarTab::Sessions;
            panel.update(cx, |p, cx| p.set_sidebar_tab(None, cx));
        }
        self.notify_sidebar(cx);
        self.projects_frame.update(cx, |_, cx| cx.notify());
    }

    /// A new swarm opens its tab beside the coordinator.
    fn reveal_swarm(&mut self, cx: &mut Context<Self>) {
        self.orchestration_control.show_swarm = true;
        cx.notify();
    }

    /// Show the board of the chat in view: the Swarm tab of the right panel,
    /// opening the panel, or of the sidebar in Classic.
    fn show_swarm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_swarm(cx);
        let Some(panel) = self.right_panel.clone() else {
            return;
        };
        if !panel.read(cx).swarm_tab() {
            return;
        }
        if self.workspace.classic_layout {
            if self.session_sidebar_available(cx) {
                self.classic_sidebar_tab = classic_sidebar::ClassicSidebarTab::Swarm;
                self.sidebar_auto_hidden = false;
                self.set_sidebar_visible(true, cx);
                self.sync_right_panel(cx);
                self.notify_sidebar(cx);
                self.projects_frame.update(cx, |_, cx| cx.notify());
                cx.notify();
            }
            return;
        }
        self.set_right_panel_open(true, window, cx);
        panel.update(cx, |p, cx| {
            p.select(crate::right_panel::RightTab::Swarm, cx)
        });
    }

    /// Read a lead's saved board off the UI thread.
    fn load_board(&mut self, lead: i64, cx: &mut Context<Self>) {
        if !self.orchestration_control.loading.insert(lead) {
            return;
        }
        let store = self.store.clone();
        let work = cx.background_executor().spawn(async move {
            locked(&store)
                .and_then(|s| s.setting(&board::setting_key(lead)))
                .ok()
                .flatten()
                .and_then(|raw| serde_json::from_str::<Board>(&raw).ok())
        });
        cx.spawn(async move |this, cx| {
            let saved = work.await;
            let _ = this.update(cx, |this, cx| {
                this.orchestration_control.loading.remove(&lead);
                if let Some(live) = this.orchestrations.get_mut(&lead)
                    && !live.board_loaded
                {
                    if let Some(saved) = saved {
                        live.board = saved;
                    }
                    live.board_loaded = true;
                    live.board_dirty = true;
                    this.publish_orchestration(lead, cx);
                    return;
                }
                this.orchestration_control
                    .boards
                    .entry(lead)
                    .or_insert_with(|| saved.map(Arc::new));
                this.sync_swarm(cx);
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests;
