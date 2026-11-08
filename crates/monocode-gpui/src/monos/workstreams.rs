//! Workstreams: the first-run tour, the dialog that starts one, the rail's
//! "New workstream" actions, and a workstream's PRs and Files tabs. Mirrors
//! `src/features/workstreams/ui` of the Tauri desktop app; the logic is in
//! `monocode_core::mono::workstreams`.
use super::*;
use crate::{
    input::Changed as InputChanged,
    modal,
    pickers::{PROJECT_PICKER_HEIGHT, ProjectPicked, ProjectPicker},
    settings::widgets::segmented,
};
use gpui::{FocusHandle, Focusable, FontWeight, KeyDownEvent, Pixels, Size, Task};
use monocode_core::mono::workstreams::{
    self, BranchPr, FolderFile, GOAL_MAX, NAME_MAX, NewWorkstream, PrGroup,
};
use std::time::Instant;

/// How often an open PRs or Files tab looks again: agents open pull
/// requests as they finish and write notes as they go.
const PRS_STALE: Duration = Duration::from_secs(60);
const FILES_STALE: Duration = Duration::from_secs(15);

/// What a tab last read, and whether it is reading again.
pub(super) struct Listing<T> {
    pub(super) items: Option<Result<T, String>>,
    pub(super) loading: bool,
    pub(super) loaded_at: Option<Instant>,
}
impl<T> Default for Listing<T> {
    fn default() -> Self {
        Self {
            items: None,
            loading: false,
            loaded_at: None,
        }
    }
}
impl<T> Listing<T> {
    fn stale(&self, after: Duration) -> bool {
        !self.loading && self.loaded_at.is_none_or(|at| at.elapsed() >= after)
    }
}

/// Reads one tab's list again.
type Refresh = fn(&mut Monos, &Mono, &mut Context<Monos>);

/// Everything Workstreams keeps on the Mono manager.
#[derive(Default)]
pub(super) struct State {
    pub(super) intro: Option<(Entity<WorkstreamIntro>, Subscription, Size<Pixels>)>,
    pub(super) dialog: Option<(Entity<NewWorkstreamDialog>, Subscription, Size<Pixels>)>,
    pub(super) prs: HashMap<String, Listing<Vec<BranchPr>>>,
    pub(super) files: HashMap<String, Listing<Vec<FolderFile>>>,
    poll: Option<Task<()>>,
}

// ---------------------------------------------------------------------------
// The tour

/// One node of a tour diagram: the lead in the accent, the rest plain.
fn node(text: &'static str, lead: bool) -> gpui::Div {
    div()
        .flex_shrink_0()
        .rounded_md()
        .border_1()
        .px_2p5()
        .py_1()
        .text_size(px(12.))
        .map(|d| {
            if lead {
                d.border_color(accent_color().opacity(0.4))
                    .bg(accent_color().opacity(0.1))
                    .text_color(accent_color())
            } else {
                d.border_color(content(0.1))
                    .bg(background_base())
                    .text_color(content(0.7))
            }
        })
        .child(text)
}

/// One node handing work to several.
fn fan(from: &'static str, to: [&'static str; 3]) -> AnyElement {
    div()
        .flex()
        .items_center()
        .justify_center()
        .gap_3()
        .child(node(from, true))
        .child(div().h(px(1.)).w(px(24.)).bg(content(0.15)))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1p5()
                .border_l_1()
                .border_color(content(0.15))
                .pl_3()
                .children(to.map(|text| node(text, false))),
        )
        .into_any_element()
}

/// You over the orchestrator over its agents.
fn chain() -> AnyElement {
    let line = || div().h(px(16.)).w(px(1.)).bg(content(0.15));
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_1p5()
        .child(node("You", false))
        .child(line())
        .child(node("Orchestrator", true))
        .child(line())
        .child(
            div()
                .flex()
                .gap_1p5()
                .children(["Agent"; 3].map(|text| node(text, false))),
        )
        .into_any_element()
}

pub(super) struct Step {
    pub(super) title: &'static str,
    pub(super) body: &'static str,
    diagram: fn() -> AnyElement,
}

