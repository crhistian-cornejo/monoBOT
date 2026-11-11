//! The quieter transcript: which steps a reply shows (Settings › Chat ›
//! Tool visibility), the last thought under a running phase's live line,
//! the files a settled turn changed and the header that gathers a turn's
//! subagents under their faces.
use super::*;
use crate::right_panel::tabular;
use gpui::AnyElement;
use monocode_core::tool::summary::{FileChange, changed_files};

/// Settings › Chat › Tool visibility.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Visibility {
    #[default]
    All,
    /// Only the steps that went wrong or still run.
    Failures,
    /// No steps; thoughts and prose stay.
    None,
    /// Prompts and answers alone.
    Zen,
}
impl Visibility {
    pub fn parse(value: &str) -> Self {
        match value {
            "failures" => Self::Failures,
            "none" => Self::None,
            "zen" => Self::Zen,
            _ => Self::All,
        }
    }
    /// Whether a step in `state` gets a row.
    pub fn shows_step(self, state: &ToolState) -> bool {
        match self {
            Self::All => true,
            Self::Failures => *state != ToolState::Completed,
            Self::None | Self::Zen => false,
        }
    }
    /// Whether a group of `calls` keeps its header. A group of thoughts
    /// alone keeps it until Zen.
    pub fn shows_group(self, calls: &[ToolCall]) -> bool {
        match self {
            Self::All => true,
            _ if calls.is_empty() => self != Self::Zen,
            Self::Failures => calls.iter().any(|call| call.state != ToolState::Completed),
            Self::None | Self::Zen => false,
        }
    }
    /// Whether a thought outside any group is drawn.
    pub fn shows_thoughts(self) -> bool {
        self != Self::Zen
    }
}

/// The last line of the newest thought in `rows`, as plain words, for the
/// line under a running phase's step.
pub(super) fn last_thought(rows: &[Row]) -> Option<SharedString> {
    let at = rows
        .iter()
        .rposition(|row| row.role == "reasoning" && row.first)?;
    let end = (at + 1..rows.len())
        .find(|&i| rows[i].first)
        .unwrap_or(rows.len());
    let text: String = rows[at..end].iter().map(|row| row.text.as_str()).collect();
    let line = text
        .lines()
        .map(|line| line.trim().trim_matches(['*', '_', '#', '`', '>', ' ']))
        .rfind(|line| !line.is_empty())?;
    const MAX: usize = 160;
    let mut line = line.to_owned();
    if line.chars().count() > MAX {
        line = line.chars().take(MAX).collect::<String>() + "…";
    }
    Some(line.into())
}

/// The last row of the group `start..=last` that `visibility` still draws:
/// a thought, or a step it shows.
pub(super) fn visible_tail(
    rows: &[Row],
    start: usize,
    last: usize,
    visibility: Visibility,
) -> usize {
    (start..=last.min(rows.len().saturating_sub(1)))
        .rev()
        .find(|&at| {
            let row = &rows[at];
            row.first
                && match row.role.as_str() {
                    "reasoning" => !row.text.trim().is_empty(),
                    "tool" => serde_json::from_str::<ToolCall>(&row.text)
                        .is_ok_and(|call| visibility.shows_step(&call.state)),
                    _ => false,
                }
        })
        .unwrap_or(last)
}

/// The files a settled turn changed, as its card lists them.
pub(super) struct TurnChanges {
    pub files: Vec<FileChange>,
    pub additions: usize,
    pub deletions: usize,
}

/// What the steps in `rows` changed, or nothing when they changed no file.
pub(super) fn turn_changes(rows: &[Row]) -> Option<Arc<TurnChanges>> {
    let calls: Vec<ToolCall> = rows
        .iter()
        .filter(|row| row.role == "tool" && row.first)
        .filter_map(|row| serde_json::from_str(&row.text).ok())
        .collect();
    let files = changed_files(&calls);
    (!files.is_empty()).then(|| {
        Arc::new(TurnChanges {
            additions: files.iter().map(|f| f.additions).sum(),
            deletions: files.iter().map(|f| f.deletions).sum(),
            files,
        })
    })
}

