//! The Agents tab: what the Claude chat in view has running beside it, live.
//! Each workflow run with its phases and their agents, then the subagents it
//! started on its own, each with its state, time and latest step. Claude
//! streams none of this, so the pane polls `monocode_core::claude_runs` on a
//! background thread: every 1.5 s while on screen, every 6 s otherwise (only
//! to know whether the tab should be there).
use crate::{conversation::Conversation, right_panel::stroke, swarm::live_color, theme::*};
use gpui::{
    AnyElement, App, Context, EventEmitter, FocusHandle, Focusable, FontWeight, Role, SharedString,
    Task, WeakEntity, Window, div, prelude::*, px,
};
use monocode_core::claude_runs::{Agent, Phase, Run, Scanner, Snapshot, State};
use monocode_core::provider::Kind;
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

const SHOWN_POLL: Duration = Duration::from_millis(1_500);
const HIDDEN_POLL: Duration = Duration::from_secs(6);

/// The tab came or went: the chat in view started or lost its agents.
pub(crate) struct AgentsChanged;

pub(crate) struct AgentsPane {
    chat: Option<(i64, WeakEntity<Conversation>)>,
    snapshot: Snapshot,
    shown: bool,
    scanner: Arc<Mutex<Scanner>>,
    /// The session folder found for `(home, provider id)`.
    dir: Option<(PathBuf, String, PathBuf)>,
    /// Runs whose open/closed state the user flipped from the default
    /// (running runs open, finished ones closed).
    toggled: HashSet<String>,
    poll: Option<Task<()>>,
    focus: FocusHandle,
}