pub(super) const STEPS: [Step; 4] = [
    Step {
        title: "A goal, not a single task",
        body: "Give a workstream something that spans several parts of a repository: a migration, a feature across front end and back end, a batch of bugs. An orchestrator plans it with you and runs one agent per part at the same time.",
        diagram: || {
            fan(
                "Orchestrator",
                ["Agent · API", "Agent · Web", "Agent · Tests"],
            )
        },
    },
    Step {
        title: "Each agent on its own branch",
        body: "Every agent works in its own worktree on the branch ws/<name>/<part>, so agents never touch each other's files. When an agent finishes, it opens a draft pull request and reports back to the orchestrator.",
        diagram: || fan("Repository", ["ws/…/api", "ws/…/web", "ws/…/tests"]),
    },
    Step {
        title: "You decide, it coordinates",
        body: "Talk to the orchestrator in its chat. It writes the plan to the workstream's shared folder, asks you only for the decisions that are yours, and applies the rules you set to every agent.",
        diagram: chain,
    },
    Step {
        title: "Review what comes back",
        body: "Follow each agent in Agents, read plans and notes in Files, and review the work in PRs. Agents run on this computer and stop when you quit MonoBOT. A workstream uses far more tokens than one session, so keep it for large work.",
        diagram: || fan("Workstream", ["Agents", "Files", "PRs"]),
    },
];

pub(super) enum IntroEvent {
    Start,
    Close,
}

/// WorkstreamIntro: the four-step tour shown once before the first workstream.
pub(super) struct WorkstreamIntro {
    pub(super) step: usize,
    focus: FocusHandle,
}
impl EventEmitter<IntroEvent> for WorkstreamIntro {}
impl Focusable for WorkstreamIntro {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl WorkstreamIntro {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            step: 0,
            focus: cx.focus_handle(),
        }
    }
}
impl Render for WorkstreamIntro {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let step = &STEPS[self.step];
        let last = self.step + 1 == STEPS.len();
        div()
            .id("workstream-intro")
            .debug_selector(|| "workstream-intro".into())
            .key_context("WorkstreamIntro")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(IntroEvent::Close);
                    cx.stop_propagation();
                }
            }))
            .size_full()
            .flex()
            .flex_col()
            .text_color(content(1.))
            .child(
                div()
                    .h(px(192.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_b_1()
                    .border_color(content(0.07))
                    .bg(content(0.03))
                    .px(px(20.))
                    .child((step.diagram)()),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .px(px(20.))
                    .pt_4()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(px(12.))
                            .text_color(content(0.5))
                            .child("Workstreams")
                            .child(beta_badge())
                            .child(div().ml_auto().child(format!(
                                "{} of {}",
                                self.step + 1,
                                STEPS.len()
                            ))),
                    )
                    .child(
                        div()
                            .debug_selector(|| "workstream-intro-title".into())
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(step.title),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(20.))
                            .text_color(content(0.65))
                            .child(step.body),
                    ),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px(px(20.))
                    .pt_5()
                    .pb_4()
                    .child(div().flex().gap_1p5().children((0..STEPS.len()).map(|ix| {
                        div().size(px(6.)).rounded_full().bg(if ix == self.step {
                            accent_color()
                        } else {
                            content(0.2)
                        })
                    })))
                    .child(div().flex_1())
                    .child(
                        modal::ghost_button("workstream-intro-skip", "Skip")
                            .debug_selector(|| "workstream-intro-skip".into())
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(IntroEvent::Close))),
                    )
                    .when(self.step > 0, |d| {
                        d.child(
                            modal::ghost_button("workstream-intro-back", "Back")
                                .debug_selector(|| "workstream-intro-back".into())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.step = this.step.saturating_sub(1);
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        modal::primary_button(
                            "workstream-intro-next",
                            if last { "Start a workstream" } else { "Next" },
                        )
                        .debug_selector(|| "workstream-intro-next".into())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if last {
                                cx.emit(IntroEvent::Start);
                            } else {
                                this.step += 1;
                                cx.notify();
                            }
                        })),
                    ),
            )
    }
}

// ---------------------------------------------------------------------------
// The new-workstream dialog

/// What the dialog hands back: the core's input plus the goal, sent to the
/// orchestrator as its first message.
#[derive(Clone, Debug)]
pub(super) struct Draft {
    pub(super) input: NewWorkstream,
    pub(super) goal: String,
}

pub(super) enum DialogEvent {
    Start(Draft),
    Close,
}

