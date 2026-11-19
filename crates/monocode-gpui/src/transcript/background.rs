//! LiveFoldTitle's "running in background", in the chat: once the agent has
//! answered with commands or subagents still going, a card under its reply
//! lists them until they finish or the agent picks the thread back up. A
//! command whose log the provider keeps shows its latest line, and opens to
//! its last lines like a terminal (T3 Code's command output panel).
use super::*;
use gpui::AnyElement;
use monocode_core::provider::{BackgroundTask, Kind};

const ROW_HEIGHT: f32 = 38.;
/// More rows than this scroll inside the card.
const VISIBLE_ROWS: f32 = 4.;
const LIVE: u32 = 0x4ade80;
const OUTPUT_LINE: f32 = 16.;
/// The card's row key: after every turn, before nothing.
pub(super) const BACKGROUND_START: usize = usize::MAX - 2;

#[derive(Default)]
pub(super) struct Background {
    pub tasks: Vec<BackgroundTask>,
    pub collapsed: bool,
    /// Commands showing their output.
    pub open: HashSet<String>,
}

impl TranscriptView {
    #[cfg(test)]
    pub(crate) fn background_count(&self) -> usize {
        self.background.tasks.len()
    }
    pub(crate) fn set_background(&mut self, tasks: Vec<BackgroundTask>, cx: &mut Context<Self>) {
        if self.background.tasks == tasks {
            return;
        }
        let same_tasks = self.background.tasks.len() == tasks.len()
            && self.background.tasks.iter().zip(&tasks).all(|(old, new)| {
                (&old.id, &old.description, &old.command)
                    == (&new.id, &new.description, &new.command)
            });
        self.background
            .open
            .retain(|id| tasks.iter().any(|task| &task.id == id));
        if same_tasks {
            // Only their logs moved: repaint the card, not the turn.
            self.background.tasks = tasks;
            self.remeasure_background(cx);
            return;
        }
        let appeared = self.background.tasks.is_empty() && !tasks.is_empty();
        self.background.tasks = tasks;
        if self.source.is_empty() {
            return;
        }
        let from = self.source.iter().rposition(Row::is_prompt).unwrap_or(0);
        self.rebuild(from, cx);
        if appeared && self.list.is_following_tail() {
            self.list.scroll_to_end();
        }
        cx.notify();
    }

    fn remeasure_background(&mut self, cx: &mut Context<Self>) {
        if let Some(ix) = self
            .rows
            .iter()
            .position(|row| matches!(row.body, Body::Background))
        {
            self.list.remeasure_items(ix..ix + 1);
            if self.list.is_following_tail() {
                self.list.scroll_to_end();
            }
        }
        cx.notify();
    }

    fn toggle_output(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.background.open.remove(id) {
            self.background.open.insert(id.to_owned());
        }
        self.remeasure_background(cx);
    }

    /// The card goes last, before a saved draft.
    pub(super) fn push_background(&mut self) {
        if self.background.tasks.is_empty()
            || self.source.is_empty()
            || self
                .rows
                .iter()
                .any(|row| matches!(row.body, Body::Background))
        {
            return;
        }
        let index = self
            .rows
            .iter()
            .position(|row| matches!(row.body, Body::SavedDraft))
            .unwrap_or(self.rows.len());
        self.rows.insert(
            index,
            UiRow {
                key: (BACKGROUND_START, 0),
                body: Body::Background,
                message: usize::MAX,
                first: false,
                last: false,
                work: false,
                work_tail: false,
                thought_phase: None,
            },
        );
    }