impl EventEmitter<AgentsChanged> for AgentsPane {}
impl Focusable for AgentsPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl AgentsPane {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        Self {
            chat: None,
            snapshot: Snapshot::default(),
            shown: false,
            scanner: Arc::default(),
            dir: None,
            toggled: HashSet::new(),
            poll: None,
            focus: cx.focus_handle(),
        }
    }

    /// Whether the chat in view has any subagents or workflows to show.
    pub(crate) fn has_agents(&self) -> bool {
        !self.snapshot.is_empty()
    }

    /// Follow chat `id`, or nothing.
    pub(crate) fn set_chat(
        &mut self,
        chat: Option<(i64, WeakEntity<Conversation>)>,
        cx: &mut Context<Self>,
    ) {
        if self.chat.as_ref().map(|(id, _)| *id) == chat.as_ref().map(|(id, _)| *id) {
            return;
        }
        self.chat = chat;
        self.scanner = Arc::default();
        self.dir = None;
        self.toggled.clear();
        self.apply(Snapshot::default(), cx);
        self.restart(cx);
    }

    pub(crate) fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        if self.shown == shown {
            return;
        }
        self.shown = shown;
        // Coming on screen reads at once rather than at the slow pace.
        self.restart(cx);
        cx.notify();
    }

    fn restart(&mut self, cx: &mut Context<Self>) {
        if self.chat.is_none() {
            self.poll = None;
            return;
        }
        self.poll = Some(cx.spawn(async move |this, cx| {
            loop {
                let Ok((source, scanner, dir)) = this.update(cx, |this, cx| {
                    (this.source(cx), this.scanner.clone(), this.dir.clone())
                }) else {
                    return;
                };
                let (snapshot, found) = match source {
                    Some((home, id)) => {
                        cx.background_executor()
                            .spawn(async move {
                                let dir = dir
                                    .filter(|(h, i, _)| *h == home && *i == id)
                                    .map(|(_, _, dir)| dir)
                                    .or_else(|| {
                                        monocode_core::claude_runs::session_dir(&home, &id)
                                    });
                                let snapshot = match &dir {
                                    Some(dir) => scanner
                                        .lock()
                                        .map(|mut s| s.scan(dir, monocode_core::time::now_ms()))
                                        .unwrap_or_default(),
                                    None => Snapshot::default(),
                                };
                                (snapshot, dir.map(|dir| (home, id, dir)))
                            })
                            .await
                    }
                    None => (Snapshot::default(), None),
                };
                let Ok(shown) = this.update(cx, |this, cx| {
                    if found.is_some() {
                        this.dir = found;
                    }
                    this.apply(snapshot, cx);
                    this.shown
                }) else {
                    return;
                };
                cx.background_executor()
                    .timer(if shown { SHOWN_POLL } else { HIDDEN_POLL })
                    .await;
            }
        }));
    }

    /// Claude's config folder and session id for the chat in view, read from
    /// memory only.
    fn source(&self, cx: &App) -> Option<(PathBuf, String)> {
        let chat = self.chat.as_ref()?.1.upgrade()?;
        let chat = chat.read(cx);
        if Kind::from_id(&chat.summary.provider) != Kind::Claude {
            return None;
        }
        let id = chat.summary.provider_id.clone()?;
        let home = chat.settings.account.home(Kind::Claude).ok()?;
        Some((home, id))
    }

    fn apply(&mut self, snapshot: Snapshot, cx: &mut Context<Self>) {
        let had = self.has_agents();
        if snapshot != self.snapshot {
            self.snapshot = snapshot;
            cx.notify();
        } else if self.shown && self.snapshot.running() > 0 {
            // Running clocks tick.
            cx.notify();
        }
        if had != self.has_agents() {
            cx.emit(AgentsChanged);
        }
    }

    fn open(&self, run: &Run) -> bool {
        (run.state() == State::Running) != self.toggled.contains(&run.id)
    }

    fn header(&self) -> AnyElement {
        let running = self.snapshot.running();
        div()
            .id("agents-header")
            .flex_shrink_0()
            .h(px(PANE_HEADER_HEIGHT))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(stroke())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(content(0.95))
                    .child("Agents"),
            )
            .child(
                div()
                    .id("agents-live")
                    .debug_selector(|| "agents-live".into())
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_size(px(11.5))
                    .text_color(content(0.6))
                    .child(div().size(px(6.)).rounded_full().bg(if running > 0 {
                        live_color()
                    } else {
                        content(0.3)
                    }))
                    .child(if running > 0 {
                        format!("{running} running")
                    } else {
                        "Idle".into()
                    }),
            )
            .into_any_element()
    }

    fn run_card(&self, run: &Run, now: u64, cx: &mut Context<Self>) -> AnyElement {
        let open = self.open(run);
        let state = run.state();
        let total = run.agents().count();
        let done = run.agents().filter(|a| a.state == State::Done).count();
        let started = run.agents().filter_map(|a| a.started_at).min();
        let toggle = run.id.clone();
        let header = div()
            .id(SharedString::from(format!("agents-run-{}", run.id)))
            .debug_selector({
                let id = run.id.clone();
                move || format!("agents-run-{id}")
            })
            .role(Role::Button)
            .aria_label(format!("Workflow {}", run.name))
            .aria_expanded(open)
            .cursor_pointer()
            .px_2p5()
            .py_2()
            .flex()
            .items_start()
            .gap_2()
            .hover(|s| s.bg(content(0.03)))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.toggled.remove(&toggle) {
                    this.toggled.insert(toggle.clone());
                }
                cx.notify();
            }))
            .child(div().pt(px(2.)).text_color(content(0.45)).child(icon(
                if open { "chevron-down-2" } else { "chevron-up" },
                12.,
            )))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(content(0.92))
                                    .child(run.name.clone()),
                            )
                            .child(state_chip(state)),
                    )
                    .when_some(run.description.clone(), |d, description| {
                        d.child(
                            div()
                                .text_size(px(11.5))
                                .text_color(content(0.5))
                                .line_clamp(2)
                                .child(description),
                        )
                    })
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(content(0.4))
                            .child(format!(
                                "{done}/{total} agents · {} phases{}",
                                run.phases.len(),
                                started
                                    .map(|at| {
                                        let end = if state.active() { now } else { run.updated_at };
                                        format!(" · {}", elapsed(end.saturating_sub(at)))
                                    })
                                    .unwrap_or_default()
                            )),
                    ),
            );
        div()
            .flex_shrink_0()
            .rounded(px(8.))
            .border_1()
            .border_color(content(0.07))
            .bg(content(0.02))
            .overflow_hidden()
            .child(header)
            .when(open, |d| {
                d.child(
                    div()
                        .border_t_1()
                        .border_color(content(0.05))
                        .py_1()
                        .flex()
                        .flex_col()
                        .children(run.phases.iter().map(|phase| phase_block(phase, now))),
                )
            })
            .into_any_element()
    }
}