/// A turn's running subagents, gathered under one header.
pub(super) struct Subagents {
    /// The first run's row; the group is known by it.
    pub first: usize,
    pub faces: Vec<(String, ToolState)>,
}
impl Subagents {
    fn label(&self) -> String {
        let count =
            |state: fn(&ToolState) -> bool| self.faces.iter().filter(|(_, s)| state(s)).count();
        let working = count(|s| *s == ToolState::Running);
        let done = count(|s| *s == ToolState::Completed);
        let failed = count(|s| matches!(s, ToolState::Failed | ToolState::Cancelled));
        let mut parts = vec![format!("{} subagents", self.faces.len())];
        if working > 0 {
            parts.push(format!("{working} working"));
        }
        if done > 0 {
            parts.push(format!("{done} done"));
        }
        if failed > 0 {
            parts.push(format!("{failed} failed"));
        }
        parts.join(" · ")
    }
}

/// The files shown before a "more" line.
const FILES: usize = 5;
/// How far each face tucks under the one before it.
const FACE_OVERLAP: f32 = 5.;

impl TranscriptView {
    /// The files the settled turn `turn..turn_end` changed, read once per
    /// version of the turn.
    pub(super) fn turn_changes(
        &mut self,
        turn: usize,
        turn_end: usize,
    ) -> Option<Arc<TurnChanges>> {
        let rows = &self.source[turn..turn_end];
        let length: usize = rows.iter().map(|row| row.text.len()).sum();
        if let Some((end, len, changes)) = self.turn_changes.get(&turn)
            && *end == turn_end
            && *len == length
        {
            return changes.clone();
        }
        let changes = turn_changes(rows);
        self.turn_changes
            .insert(turn, (turn_end, length, changes.clone()));
        changes
    }

    /// A path as the session's diff knows it: relative to the checkout.
    fn session_path(&self, path: &str) -> String {
        self.file_link_base
            .as_deref()
            .and_then(|base| std::path::Path::new(path).strip_prefix(base).ok())
            .map_or_else(|| path.to_owned(), |p| p.to_string_lossy().into_owned())
    }