    pub(super) fn background_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let tasks = &self.background.tasks;
        if tasks.is_empty() {
            return div().into_any_element();
        }
        let count = tasks.len();
        // A lone task folds into the header: its title takes the subtitle's
        // place and its clock sits at the end, so the card stays one row.
        let single = (count == 1).then(|| &tasks[0]);
        let single_output = single.filter(|task| !task.output.is_empty());
        let single_open = single_output.is_some_and(|task| self.background.open.contains(&task.id));
        let collapsible = count > 1;
        let collapsed = collapsible && self.background.collapsed;
        let title = if count == 1 {
            "Running in background".to_owned()
        } else {
            format!("{count} tasks running in background")
        };
        let agent = Kind::from_id(&self.provider).label();
        let header = div()
            .id("background-tasks-header")
            .debug_selector(|| "background-tasks-header".into())
            .role(Role::Button)
            .aria_label(title.clone())
            .when(collapsible, |d| d.aria_expanded(!collapsed))
            .when(single_output.is_some(), |d| d.aria_expanded(single_open))
            .h(px(36.))
            .px_3()
            .flex()
            .items_center()
            .gap(px(8.))
            .when(collapsible, |d| {
                d.cursor_pointer().on_click(cx.listener(|this, _, _, cx| {
                    this.background.collapsed = !this.background.collapsed;
                    this.list.pause_following_tail();
                    let from = this.source.iter().rposition(Row::is_prompt).unwrap_or(0);
                    this.rebuild(from, cx);
                    cx.notify();
                }))
            })
            .when_some(single_output, |d, task| {
                let id = task.id.clone();
                d.cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_output(&id, cx)))
            })
            .child(
                div()
                    .size(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(if cx.reduce_motion() {
                        crate::session_mark::spinner_at(0.3).into_any_element()
                    } else {
                        crate::session_mark::spinner("background-tasks-spinner").into_any_element()
                    }),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(content(0.8))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(title),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(content(0.35))
                    .child(if let Some(task) = single {
                        task.description.clone()
                    } else if collapsed {
                        tasks
                            .iter()
                            .map(|task| task.description.as_str())
                            .collect::<Vec<_>>()
                            .join(" · ")
                    } else {
                        format!("{agent} continues when they finish")
                    }),
            )
            .when_some(single, |d, task| d.child(live_clock(task)))
            .when(single_output.is_some(), |d| {
                d.child(disclosure(single_open))
            })
            .when(collapsible, |d| {
                d.child(
                    div()
                        .size(px(20.))
                        .flex_shrink_0()
                        .rounded_md()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(content(0.45))
                        .hover(|s| s.bg(content(0.08)).text_color(content(0.9)))
                        .child(icon(
                            if collapsed {
                                "chevron-right"
                            } else {
                                "chevron-down-2"
                            },
                            12.,
                        )),
                )
            });
        let any_open = tasks
            .iter()
            .any(|task| self.background.open.contains(&task.id));
        let rows = div()
            .id("background-tasks-rows")
            .max_h(px(ROW_HEIGHT * VISIBLE_ROWS
                + if any_open {
                    output_height(monocode_core::provider::OUTPUT_LINES)
                } else {
                    0.
                }))
            .overflow_y_scroll()
            .px_3()
            .children(tasks.iter().map(|task| {
                let open = self.background.open.contains(&task.id);
                let row = background_task(task, open);
                if task.output.is_empty() {
                    return row;
                }
                let id = task.id.clone();
                row.on_click(cx.listener(move |this, _, _, cx| this.toggle_output(&id, cx)))
            }));
        // The same card as the files a turn changed (`calm.rs`).
        let card = div()
            .id("background-tasks-card")
            .role(Role::Group)
            .aria_label("Background tasks")
            .w_full()
            .overflow_hidden()
            .rounded(px(12.))
            .border_1()
            .border_color(content(0.06))
            .bg(content(0.03))
            .text_size(px(13.))
            .line_height(px(18.))
            .child(header)
            .when_some(single_output.filter(|_| single_open), |d, task| {
                d.child(div().px_3().child(output_box(task)))
            })
            .when(!collapsed && single.is_none(), |d| d.child(rows));
        div()
            .w_full()
            .flex()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(COLUMN))
                    .px(px(crate::theme::COLUMN_PAD))
                    .pt_2()
                    .pb_1()
                    .child(card),
            )
            .into_any_element()
    }
}