fn phase_block(phase: &Phase, now: u64) -> AnyElement {
    let state = phase.state();
    let done = phase
        .agents
        .iter()
        .filter(|a| a.state == State::Done)
        .count();
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .px_2p5()
                .pt_1p5()
                .pb_0p5()
                .flex()
                .items_center()
                .gap_2()
                .child(state_glyph(state, 12.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(content(if state.is_some() { 0.8 } else { 0.4 }))
                        .child(phase.title.clone()),
                )
                .when(!phase.agents.is_empty(), |d| {
                    d.child(
                        div()
                            .text_size(px(11.))
                            .text_color(content(0.4))
                            .child(format!("{done}/{}", phase.agents.len())),
                    )
                }),
        )
        .children(phase.agents.iter().map(|agent| agent_row(agent, now, 22.)))
        .into_any_element()
}

fn agent_row(agent: &Agent, now: u64, indent: f32) -> AnyElement {
    let line = match agent.state {
        State::Done | State::Failed => agent.summary.clone().or_else(|| step_line(agent)),
        _ => step_line(agent),
    };
    let end = if agent.state.active() {
        now
    } else {
        agent.updated_at
    };
    div()
        .id(SharedString::from(format!("agents-agent-{}", agent.id)))
        .debug_selector({
            let id = agent.id.clone();
            move || format!("agents-agent-{id}")
        })
        .role(Role::Group)
        .aria_label(format!("{}: {}", agent.label, state_label(agent.state)))
        .pl(px(indent))
        .pr_2p5()
        .py_1()
        .flex()
        .items_start()
        .gap_2()
        .child(div().pt(px(3.)).child(state_glyph(Some(agent.state), 10.)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(12.))
                                .text_color(content(0.85))
                                .child(agent.label.clone()),
                        )
                        .when_some(agent.started_at, |d, at| {
                            d.child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(px(11.))
                                    .font_features(crate::right_panel::tabular())
                                    .text_color(content(0.4))
                                    .child(elapsed(end.saturating_sub(at))),
                            )
                        }),
                )
                .when_some(line, |d, line| {
                    d.child(
                        div()
                            .truncate()
                            .text_size(px(11.))
                            .text_color(content(0.45))
                            .when(agent.state.active(), |d| {
                                d.font_family(crate::fonts::mono())
                            })
                            .child(line),
                    )
                }),
        )
        .into_any_element()
}

/// `Bash · cargo test`, or the agent's latest sentence.
fn step_line(agent: &Agent) -> Option<String> {
    let step = agent.last.as_ref()?;
    Some(match (&step.tool, step.text.is_empty()) {
        (Some(tool), true) => tool.clone(),
        (Some(tool), false) => format!("{tool} · {}", step.text),
        (None, _) => step.text.clone(),
    })
}

fn state_label(state: State) -> &'static str {
    match state {
        State::Running => "Running",
        State::Done => "Done",
        State::Failed => "Failed",
        State::Stopped => "Stopped",
    }
}