/// NewWorkstreamDialog: names the workstream, gives its orchestrator the
/// goal and the repository, and picks where the orchestrator runs. Its
/// agents always get worktrees of their own.
pub(super) struct NewWorkstreamDialog {
    pub(super) name: Entity<Input>,
    pub(super) goal: Entity<Input>,
    pub(super) project: String,
    projects: Vec<String>,
    pub(super) worktree: bool,
    pub(super) busy: bool,
    pub(super) error: Option<String>,
    picker: Option<(Entity<ProjectPicker>, Subscription)>,
    project_anchor: AnchorSlot,
    picker_region: AnchorSlot,
    focus: FocusHandle,
    _changed: Subscription,
}
impl EventEmitter<DialogEvent> for NewWorkstreamDialog {}
impl Focusable for NewWorkstreamDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl NewWorkstreamDialog {
    pub(super) fn new(project: String, projects: Vec<String>, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| {
            let mut field = Input::field("Name", "Move the API to Hono", content(0.35), cx);
            field.set_max_chars(NAME_MAX);
            field
        });
        let goal = cx.new(|cx| {
            let mut field = Input::multiline_field(
                "Goal",
                "What should be true when it is done, how to check it, and what to leave alone.",
                cx,
            );
            field.set_max_chars(GOAL_MAX);
            field
        });
        let changed = cx.subscribe(&name, |_, _, _: &InputChanged, cx| cx.notify());
        Self {
            name,
            goal,
            project,
            projects,
            worktree: true,
            busy: false,
            error: None,
            picker: None,
            project_anchor: AnchorSlot::default(),
            picker_region: AnchorSlot::default(),
            focus: cx.focus_handle(),
            _changed: changed,
        }
    }
    pub(super) fn ready(&self, cx: &App) -> bool {
        !self.busy && workstreams::can_start(&self.name.read(cx).content, &self.project)
    }
    pub(super) fn submit(&mut self, cx: &mut Context<Self>) {
        if !self.ready(cx) {
            return;
        }
        self.busy = true;
        self.error = None;
        cx.emit(DialogEvent::Start(Draft {
            input: NewWorkstream {
                name: self.name.read(cx).content.trim().to_owned(),
                project: self.project.clone(),
                worktree: self.worktree,
            },
            goal: self.goal.read(cx).content.trim().to_owned(),
        }));
        cx.notify();
    }
    /// The workstream could not start: say why and let the user try again.
    pub(super) fn failed(&mut self, error: String, cx: &mut Context<Self>) {
        self.busy = false;
        self.error = Some(error);
        cx.notify();
    }
    fn close(&mut self, cx: &mut Context<Self>) {
        if !self.busy {
            cx.emit(DialogEvent::Close);
        }
    }
    fn choose_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let picker = cx.new(|cx| ProjectPicker::new(&self.projects, &self.project, cx));
        let events = cx.subscribe_in(
            &picker,
            window,
            |this, _, picked: &ProjectPicked, window, cx| {
                this.picker = None;
                match picked {
                    ProjectPicked::Project(path) => this.project = path.clone(),
                    ProjectPicked::New => this.browse_project(cx),
                    ProjectPicked::Remote | ProjectPicked::Mono(_) => {}
                }
                window.focus(&this.focus, cx);
                cx.notify();
            },
        );
        window.focus(&picker.read(cx).search_focus(cx), cx);
        self.picker = Some((picker, events));
        cx.notify();
    }
    fn browse_project(&mut self, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose repository".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = prompt.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.project = path.to_string_lossy().into_owned();
                cx.notify();
            });
        })
        .detach();
    }
    /// The picker's surface, which the dialog's own popover must not take
    /// for a press outside it.
    fn picker_region(&self) -> AnchorSlot {
        self.picker_region.clone()
    }
}

/// The dialog's labels, hints and field frames (`text-[12px] text-content/70`).
fn label(text: &'static str) -> gpui::Div {
    div()
        .text_size(px(12.))
        .text_color(content(0.7))
        .child(text)
}
fn hint(text: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_size(px(11.))
        .line_height(px(15.))
        .text_color(content(0.45))
        .child(text.into())
}
fn field_box() -> gpui::Div {
    div()
        .rounded_md()
        .border_1()
        .border_color(content(0.1))
        .bg(background_base())
        .px_2p5()
        .py_1p5()
        .text_size(px(13.))
}