    /// "3 files changed +12 −3" under a settled reply, each file opening
    /// its part of the session's diff.
    pub(super) fn changes_card(
        &mut self,
        changes: &TurnChanges,
        message_start: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let count = changes.files.len();
        let title = git_turns::plural(count, "file changed", "files changed");
        let hidden = count.saturating_sub(FILES);
        let rows: Vec<_> = changes
            .files
            .iter()
            .take(FILES)
            .enumerate()
            .map(|(ix, file)| {
                let shown = self.session_path(&file.path);
                let name = std::path::Path::new(&file.path)
                    .file_name()
                    .map_or_else(|| file.path.clone(), |n| n.to_string_lossy().into_owned());
                let open = shown.clone();
                div()
                    .id(("turn-change", message_start * 64 + ix))
                    .role(Role::Link)
                    .aria_label(SharedString::from(format!("Open the changes to {shown}")))
                    .h(px(28.))
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .cursor_pointer()
                    .text_color(content(0.65))
                    .hover(|s| s.bg(content(0.05)).text_color(content(1.)))
                    .child(crate::file_icons::file_type_icon(&name, false, false, 14.))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .font_family(crate::fonts::mono())
                            .text_size(px(12.))
                            .child(shown),
                    )
                    .child(git_turns::counts(
                        file.additions as u64,
                        file.deletions as u64,
                    ))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(ResponseRequest::OpenChanges {
                            path: Some(open.clone()),
                        });
                    }))
            })
            .collect();
        let header = div()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .h(px(36.))
            .child(
                icon("file-diff", 14.)
                    .flex_shrink_0()
                    .text_color(content(0.55)),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_size(px(12.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(content(0.8))
                    .child(title.clone()),
            )
            .child(git_turns::counts(
                changes.additions as u64,
                changes.deletions as u64,
            ))
            .child(
                div()
                    .id(("turn-changes-open", message_start))
                    .debug_selector(move || format!("turn-changes-open-{message_start}"))
                    .role(Role::Button)
                    .aria_label("Open the diff")
                    .flex_shrink_0()
                    .h(px(24.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .bg(content(0.06))
                    .text_size(px(11.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(content(0.7))
                    .cursor_pointer()
                    .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
                    .child("Open diff")
                    .on_click(cx.listener(|_, _, _, cx| {
                        cx.emit(ResponseRequest::OpenChanges { path: None });
                    })),
            );
        div()
            .w_full()
            .pt_2()
            .pb_1()
            .child(
                div()
                    .id(("turn-changes", message_start))
                    .debug_selector(move || format!("turn-changes-{message_start}"))
                    .role(Role::Group)
                    .aria_label(SharedString::from(title))
                    .w_full()
                    .overflow_hidden()
                    .rounded(px(12.))
                    .border_1()
                    .border_color(content(0.06))
                    .bg(content(0.03))
                    .child(header)
                    .child(
                        div()
                            .border_t_1()
                            .border_color(content(0.05))
                            .py_1()
                            .children(rows)
                            .when(hidden > 0, |d| {
                                d.child(
                                    div()
                                        .h(px(24.))
                                        .px_3()
                                        .flex()
                                        .items_center()
                                        .text_size(px(11.))
                                        .font_features(tabular())
                                        .text_color(content(0.4))
                                        .child(format!(
                                            "+{} more",
                                            git_turns::plural(hidden, "file", "files")
                                        )),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }

    /// "3 subagents · 2 working · 1 done" beside their faces; a click shows
    /// or hides their cards.
    pub(super) fn subagents_header(
        &mut self,
        group: &Subagents,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let first = group.first;
        let open = !self.folded_subagents(first);
        let label = group.label();
        let working = group
            .faces
            .iter()
            .any(|(_, state)| *state == ToolState::Running);
        let faces = group.faces.iter().enumerate().map(|(ix, (name, state))| {
            div()
                .when(ix > 0, |d| d.ml(px(-FACE_OVERLAP)))
                .rounded_full()
                .child(super::mascot::subagent_mascot(
                    name,
                    state,
                    *state == ToolState::Running && !cx.reduce_motion(),
                ))
        });
        div()
            .id(("subagents", first))
            .debug_selector(move || format!("subagents-{first}"))
            .role(Role::Button)
            .aria_label(SharedString::from(format!(
                "{} the cards for {label}",
                if open { "Hide" } else { "Show" }
            )))
            .aria_expanded(open)
            .flex()
            .items_center()
            .gap(px(8.))
            .py_1()
            .text_size(px(14.))
            .line_height(px(20.))
            .text_color(content(0.5))
            .cursor_pointer()
            .hover(|s| s.text_color(content(0.8)))
            .child(div().flex().items_center().flex_shrink_0().children(faces))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .child(if working && !cx.reduce_motion() {
                        crate::activity::shimmer(
                            label,
                            self.started.map_or(0., |s| s.elapsed().as_secs_f32()),
                        )
                    } else {
                        div().child(label).into_any_element()
                    }),
            )
            .child(
                icon(
                    if open {
                        "chevron-down"
                    } else {
                        "chevron-right"
                    },
                    14.,
                )
                .flex_shrink_0()
                .text_color(content(0.4)),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                let folded = this.folded_subagents(first);
                this.subagent_folds.insert(first, !folded);
                this.list.pause_following_tail();
                this.rebuild(first, cx);
            }))
            .into_any_element()
    }

    /// Calm work keeps a turn's subagents folded under their header until
    /// it is clicked open.
    pub(super) fn folded_subagents(&self, first: usize) -> bool {
        self.subagent_folds
            .get(&first)
            .copied()
            .unwrap_or(self.calm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(role: &str, text: &str) -> Row {
        Row {
            role: role.into(),
            text: text.into(),
            first: true,
            sent_at: None,
        }
    }

    #[test]
    fn the_live_line_shows_the_newest_thoughts_last_words() {
        let rows = [
            row("reasoning", "Old thought"),
            row("tool", "{}"),
            row(
                "reasoning",
                "**Checking paths**\n\nThe notch reads the hook.\n",
            ),
        ];
        assert_eq!(
            last_thought(&rows).as_deref(),
            Some("The notch reads the hook.")
        );
        assert_eq!(last_thought(&[row("tool", "{}")]), None);
    }

    #[test]
    fn visibility_hides_what_went_well_or_everything() {
        let ok = ToolCall {
            state: ToolState::Completed,
            ..ToolCall::new("1", "Bash")
        };
        let failed = ToolCall {
            state: ToolState::Failed,
            ..ToolCall::new("2", "Bash")
        };
        let failures = Visibility::parse("failures");
        assert!(!failures.shows_step(&ok.state) && failures.shows_step(&failed.state));
        assert!(!failures.shows_group(std::slice::from_ref(&ok)));
        assert!(failures.shows_group(&[ok.clone(), failed.clone()]));
        assert!(!Visibility::parse("none").shows_group(&[failed]));
        assert!(Visibility::parse("none").shows_group(&[]));
        assert!(!Visibility::Zen.shows_group(&[]) && !Visibility::Zen.shows_thoughts());
        assert_eq!(Visibility::parse("bogus"), Visibility::All);
    }

    #[test]
    fn a_settled_turn_lists_the_files_its_steps_changed() {
        let mut edit = ToolCall::new("1", "Edit");
        edit.input = serde_json::json!({
            "file_path": "/repo/src/a.rs", "old_string": "a", "new_string": "b\nc"
        })
        .to_string();
        edit.state = ToolState::Completed;
        let rows = [
            row("user", "Fix it"),
            row("tool", &serde_json::to_string(&edit).unwrap()),
            row("assistant", "Done"),
        ];
        let changes = turn_changes(&rows).unwrap();
        assert_eq!(
            (changes.files.len(), changes.additions, changes.deletions),
            (1, 2, 1)
        );
        assert!(turn_changes(&rows[..1]).is_none());
    }

    fn tool(id: &str, name: &str, input: &str, state: ToolState) -> Row {
        let mut call = ToolCall::new(id, name);
        call.input = input.into();
        call.state = state;
        row("tool", &serde_json::to_string(&call).unwrap())
    }

    fn agent(id: &str, name: &str, state: ToolState) -> Row {
        let mut call = ToolCall::new(id, "Task");
        call.state = state;
        call.panel = Some(Panel::Agent(monocode_core::tool::tasks::AgentRun {
            name: Some(name.into()),
            ..Default::default()
        }));
        row("tool", &serde_json::to_string(&call).unwrap())
    }

    fn phases(view: &TranscriptView) -> Vec<(usize, String, Option<SharedString>)> {
        view.rows
            .iter()
            .filter_map(|row| match &row.body {
                Body::Phase {
                    title,
                    group,
                    thought,
                    ..
                } => Some((*group, title.clone(), thought.clone())),
                _ => None,
            })
            .collect()
    }

    fn tools(view: &TranscriptView) -> Vec<usize> {
        view.rows
            .iter()
            .filter(|row| matches!(row.body, Body::Tool { .. }))
            .map(|row| row.key.0)
            .collect()
    }

    #[gpui::test]
    fn focused_calm_work_shows_the_step_at_work_and_its_newest_thought(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let view = cx.new(|_| TranscriptView::new());
        view.update(cx, |view, cx| {
            view.calm = true;
            // Unfolded, so the work before the prose would otherwise show.
            view.fold_work = false;
            view.streaming(true, cx);
            view.patch(
                0,
                vec![
                    row("user", "Fix it"),
                    tool(
                        "read",
                        "Read",
                        r#"{"file_path":"src/main.rs"}"#,
                        ToolState::Completed,
                    ),
                    row("assistant", "Now the library."),
                    row("reasoning", "**Plan**\n\nThe hook decides."),
                    tool(
                        "edit",
                        "Edit",
                        r#"{"file_path":"src/lib.rs"}"#,
                        ToolState::Running,
                    ),
                ],
                cx,
            );
            view.rebuild(0, cx);
            // The read is behind it; only the live step shows, with its thought.
            assert_eq!(
                phases(view),
                [(
                    3,
                    "Editing src/lib.rs".into(),
                    Some("The hook decides.".into())
                )]
            );
            // Classic keeps every running step in view, without the thought.
            view.set_focused(false, cx);
            let shown = phases(view);
            assert_eq!(shown.len(), 2);
            assert!(shown.iter().all(|(_, _, thought)| thought.is_none()));
        });
    }

    #[gpui::test]
    fn tool_visibility_leaves_out_steps_that_went_well(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let view = cx.new(|_| TranscriptView::new());
        view.update(cx, |view, cx| {
            view.fold_work = false;
            view.patch(
                0,
                vec![
                    row("user", "Test"),
                    tool("ok", "Bash", r#"{"command":"ls"}"#, ToolState::Completed),
                    tool(
                        "bad",
                        "Bash",
                        r#"{"command":"cargo test"}"#,
                        ToolState::Failed,
                    ),
                    tool("ok2", "Bash", r#"{"command":"pwd"}"#, ToolState::Completed),
                    row("assistant", "One test failed."),
                ],
                cx,
            );
            view.phases.insert(1, true);
            view.rebuild(0, cx);
            assert_eq!(tools(view), [1, 2, 3]);
            view.visibility = Visibility::Failures;
            view.rebuild(0, cx);
            assert_eq!(phases(view).len(), 1);
            assert_eq!(tools(view), [2]);
            // The rail ends at the failed step, the last one drawn.
            assert!(
                view.rows
                    .iter()
                    .any(|row| matches!(row.body, Body::Tool { tail: true, .. }) && row.key.0 == 2)
            );
            view.visibility = Visibility::None;
            view.rebuild(0, cx);
            assert!(phases(view).is_empty() && tools(view).is_empty());
        });
    }

    #[gpui::test]
    fn a_settled_reply_lists_its_changed_files_and_opens_the_diff(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = TranscriptView::new();
                    view.patch(
                        0,
                        vec![
                            row("user", "Fix it"),
                            tool(
                                "edit",
                                "Edit",
                                r#"{"file_path":"src/a.rs","old_string":"a","new_string":"b\nc"}"#,
                                ToolState::Completed,
                            ),
                            row("assistant", "Fixed."),
                        ],
                        cx,
                    );
                    view
                })
            })
            .unwrap()
        });
        let view = window.root(cx).unwrap();
        let requests = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let seen = requests.clone();
        cx.update(|cx| {
            cx.subscribe(&view, move |_, request: &ResponseRequest, _| {
                if let ResponseRequest::OpenChanges { path } = request {
                    seen.borrow_mut().push(path.clone());
                }
            })
            .detach()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let open = visual
            .debug_bounds("turn-changes-open-2")
            .expect("the card's Open diff");
        visual.simulate_click(open.center(), Default::default());
        assert_eq!(requests.borrow().as_slice(), &[None]);
    }

    #[gpui::test]
    fn running_subagents_share_a_header_that_opens_their_cards(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = TranscriptView::new();
                    view.calm = true;
                    view.streaming(true, cx);
                    view.patch(
                        0,
                        vec![
                            row("user", "Look around"),
                            agent("a", "Read the hooks", ToolState::Running),
                            agent("b", "Read the notch", ToolState::Completed),
                        ],
                        cx,
                    );
                    view
                })
            })
            .unwrap()
        });
        let view = window.root(cx).unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        view.update(visual, |view, _| {
            let header = view.rows.iter().find_map(|row| match &row.body {
                Body::Subagents(group) => Some(group.label()),
                _ => None,
            });
            assert_eq!(header.as_deref(), Some("2 subagents · 1 working · 1 done"));
            // Calm work keeps the cards folded under it.
            assert!(tools(view).is_empty());
        });
        let header = visual.debug_bounds("subagents-1").expect("the header");
        visual.simulate_click(header.center(), Default::default());
        view.update(visual, |view, _| assert_eq!(tools(view), [1, 2]));
    }
}