fn state_chip(state: State) -> AnyElement {
    let color = match state {
        State::Running => live_color(),
        State::Failed => gpui::rgb(crate::right_panel::RED_400).into(),
        _ => content(0.5),
    };
    div()
        .flex_shrink_0()
        .px_1p5()
        .rounded(px(4.))
        .bg(color.opacity(0.12))
        .text_size(px(10.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(color)
        .child(state_label(state))
        .into_any_element()
}

/// A still mark: the clocks and the latest steps already show that it moves.
fn state_glyph(state: Option<State>, side: f32) -> AnyElement {
    match state {
        Some(State::Running) => div()
            .size(px(side))
            .flex()
            .items_center()
            .justify_center()
            .child(div().size(px(side * 0.6)).rounded_full().bg(live_color()))
            .into_any_element(),
        Some(State::Done) => icon("check", side)
            .text_color(content(0.55))
            .into_any_element(),
        Some(State::Failed) => icon("circle-x", side)
            .text_color(gpui::rgb(crate::right_panel::RED_400))
            .into_any_element(),
        Some(State::Stopped) => div()
            .size(px(side))
            .flex()
            .items_center()
            .justify_center()
            .child(div().size(px(side * 0.5)).rounded_full().bg(content(0.3)))
            .into_any_element(),
        None => icon("circle-dashed-hi", side)
            .text_color(content(0.3))
            .into_any_element(),
    }
}

fn elapsed(ms: u64) -> String {
    let seconds = ms / 1000;
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let (minutes, rest) = (seconds / 60, seconds % 60);
    if minutes < 60 {
        return format!("{minutes}m {rest:02}s");
    }
    format!("{}h {:02}m", minutes / 60, minutes % 60)
}

impl Render for AgentsPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = monocode_core::time::now_ms();
        let runs: Vec<AnyElement> = self
            .snapshot
            .runs
            .clone()
            .iter()
            .map(|run| self.run_card(run, now, cx))
            .collect();
        let agents = &self.snapshot.agents;
        div()
            .id("agents-panel")
            .debug_selector(|| "agents-panel".into())
            .track_focus(&self.focus)
            .role(Role::Group)
            .aria_label("Agents")
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .child(self.header())
            .child(
                div()
                    .id("agents-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_2()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children(runs)
                    .when(!agents.is_empty(), |d| {
                        d.child(
                            section_label("Subagents")
                                .px_1()
                                .pt_1(),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .children(agents.iter().map(|agent| agent_row(agent, now, 6.))),
                        )
                    })
                    .when(self.snapshot.is_empty(), |d| {
                        d.child(
                            div()
                                .p_3()
                                .text_size(px(12.))
                                .text_color(content(0.45))
                                .child("Subagents and workflows that Claude starts in this chat show up here as they work."),
                        )
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext};
    use monocode_core::claude_runs::Step;

    fn agent(id: &str, state: State) -> Agent {
        Agent {
            id: id.into(),
            label: format!("agent {id}"),
            phase: None,
            state,
            model: None,
            started_at: Some(1_000),
            updated_at: 61_000,
            last: Some(Step {
                tool: Some("Bash".into()),
                text: "cargo test".into(),
            }),
            summary: (state == State::Done).then(|| "All green".into()),
        }
    }

    fn run(id: &str, states: &[State]) -> Run {
        Run {
            id: id.into(),
            name: format!("run {id}"),
            description: Some("Implement it".into()),
            phases: vec![
                Phase {
                    title: "Build".into(),
                    agents: states
                        .iter()
                        .enumerate()
                        .map(|(ix, s)| agent(&format!("{id}{ix}"), *s))
                        .collect(),
                },
                Phase {
                    title: "Review".into(),
                    agents: Vec::new(),
                },
            ],
            updated_at: 61_000,
        }
    }

    #[gpui::test]
    fn running_runs_open_finished_ones_fold_and_a_click_flips_them(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.set_reduce_motion(true);
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut pane = AgentsPane::new(cx);
                    pane.shown = true;
                    pane.apply(
                        Snapshot {
                            runs: vec![
                                run("live", &[State::Done, State::Running]),
                                run("old", &[State::Done]),
                            ],
                            agents: vec![agent("solo", State::Running)],
                        },
                        cx,
                    );
                    pane
                })
            })
            .unwrap()
        });
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("agents-agent-live1").is_some());
        assert!(visual.debug_bounds("agents-agent-old0").is_none());
        assert!(visual.debug_bounds("agents-agent-solo").is_some());
        window
            .update(visual, |pane, _, _| {
                assert!(pane.has_agents());
                assert_eq!(pane.snapshot.running(), 2);
            })
            .unwrap();

        let old = visual.debug_bounds("agents-run-old").unwrap();
        visual.simulate_click(old.center(), Default::default());
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("agents-agent-old0").is_some());

        let live = visual.debug_bounds("agents-run-live").unwrap();
        visual.simulate_click(live.center(), Default::default());
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("agents-agent-live1").is_none());
    }

    #[test]
    fn rows_read_as_a_step_or_a_report() {
        assert_eq!(
            step_line(&agent("a", State::Running)).as_deref(),
            Some("Bash · cargo test")
        );
        assert_eq!(elapsed(60_000), "1m 00s");
    }
}