impl Render for NewWorkstreamDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let name = self.name.read(cx).content.clone();
        let ready = self.ready(cx);
        let busy = self.busy;
        let project = (!self.project.trim().is_empty() && self.project != "~")
            .then(|| crate::workspace::project_name(&self.project));
        let weak = cx.weak_entity();
        div()
            .id("new-workstream-dialog")
            .debug_selector(|| "new-workstream-dialog".into())
            .key_context("NewWorkstreamDialog")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.close(cx);
                    cx.stop_propagation();
                }
            }))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .text_color(content(1.))
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(20.))
                    .pt_4()
                    .pb_3()
                    .child(
                        div()
                            .text_size(px(15.))
                            .line_height(px(20.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("New workstream"),
                    )
                    .child(
                        div()
                            .mt_1()
                            .text_size(px(12.))
                            .text_color(content(0.55))
                            .child("An orchestrator and its agents, working in parallel"),
                    ),
            )
            .child(
                div()
                    .id("new-workstream-body")
                    .debug_selector(|| "new-workstream-body".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(20.))
                    .pb_4()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .child(label("Name"))
                                    .child(
                                        div()
                                            .ml_auto()
                                            .text_size(px(12.))
                                            .text_color(content(0.35))
                                            .child(format!(
                                                "{}/{NAME_MAX}",
                                                name.chars().count()
                                            )),
                                    ),
                            )
                            .child(
                                field_box()
                                    .debug_selector(|| "new-workstream-name".into())
                                    .child(self.name.clone()),
                            )
                            .when(!name.trim().is_empty(), |d| {
                                d.child(
                                    div()
                                        .debug_selector(|| "new-workstream-branches".into())
                                        .flex()
                                        .gap_1()
                                        .child(hint("Agents' branches:"))
                                        .child(
                                            hint(format!(
                                                "{}<part>",
                                                workstreams::branch_prefix(&workstreams::slug(
                                                    &name
                                                ))
                                            ))
                                            .font_family(crate::fonts::mono())
                                            .text_color(content(0.6)),
                                        ),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .child(label("Goal"))
                            .child(
                                field_box()
                                    .debug_selector(|| "new-workstream-goal".into())
                                    .child(self.goal.clone()),
                            )
                            .child(hint(
                                "The orchestrator gets it as its first message and starts the plan. Leave it empty to explain it in the chat.",
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .child(label("Repository"))
                            .child(
                                field_box()
                                    .id("new-workstream-project")
                                    .debug_selector(|| "new-workstream-project".into())
                                    .role(Role::Button)
                                    .aria_label("Repository")
                                    .relative()
                                    .h(px(36.))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .cursor_pointer()
                                    .hover(|s| s.border_color(content(0.25)))
                                    .child(self.project_anchor.probe())
                                    .child(icon("folder-01", 14.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .when(project.is_none(), |d| {
                                                d.text_color(content(0.35))
                                            })
                                            .child(
                                                project
                                                    .clone()
                                                    .unwrap_or_else(|| "Choose a repository".into()),
                                            ),
                                    )
                                    .child(icon("chevron-down", 12.))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.choose_project(window, cx)
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .child(label("Orchestrator runs in"))
                            .child(segmented(
                                "workstream-checkout",
                                "Orchestrator runs in",
                                if self.worktree { "worktree" } else { "project" },
                                &[
                                    ("worktree", "Its own worktree"),
                                    ("project", "The project folder"),
                                ],
                                "new",
                                move |option, _, cx| {
                                    let _ = weak.update(cx, |this, cx| {
                                        if !this.busy {
                                            this.worktree = option == "worktree";
                                            cx.notify();
                                        }
                                    });
                                },
                            ))
                            .child(hint(if self.worktree {
                                "Your open files stay untouched. Each agent gets a worktree of its own either way."
                            } else {
                                "The orchestrator shares the folder you work in. Each agent still gets a worktree of its own."
                            })),
                    )
                    .when_some(self.error.clone(), |d, error| {
                        d.child(
                            div()
                                .id("new-workstream-error")
                                .debug_selector(|| "new-workstream-error".into())
                                .role(Role::Alert)
                                .text_size(px(12.))
                                .text_color(rgb(0xf87171))
                                .child(error),
                        )
                    }),
            )
            .child(
                div()
                    .debug_selector(|| "new-workstream-footer".into())
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_t_1()
                    .border_color(content(0.1))
                    .px(px(20.))
                    .py_3()
                    .child(beta_badge())
                    .child(
                        hint("Runs on this computer and stops when you quit MonoBOT.")
                            .flex_1()
                            .min_w_0(),
                    )
                    .child(
                        modal::disabled(
                            modal::ghost_button("new-workstream-cancel", "Cancel"),
                            busy,
                        )
                        .debug_selector(|| "new-workstream-cancel".into())
                        .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    )
                    .child(
                        modal::disabled(
                            modal::primary_button(
                                "new-workstream-start",
                                if busy { "Starting…" } else { "Start workstream" },
                            ),
                            !ready,
                        )
                        .debug_selector(|| "new-workstream-start".into())
                        .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                    ),
            )
            .when_some(self.picker.as_ref(), |d, (picker, _)| {
                let weak = cx.weak_entity();
                d.child(
                    popover(
                        "new-workstream-project-picker",
                        Anchor::Slot(self.project_anchor.clone()),
                    )
                    .priority(96)
                    .width(320.)
                    .height(PROJECT_PICKER_HEIGHT)
                    .track(self.picker_region.clone())
                    .on_dismiss(move |_, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            this.picker = None;
                            cx.notify();
                        });
                    })
                    .child(picker.clone()),
                )
            })
    }
}

// ---------------------------------------------------------------------------
// The Mono manager's side

impl Monos {
    /// The rail's "New workstream": the tour the first time, then the dialog
    /// that names it (App.tsx's `onCreateWorkstream`).
    pub(super) fn new_workstream(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workstreams.intro.is_some() || self.workstreams.dialog.is_some() {
            return;
        }
        if self.roster.workstream_intro_seen {
            self.open_workstream_dialog(window, cx);
            return;
        }
        let view = cx.new(WorkstreamIntro::new);
        let events = cx.subscribe_in(&view, window, |this, _, event: &IntroEvent, window, cx| {
            // Seen once it shows, whether started or skipped.
            this.roster.workstream_intro_seen = true;
            this.save(cx);
            this.workstreams.intro = None;
            if matches!(event, IntroEvent::Start) {
                this.open_workstream_dialog(window, cx);
            }
            cx.notify();
        });
        window.focus(&view.focus_handle(cx), cx);
        self.workstreams.intro = Some((view, events, window.viewport_size()));
        cx.notify();
    }

    fn open_workstream_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The repository starts at the project the sidebar is on.
        let (project, projects) = self
            .owner
            .upgrade()
            .map(|owner| {
                let workspace = &owner.read(cx).workspace;
                (workspace.project.clone(), workspace.projects.clone())
            })
            .unwrap_or_default();
        let view = cx.new(|cx| NewWorkstreamDialog::new(project, projects, cx));
        let events = cx.subscribe_in(
            &view,
            window,
            |this, view, event: &DialogEvent, window, cx| match event {
                DialogEvent::Start(draft) => {
                    this.start_workstream(view.clone(), draft.clone(), window, cx)
                }
                DialogEvent::Close => {
                    this.workstreams.dialog = None;
                    cx.notify();
                }
            },
        );
        window.focus(&view.read(cx).name.focus_handle(cx), cx);
        self.workstreams.dialog = Some((view, events, window.viewport_size()));
        cx.notify();
    }

    /// App.tsx's `onStartWorkstream`: the shared folder and the orchestrator's
    /// checkout off the UI thread, then its Mono, opened on its Agents, with
    /// the goal as its first message so it starts planning.
    fn start_workstream(
        &mut self,
        view: Entity<NewWorkstreamDialog>,
        draft: Draft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let data_dir = self
            .root
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| self.root.clone());
        let input = draft.input.clone();
        let work = cx
            .background_executor()
            .spawn(async move { workstreams::create(&data_dir, &input) });
        let window = window.window_handle();
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = cx.update_window(window, |_, window, cx| {
                let _ = this.update(cx, |this, cx| match result {
                    Ok(created) => this.finish_workstream(created, &draft.goal, window, cx),
                    Err(error) => view.update(cx, |view, cx| view.failed(error, cx)),
                });
            });
        })
        .detach();
    }

    pub(super) fn finish_workstream(
        &mut self,
        created: workstreams::Created,
        goal: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.roster.create_workstream(created).id.clone();
        self.workstreams.dialog = None;
        self.save(cx);
        self.open(&id, window, cx);
        self.overview_tab = OverviewTab::Sessions;
        self.show_overview(cx);
        if !goal.is_empty() {
            self.enqueue(&id, goal.to_owned(), Vec::new(), false, cx);
        }
        self.changed(cx);
    }

    /// The tour or the dialog over the whole surface, like the group editor.
    pub(crate) fn workstream_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if let Some((view, _, size)) = &self.workstreams.intro {
            let weak = cx.weak_entity();
            return Some(self.centered_dialog(
                "workstream-intro-modal",
                *size,
                (448., 400.),
                None,
                view.clone().into_any_element(),
                move |_, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        this.roster.workstream_intro_seen = true;
                        this.save(cx);
                        this.workstreams.intro = None;
                        cx.notify();
                    });
                },
            ));
        }
        let (view, _, size) = self.workstreams.dialog.as_ref()?;
        let weak = view.downgrade();
        Some(self.centered_dialog(
            "new-workstream-modal",
            *size,
            (520., 600.),
            Some(view.read(cx).picker_region()),
            view.clone().into_any_element(),
            move |_, cx| {
                let _ = weak.update(cx, |view, cx| view.close(cx));
            },
        ))
    }

    /// The Workstreams label's "+".
    pub(super) fn new_workstream_button(&mut self, cx: &mut Context<Self>) -> AnyElement {
        icon_button("new-workstream", "New workstream", "plus")
            .debug_selector(|| "sidebar-new-workstream".into())
            .size(px(20.))
            .mr(px(-3.))
            .on_click(cx.listener(|this, _, w, cx| this.new_workstream(w, cx)))
            .into_any_element()
    }

    /// With no workstream yet, a "New workstream" row starts one.
    pub(super) fn new_workstream_row(
        &mut self,
        original: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id("new-workstream-row")
            .debug_selector(|| "new-workstream".into())
            .role(Role::Button)
            .aria_label("New workstream")
            .h(px(32.))
            .when(original, |d| d.ml(px(6.)).mr_2())
            .px_2()
            .rounded_md()
            .flex()
            .items_center()
            .gap_2()
            .text_size(px(14.))
            .text_color(content(0.5))
            .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
            .child(icon("plus", 16.))
            .child("New workstream")
            .on_click(cx.listener(|this, _, w, cx| this.new_workstream(w, cx)))
            .into_any_element()
    }

    // -- PRs and Files ------------------------------------------------------

    /// Open pull requests still waiting on the user, for the PRs tab's count.
    pub(super) fn open_pr_count(&self, mono: &Mono) -> usize {
        match self
            .workstreams
            .prs
            .get(&mono.id)
            .and_then(|l| l.items.as_ref())
        {
            Some(Ok(prs)) => prs
                .iter()
                .filter(|pr| workstreams::pr_group(pr) == PrGroup::Open)
                .count(),
            _ => 0,
        }
    }

    pub(super) fn refresh_workstream_prs(&mut self, mono: &Mono, cx: &mut Context<Self>) {
        let Some(workstream) = mono.workstream.clone() else {
            return;
        };
        let listing = self.workstreams.prs.entry(mono.id.clone()).or_default();
        if listing.loading {
            return;
        }
        listing.loading = true;
        let id = mono.id.clone();
        let work = cx
            .background_executor()
            .spawn(async move { workstreams::list_prs(&workstream) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                let listing = this.workstreams.prs.entry(id).or_default();
                listing.items = Some(result);
                listing.loading = false;
                listing.loaded_at = Some(Instant::now());
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn refresh_workstream_files(&mut self, mono: &Mono, cx: &mut Context<Self>) {
        let Some(workstream) = mono.workstream.clone() else {
            return;
        };
        let listing = self.workstreams.files.entry(mono.id.clone()).or_default();
        if listing.loading {
            return;
        }
        listing.loading = true;
        let id = mono.id.clone();
        let work = cx.background_executor().spawn(async move {
            Ok(workstreams::folder_files(std::path::Path::new(
                &workstream.folder,
            )))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                let listing = this.workstreams.files.entry(id).or_default();
                listing.items = Some(result);
                listing.loading = false;
                listing.loaded_at = Some(Instant::now());
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Keeps the shown tab current: reads it now when it is stale, and looks
    /// again every few seconds while a PRs or Files tab stays open.
    fn keep_current(&mut self, mono: &Mono, cx: &mut Context<Self>) {
        let (stale, refresh): (bool, Refresh) = match self.overview_tab {
            OverviewTab::Prs => (
                self.workstreams
                    .prs
                    .get(&mono.id)
                    .is_none_or(|l| l.stale(PRS_STALE)),
                Self::refresh_workstream_prs,
            ),
            OverviewTab::Files => (
                self.workstreams
                    .files
                    .get(&mono.id)
                    .is_none_or(|l| l.stale(FILES_STALE)),
                Self::refresh_workstream_files,
            ),
            _ => return,
        };
        if stale {
            refresh(self, mono, cx);
        }
        if self.workstreams.poll.is_none() {
            self.workstreams.poll = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(FILES_STALE).await;
                    let Ok(keep) = this.update(cx, |this, cx| {
                        let shown = this.current().filter(|m| m.is_workstream()).is_some()
                            && matches!(this.overview_tab, OverviewTab::Prs | OverviewTab::Files);
                        if shown {
                            // A render reads the tab again once it is stale.
                            cx.notify();
                        } else {
                            this.workstreams.poll = None;
                        }
                        shown
                    }) else {
                        return;
                    };
                    if !keep {
                        return;
                    }
                }
            }));
        }
    }

    /// The small row of actions above the PRs and Files lists.
    fn tab_action(
        &self,
        id: &'static str,
        label: &'static str,
        glyph: &'static str,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .debug_selector(move || id.into())
            .role(Role::Button)
            .aria_label(label)
            .flex()
            .items_center()
            .gap_1()
            .rounded_md()
            .px_1p5()
            .py_0p5()
            .text_size(px(11.))
            .text_color(content(0.55))
            .cursor_pointer()
            .hover(|s| s.bg(content(0.06)).text_color(content(1.)))
            .child(icon(glyph, 12.))
            .child(label)
    }

    /// WorkstreamPrs: the pull requests the agents opened, by where they stand.
    pub(super) fn workstream_prs(&mut self, mono: &Mono, cx: &mut Context<Self>) -> AnyElement {
        self.keep_current(mono, cx);
        let Some(workstream) = mono.workstream.clone() else {
            return div().into_any_element();
        };
        let monos = cx.weak_entity();
        let refresh = {
            let mono = mono.clone();
            self.tab_action("workstream-prs-refresh", "Refresh", "refresh-cw")
                .ml_auto()
                .on_click(move |_, _, cx| {
                    let mono = mono.clone();
                    let _ = monos.update(cx, |this, cx| this.refresh_workstream_prs(&mono, cx));
                })
        };
        let listing = self.workstreams.prs.get(&mono.id);
        let body: Vec<AnyElement> = match listing.and_then(|l| l.items.as_ref()) {
            None => Vec::new(),
            Some(Err(error)) => vec![
                div()
                    .id("workstream-prs-error")
                    .debug_selector(|| "workstream-prs-error".into())
                    .role(Role::Alert)
                    .px_1()
                    .text_size(px(12.))
                    .text_color(rgb(0xf87171))
                    .child(error.clone())
                    .into_any_element(),
            ],
            Some(Ok(prs)) if prs.is_empty() => vec![panels::empty_state(
                "git-pull-request",
                &format!(
                    "Each agent opens a pull request from its {}… branch when its part is done, and it shows up here for review.",
                    workstreams::branch_prefix(&workstream.slug)
                ),
            )],
            Some(Ok(prs)) => {
                let mut rows = Vec::new();
                for group in PrGroup::ALL {
                    let items: Vec<_> = prs
                        .iter()
                        .filter(|pr| workstreams::pr_group(pr) == group)
                        .collect();
                    if items.is_empty() {
                        continue;
                    }
                    rows.push(panels::list_heading(
                        format!("workstream-prs-{}", group.label()),
                        format!("{} · {}", group.label(), items.len()),
                    ));
                    rows.extend(items.into_iter().map(|pr| pr_row(pr, group)));
                }
                rows
            }
        };
        div()
            .id("workstream-prs")
            .debug_selector(|| "workstream-prs".into())
            .size_full()
            .overflow_y_scroll()
            .p_2()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().flex().child(refresh))
            .children(body)
            .into_any_element()
    }

    /// WorkstreamFiles: the shared folder's plan, notes and deliverables.
    pub(super) fn workstream_files(&mut self, mono: &Mono, cx: &mut Context<Self>) -> AnyElement {
        self.keep_current(mono, cx);
        let Some(workstream) = mono.workstream.clone() else {
            return div().into_any_element();
        };
        let monos = cx.weak_entity();
        let folder = workstream.folder.clone();
        let actions = div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                self.tab_action("workstream-files-reveal", "Show folder", "folder-open")
                    .on_click(move |_, _, cx| {
                        let folder = std::path::PathBuf::from(&folder);
                        cx.background_executor()
                            .spawn(async move {
                                let _ = std::fs::create_dir_all(&folder);
                                let _ = monocode_core::files::reveal_path(&folder);
                            })
                            .detach();
                    }),
            )
            .child({
                let mono = mono.clone();
                self.tab_action("workstream-files-refresh", "Refresh", "refresh-cw")
                    .ml_auto()
                    .on_click(move |_, _, cx| {
                        let mono = mono.clone();
                        let _ =
                            monos.update(cx, |this, cx| this.refresh_workstream_files(&mono, cx));
                    })
            });
        let files = match self
            .workstreams
            .files
            .get(&mono.id)
            .and_then(|l| l.items.as_ref())
        {
            Some(Ok(files)) => Some(files.clone()),
            _ => None,
        };
        let empty = files.as_ref().is_some_and(Vec::is_empty);
        div()
            .id("workstream-files")
            .debug_selector(|| "workstream-files".into())
            .size_full()
            .overflow_y_scroll()
            .p_2()
            .flex()
            .flex_col()
            .gap_1()
            .child(actions)
            .when(empty, |d| {
                d.child(panels::empty_state(
                    "file",
                    "The orchestrator and its agents keep the plan, notes and findings here. Code ships as pull requests.",
                ))
            })
            .children(files.into_iter().flatten().enumerate().map(|(ix, file)| {
                let selector = format!("workstream-file-{ix}");
                let path = file.path.clone();
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .role(Role::Button)
                    .aria_label(format!("Open {}", file.name))
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_md()
                    .px_2()
                    .py_1p5()
                    .cursor_pointer()
                    .hover(|s| s.bg(content(0.06)))
                    .child(crate::file_icons::file_type_icon(&file.name, false, false, 14.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .child(file.name),
                    )
                    .on_click(move |_, _, cx| cx.open_with_system(std::path::Path::new(&path)))
            }))
            .into_any_element()
    }
}