fn background_task(task: &BackgroundTask, open: bool) -> gpui::Stateful<gpui::Div> {
    let has_output = !task.output.is_empty();
    // Closed, a command shows what it printed last, like a terminal's last line.
    let subtitle = if open {
        task.command
            .as_deref()
            .map(|command| format!("$ {}", first_line(command)))
    } else {
        task.output
            .last()
            .map(|line| line.trim().to_owned())
            .or_else(|| {
                task.command
                    .as_deref()
                    .map(|command| format!("$ {}", first_line(command)))
            })
    };
    let line = div()
        .min_h(px(ROW_HEIGHT))
        .flex()
        .items_center()
        .gap(px(10.))
        .child(
            div()
                .size(px(24.))
                .flex_shrink_0()
                .rounded(px(7.))
                .bg(content(0.07))
                .text_color(content(0.6))
                .flex()
                .items_center()
                .justify_center()
                .child(icon(if task.agent { "bot" } else { "terminal" }, 13.)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .text_color(content(0.8))
                        .child(task.description.clone()),
                )
                .when_some(subtitle, |d, subtitle| {
                    d.child(
                        div()
                            .truncate()
                            .font_family(crate::fonts::mono())
                            .text_size(px(11.))
                            .text_color(content(0.4))
                            .child(subtitle),
                    )
                }),
        )
        .child(live_clock(task))
        .when(has_output, |d| d.child(disclosure(open)));
    div()
        .id(SharedString::from(format!("background-{}", task.id)))
        .debug_selector({
            let id = task.id.clone();
            move || format!("background-task-{id}")
        })
        .role(if has_output {
            Role::Button
        } else {
            Role::Group
        })
        .aria_label(format!("Background task: {}", task.description))
        .when(has_output, |d| {
            d.aria_expanded(open)
                .cursor_pointer()
                .hover(|s| s.bg(content(0.02)))
        })
        .border_t_1()
        .border_color(content(0.05))
        .child(line)
        .when(open && has_output, |d| d.child(output_box(task)))
}

/// The chevron of a command that opens to its output.
fn disclosure(open: bool) -> impl IntoElement {
    div()
        .size(px(20.))
        .flex_shrink_0()
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .text_color(content(0.45))
        .child(icon(
            if open {
                "chevron-down-2"
            } else {
                "chevron-right"
            },
            12.,
        ))
}

fn output_height(lines: usize) -> f32 {
    // The command line, the output lines and the box's padding and margin.
    (lines as f32 + 1.) * OUTPUT_LINE + 12. + 8.
}

/// The command and its last lines, as a terminal shows them.
fn output_box(task: &BackgroundTask) -> impl IntoElement {
    let id = task.id.clone();
    div()
        .debug_selector(move || format!("background-output-{id}"))
        .mb_2()
        .px(px(10.))
        .py(px(6.))
        .rounded(px(8.))
        .bg(content(0.04))
        .font_family(crate::fonts::mono())
        .text_size(px(11.))
        .line_height(px(OUTPUT_LINE))
        .when_some(task.command.as_deref(), |d, command| {
            d.child(
                div()
                    .truncate()
                    .text_color(content(0.45))
                    .child(format!("$ {}", first_line(command))),
            )
        })
        .children(task.output.iter().map(|line| {
            div()
                .truncate()
                .text_color(content(0.72))
                .child(line.clone())
        }))
}

/// Elapsed time beside a breathing green dot.
fn live_clock(task: &BackgroundTask) -> impl IntoElement {
    let started_at = task.started_at;
    crate::pulse::looping(
        SharedString::from(format!("background-clock-{}", task.id)),
        std::time::Duration::from_millis(2_000),
        crate::pulse::Rate::Low,
        move |phase| {
            let ms = monocode_core::time::now_ms().saturating_sub(started_at);
            // A slow breath: the dot swells and its halo fades out.
            let breath = 0.5 - 0.5 * (phase * std::f32::consts::TAU).cos();
            let live: gpui::Hsla = rgb(LIVE).into();
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(content(0.45))
                        .child(elapsed(ms)),
                )
                .child(
                    div()
                        .size(px(12.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(live.opacity(0.25 * (1. - breath)))
                        .child(
                            div()
                                .size(px(6.))
                                .rounded_full()
                                .bg(live.opacity(0.65 + 0.35 * breath)),
                        ),
                )
        },
    )
}

fn first_line(command: &str) -> &str {
    command
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(command)
        .trim()
}

/// A ticking clock, so seconds stay padded once minutes appear.
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

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn task(id: &str, agent: bool) -> BackgroundTask {
        BackgroundTask {
            id: id.into(),
            description: format!("Task {id}"),
            agent,
            command: (!agent).then(|| "npm run dev".into()),
            started_at: monocode_core::time::now_ms(),
            output: Vec::new(),
        }
    }

    #[gpui::test]
    fn background_work_sits_in_the_chat_under_the_reply_and_folds(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = TranscriptView::new();
                    let mut transcript = monocode_core::transcript::Transcript::default();
                    transcript.begin("user");
                    transcript.append("Start the server and review the diff");
                    transcript.begin("assistant");
                    transcript.append("Both are running.");
                    view.patch(0, transcript.rows, cx);
                    view.set_background(vec![task("dev", false), task("review", true)], cx);
                    view
                })
            })
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        window
            .update(visual, |view, _, _| {
                assert!(
                    matches!(view.rows.last().unwrap().body, Body::Background),
                    "the card is the chat's last row"
                );
            })
            .unwrap();
        assert!(visual.debug_bounds("background-task-dev").is_some());
        assert!(visual.debug_bounds("background-task-review").is_some());

        let header = visual.debug_bounds("background-tasks-header").unwrap();
        visual.simulate_click(header.center(), Default::default());
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("background-task-dev").is_none());
        assert!(visual.debug_bounds("background-tasks-header").is_some());

        window
            .update(visual, |view, _, cx| view.set_background(Vec::new(), cx))
            .unwrap();
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("background-tasks-header").is_none());
        window
            .update(visual, |view, _, _| {
                assert!(
                    !view
                        .rows
                        .iter()
                        .any(|row| matches!(row.body, Body::Background))
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn a_lone_task_folds_into_the_header(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = TranscriptView::new();
                    let mut transcript = monocode_core::transcript::Transcript::default();
                    transcript.begin("user");
                    transcript.append("Run the dev server");
                    view.patch(0, transcript.rows, cx);
                    view.set_background(vec![task("dev", false)], cx);
                    view
                })
            })
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("background-tasks-header").is_some());
        assert!(visual.debug_bounds("background-task-dev").is_none());
    }

    fn printing(id: &str) -> BackgroundTask {
        BackgroundTask {
            command: Some("cargo clippy --workspace".into()),
            output: vec!["Checking aitrack-ai".into(), "Checking aitrack-app".into()],
            ..task(id, false)
        }
    }

    fn open_card(
        cx: &mut TestAppContext,
        tasks: Vec<BackgroundTask>,
    ) -> gpui::WindowHandle<TranscriptView> {
        cx.update(|cx| cx.set_reduce_motion(true));
        cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = TranscriptView::new();
                    let mut transcript = monocode_core::transcript::Transcript::default();
                    transcript.begin("user");
                    transcript.append("Run clippy and the dev server");
                    view.patch(0, transcript.rows, cx);
                    view.set_background(tasks, cx);
                    view
                })
            })
            .unwrap()
        })
    }

    #[gpui::test]
    fn a_command_opens_to_its_output_and_follows_its_log(cx: &mut TestAppContext) {
        let window = open_card(cx, vec![printing("clippy"), task("dev", false)]);
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("background-output-clippy").is_none());
        let rebuilds = window
            .update(visual, |view, _, _| view.rebuild_passes)
            .unwrap();

        let row = visual.debug_bounds("background-task-clippy").unwrap();
        visual.simulate_click(row.center(), Default::default());
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("background-output-clippy").is_some());
        assert!(
            visual.debug_bounds("background-output-dev").is_none(),
            "a command without a log has nothing to open"
        );

        // The log grows: the card repaints without rebuilding the turn.
        let mut grown = printing("clippy");
        grown.output.push("Finished `dev` profile".into());
        window
            .update(visual, |view, _, cx| {
                view.set_background(vec![grown, task("dev", false)], cx)
            })
            .unwrap();
        visual.refresh().unwrap();
        window
            .update(visual, |view, _, _| {
                assert_eq!(view.rebuild_passes, rebuilds);
                assert!(view.background.open.contains("clippy"));
            })
            .unwrap();
        assert!(visual.debug_bounds("background-output-clippy").is_some());

        let row = visual.debug_bounds("background-task-clippy").unwrap();
        visual.simulate_click(row.center(), Default::default());
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("background-output-clippy").is_none());

        // A finished command forgets that it was open.
        window
            .update(visual, |view, _, cx| {
                view.background.open.insert("clippy".into());
                view.set_background(vec![task("dev", false)], cx);
                assert!(view.background.open.is_empty());
            })
            .unwrap();
    }

    #[gpui::test]
    fn a_lone_command_opens_from_the_header(cx: &mut TestAppContext) {
        let window = open_card(cx, vec![printing("clippy")]);
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("background-output-clippy").is_none());
        let header = visual.debug_bounds("background-tasks-header").unwrap();
        visual.simulate_click(header.center(), Default::default());
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("background-output-clippy").is_some());
    }

    #[test]
    fn elapsed_reads_as_a_running_clock() {
        assert_eq!(elapsed(4_200), "4s");
        assert_eq!(elapsed(72_000), "1m 12s");
        assert_eq!(elapsed(3_660_000), "1h 01m");
        assert_eq!(first_line("\n  npm run dev\nsecond"), "npm run dev");
    }
}