/// One pull request: its state's glyph, number and title, and the branch it
/// ships from. Opens on GitHub.
fn pr_row(pr: &BranchPr, group: PrGroup) -> AnyElement {
    let (glyph, ink) = match group {
        PrGroup::Open => ("git-pull-request", accent_color()),
        PrGroup::Draft => ("git-pull-request-draft", content(0.5)),
        PrGroup::Merged => ("git-merge", content(0.7)),
        PrGroup::Closed => ("git-pull-request-closed", content(0.4)),
    };
    let url = pr.url.clone();
    let number = pr.number;
    panels::outlined(
        div()
            .id(("workstream-pr", number as usize))
            .debug_selector(move || format!("workstream-pr-{number}"))
            .role(Role::Link)
            .aria_label(format!("#{number} {}, {}", pr.title, group.label())),
    )
    .flex_shrink_0()
    .px(px(10.))
    .py_2()
    .flex()
    .items_center()
    .gap_2p5()
    .cursor_pointer()
    .child(icon(glyph, 16.).text_color(ink))
    .child(
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .gap_1p5()
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_color(content(0.5))
                            .child(format!("#{number}")),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(pr.title.clone()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap_1()
                    .text_size(px(11.))
                    .line_height(px(16.))
                    .text_color(content(0.45))
                    .child(icon("git-branch", 11.).text_color(content(0.4)))
                    .child(div().min_w_0().truncate().child(pr.head_ref_name.clone())),
            ),
    )
    .on_click(move |_, _, cx| cx.open_url(&url))
    .into_any_element()
}
