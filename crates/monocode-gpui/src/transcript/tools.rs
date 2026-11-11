//! Compact semantic tool captions with clickable file targets.
use super::{TranscriptView, tasks};
use crate::theme::*;
use gpui::{AnyElement, Context, Div, Role, div, prelude::*, px, rgb};
use monocode_core::tool::edit_preview::EditPreview;
use monocode_core::tool::{CommandStep, ToolCall, ToolState};
use std::sync::Arc;

/// Shell plumbing that says nothing about what a chained command was for.
fn is_noise(step: &CommandStep) -> bool {
    step.action == "Run"
        && matches!(
            step.command.split_whitespace().next(),
            Some("cd" | "pwd" | "echo" | "printf" | "true" | "false" | "sleep" | "clear")
        )
}

/// `git status --short` reads as `git status`; flags and paths are detail.
fn command_label(command: &str) -> String {
    let mut words = command.split_whitespace();
    let program = words.next().unwrap_or("");
    let program = program.rsplit('/').next().unwrap_or(program);
    match words.next() {
        Some(sub)
            if !sub.starts_with('-')
                && !sub.contains(['/', '.', '=', '\'', '"'])
                && sub.len() <= 16 =>
        {
            format!("{program} {sub}")
        }
        _ => program.to_owned(),
    }
}

/// A shared fixed box keeps overflow counts aligned with the pattern pills.
fn search_pill() -> Div {
    div()
        .flex()
        .min_w_0()
        .h(px(24.))
        .items_center()
        .px_2()
        .border_1()
        .border_color(content(0.))
        .rounded(px(6.))
        .bg(content(0.06))
}

/// Lines an inline diff shows before pointing at the file.
const DIFF_LINES: usize = 80;

/// The edit's change under its row: what it is, its counts, then its lines.
fn diff_block(
    start: usize,
    preview: &EditPreview,
    state: &ToolState,
    path: Option<String>,
) -> gpui::Div {
    use crate::right_panel::{DIFF_ADD, DIFF_DEL};
    use monocode_core::diff::LineKind;
    let more = preview.lines.len().saturating_sub(DIFF_LINES) + preview.hidden;
    div()
        .debug_selector(move || format!("tool-diff-{start}"))
        .min_w_0()
        .mb_2()
        .rounded(px(8.))
        .border_1()
        .border_color(content(0.08))
        .overflow_hidden()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .px_2p5()
                .py_1()
                .border_b_1()
                .border_color(content(0.08))
                .text_size(px(11.))
                .text_color(content(0.5))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(preview.description(state)),
                )
                .when(preview.additions > 0, |d| {
                    d.child(
                        div()
                            .text_color(rgb(DIFF_ADD))
                            .child(format!("+{}", preview.additions)),
                    )
                })
                .when(preview.deletions > 0, |d| {
                    d.child(
                        div()
                            .text_color(rgb(DIFF_DEL))
                            .child(format!("−{}", preview.deletions)),
                    )
                }),
        )
        .child(
            div()
                .py_1()
                .font_family(crate::fonts::mono())
                .text_size(px(12.))
                .line_height(px(18.))
                .children(preview.lines.iter().take(DIFF_LINES).map(|line| {
                    let (mark, ink) = match line.kind {
                        LineKind::Add => ("+", Some(rgb(DIFF_ADD))),
                        LineKind::Del => ("−", Some(rgb(DIFF_DEL))),
                        LineKind::Hunk => ("", None),
                        LineKind::Context => (" ", None),
                    };
                    let hunk = line.kind == LineKind::Hunk;
                    div()
                        .flex()
                        .min_w_0()
                        .px_2()
                        .when_some(ink, |d, ink| d.bg(gpui::Hsla::from(ink).opacity(0.08)))
                        .child(
                            div()
                                .w(px(14.))
                                .flex_shrink_0()
                                .text_color(ink.map_or(content(0.3), Into::into))
                                .child(mark),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_color(if hunk { content(0.35) } else { content(0.75) })
                                .child(crate::diff_view::expand_tabs(&line.text).into_owned()),
                        )
                })),
        )
        .when(more > 0, |d| {
            d.child(
                div()
                    .id(("tool-diff-more", start))
                    .px_2p5()
                    .py_1()
                    .border_t_1()
                    .border_color(content(0.08))
                    .text_size(px(11.))
                    .text_color(content(0.5))
                    .child(format!(
                        "{more} more {}",
                        if more == 1 { "line" } else { "lines" }
                    ))
                    .when_some(path, |d, path| {
                        d.role(Role::Button)
                            .aria_label(format!("Open {path}"))
                            .cursor_pointer()
                            .hover(|s| s.text_color(content(0.8)))
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                window.dispatch_action(
                                    Box::new(crate::file_editor::OpenPath(path.clone())),
                                    cx,
                                );
                            })
                    }),
            )
        })
}

impl TranscriptView {
    /// The edit's diff, built once per version of the step.
    fn edit_preview(&mut self, key: &str, tool: &ToolCall) -> Option<Arc<EditPreview>> {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        tool.input.hash(&mut hasher);
        tool.output.len().hash(&mut hasher);
        tool.display
            .as_ref()
            .map(|d| d.to_string())
            .hash(&mut hasher);
        let version = hasher.finish();
        if let Some((built, preview)) = self.edit_previews.get(key)
            && *built == version
        {
            return preview.clone();
        }
        let preview = monocode_core::tool::edit_preview::edit_preview(tool).map(Arc::new);
        if self.edit_previews.len() > 64 {
            self.edit_previews.clear();
        }
        self.edit_previews
            .insert(key.to_owned(), (version, preview.clone()));
        preview
    }

    /// `tiled`: the row leads with its own tile, so the line drops the
    /// inline logo a monoBOT tool would otherwise carry.
    pub(super) fn tool(
        &mut self,
        tool: ToolCall,
        start: usize,
        tiled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.tool_line(tool, start, None, None, tiled, cx)
    }

    pub(super) fn tool_in(
        &mut self,
        tool: ToolCall,
        start: usize,
        scope: Option<&str>,
        width: Option<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.tool_line(tool, start, scope, width, false, cx)
    }

    fn tool_line(
        &mut self,
        tool: ToolCall,
        start: usize,
        scope: Option<&str>,
        width: Option<gpui::Pixels>,
        tiled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(panel) = tasks::render(self, &tool, start, width, cx) {
            return panel;
        }
        let identity = scope.map_or_else(|| start.to_string(), |scope| format!("{start}:{scope}"));
        if tool.is_milestone() {
            let running = tool.state == ToolState::Running;
            let failed = tool.state == ToolState::Failed;
            let label = tool.summary().0;
            return div()
                .id(gpui::SharedString::from(format!("tool:{identity}")))
                .debug_selector(move || format!("tool-{start}"))
                .role(Role::Group)
                .aria_label(label.clone())
                .flex()
                .w_full()
                .min_w_0()
                .items_center()
                .gap_2()
                .py_1()
                .line_height(px(20.))
                .text_color(if failed {
                    rgb(0xf87171).into()
                } else {
                    content(0.55)
                })
                .child(if running {
                    div()
                        .size(px(14.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(crate::session_mark::spinner(gpui::SharedString::from(
                            format!("compaction-loader:{identity}"),
                        )))
                        .into_any_element()
                } else {
                    let glyph = match tool.state {
                        ToolState::Completed => "check",
                        ToolState::Failed => "close",
                        _ => "minus",
                    };
                    icon(glyph, 14.)
                        .text_color(if failed {
                            rgb(0xf87171).into()
                        } else {
                            content(0.45)
                        })
                        .into_any_element()
                })
                .child(div().min_w_0().truncate().child(if running {
                    crate::activity::shimmer(label, self.started_at.elapsed().as_secs_f32())
                } else {
                    div().child(label).into_any_element()
                }))
                .into_any_element();
        }
        let shown = tool.presentation_in(self.file_link_base.as_deref());
        let visual = tool.visual_step().is_some();
        let failed = matches!(tool.state, ToolState::Failed | ToolState::Cancelled);
        let is_search =
            matches!(shown.action.as_str(), "Find" | "Search") && !shown.target.is_empty();
        let action = if is_search { "Search" } else { &shown.action };
        let is_file = matches!(shown.action.as_str(), "Read" | "Edit" | "Write" | "List");
        let target_path = if is_file {
            shown.file_path.as_deref().map(|path| {
                let (path, location) = crate::file_editor::file_location(
                    path,
                    self.file_link_base.as_deref().unwrap_or(""),
                );
                if location.is_none() {
                    shown
                        .start_line
                        .map_or(path.clone(), |line| format!("{path}:{line}"))
                } else {
                    location.map_or(path.clone(), |(line, col)| format!("{path}:{line}:{col}"))
                }
            })
        } else {
            None
        };
        let link_base = self.file_link_base.clone().unwrap_or_default();
        let link = |path: &str, line: Option<u64>| {
            let (path, location) = crate::file_editor::file_location(path, &link_base);
            match location {
                Some((line, col)) => format!("{path}:{line}:{col}"),
                None => line.map_or(path.clone(), |line| format!("{path}:{line}")),
            }
        };
        let chain = shown.action == "Run" && !shown.steps.is_empty();
        let caption = match (&shown.description, chain) {
            (Some(description), true) => description.clone(),
            _ => format!("{action} {}", shown.target),
        };
        let state_label = match tool.state {
            ToolState::Running => "Running",
            ToolState::Completed => "Completed",
            ToolState::Failed => "Failed",
            ToolState::Cancelled => "Cancelled",
        };
        // Chained commands and failures share one disclosure: steps, then error.
        let error_key = format!("tool-detail:{start}:{}", tool.id);
        let error_open = self.agent_panels.part_open(&error_key, false);
        let has_error = failed && !tool.output.trim().is_empty();
        let has_detail = has_error || chain;
        // ToolDiffPreview: an edit shows its own change, independent of git.
        let diff_key = format!("tool-diff:{start}:{}", tool.id);
        let can_diff = !has_detail
            && matches!(shown.action.as_str(), "Edit" | "Write")
            && monocode_core::tool::edit_preview::available(&tool);
        let diff_open = can_diff && self.agent_panels.part_open(&diff_key, false);
        let preview = diff_open
            .then(|| self.edit_preview(&diff_key, &tool))
            .flatten();
        let diff_state = tool.state.clone();
        let diff_path = target_path.clone();
        let chips = if chain {
            let mut seen = std::collections::HashSet::new();
            let chips: Vec<_> = shown
                .steps
                .iter()
                .filter(|step| !is_noise(step))
                .filter_map(|step| {
                    let (label, path) = match step.action {
                        "Read" | "List" => (
                            step.target
                                .rsplit(['/', '\\'])
                                .find(|part| !part.is_empty())
                                .unwrap_or(&step.target)
                                .to_owned(),
                            step.file_path
                                .as_deref()
                                .map(|path| link(path, step.start_line)),
                        ),
                        "Find" => (step.target.clone(), None),
                        _ => (command_label(&step.command), None),
                    };
                    seen.insert((step.action, label.clone()))
                        .then_some((step.action, label, path))
                })
                .collect();
            chips
        } else {
            Vec::new()
        };
        let steps = if chain {
            shown
                .steps
                .iter()
                .map(|step| {
                    let path = step
                        .file_path
                        .as_deref()
                        .map(|path| link(path, step.start_line));
                    (step.op, step.command.clone(), path)
                })
                .collect()
        } else {
            Vec::new()
        };
        let error_detail = tool.output.clone();
        let summary = div()
            .id(gpui::SharedString::from(format!("tool:{identity}")))
            .debug_selector(move || format!("tool-{start}"))
            .role(Role::Group)
            .aria_label(format!("{state_label} tool call: {caption}"))
            .flex()
            .w_full()
            .min_w_0()
            .items_center()
            .gap_1p5()
            .py_1()
            .line_height(px(20.))
            // React's activity rows are always bare: no status glyph, only
            // a failure stays marked. monoBOT's own tools carry its logo.
            .when(visual && !tiled, |d| {
                d.child(crate::activity::glyph("monocode", 14.))
            })
            .when(shown.action != "Run", |d| {
                d.child(
                    div()
                        .flex_shrink_0()
                        .text_color(if failed {
                            rgb(0xf87171).into()
                        } else {
                            content(0.5)
                        })
                        .child(action.to_owned()),
                )
            })
            .child(
                div().min_w_0().flex_1().flex().child(
                    div()
                        .id(gpui::SharedString::from(format!("tool-target:{identity}")))
                        .debug_selector(move || format!("tool-target-{start}"))
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap_1()
                        // A chain's description is prose; only its chips are
                        // code. A chart's title is prose too.
                        .when(!chain && !visual, |d| {
                            d.font_family(crate::fonts::mono()).text_size(px(12.))
                        })
                        .text_color(if failed {
                            rgb(0xff6467).into()
                        } else {
                            content(0.7)
                        })
                        .when(is_search, |d| {
                            let patterns = shown.search_patterns();
                            let hidden = patterns.len().saturating_sub(2);
                            d.children(patterns.iter().take(2).enumerate().map(
                                |(index, pattern)| {
                                    search_pill()
                                        .debug_selector(move || {
                                            format!("tool-search-pill-{start}-{index}")
                                        })
                                        .child(
                                            div().min_w_0().truncate().child((*pattern).to_owned()),
                                        )
                                },
                            ))
                            .when(hidden > 0, |d| {
                                d.child(
                                    search_pill()
                                        .id(gpui::SharedString::from(format!(
                                            "tool-search-more:{identity}"
                                        )))
                                        .debug_selector(move || format!("tool-search-more-{start}"))
                                        .flex_shrink_0()
                                        .bg(content(0.))
                                        .border_color(content(0.12))
                                        .text_color(if failed {
                                            rgb(0xff6467).into()
                                        } else {
                                            content(0.5)
                                        })
                                        .aria_label(format!(
                                            "{hidden} more patterns: {}",
                                            patterns[2..].join(", ")
                                        ))
                                        .child(format!("+{hidden}")),
                                )
                            })
                        })
                        .when(is_file, |d| {
                            d.rounded(px(4.)).px_1().bg(content(0.06)).child(
                                if shown.action == "List" {
                                    icon("folder", 16.).into_any_element()
                                } else {
                                    crate::activity::file_icon(&shown.target, 16.)
                                },
                            )
                        })
                        .when_some(target_path, |d, path| {
                            d.role(Role::Button)
                                .aria_label(format!("Open {path}"))
                                .cursor_pointer()
                                .hover(|style| style.text_color(rgb(0x7dd3fc)))
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    window.dispatch_action(
                                        Box::new(crate::file_editor::OpenPath(path.clone())),
                                        cx,
                                    );
                                })
                        })
                        .when(!is_search && !chain, |d| {
                            d.child(div().min_w_0().truncate().child(shown.target))
                        })
                        .when(chain, |d| {
                            let hidden = chips.len().saturating_sub(3);
                            let rest: Vec<_> = chips
                                .iter()
                                .skip(3)
                                .map(|(_, label, _)| label.clone())
                                .collect();
                            d.gap_1p5()
                                .when_some(shown.description.clone(), |d, description| {
                                    d.child(
                                        div()
                                            .min_w(px(64.))
                                            .flex_shrink(1.)
                                            .truncate()
                                            .child(description),
                                    )
                                })
                                .children(chips.into_iter().take(3).enumerate().map(
                                    |(index, (action, label, path))| {
                                        search_pill()
                                            .id(gpui::SharedString::from(format!(
                                                "tool-step-chip:{identity}:{index}"
                                            )))
                                            .debug_selector(move || {
                                                format!("tool-step-chip-{start}-{index}")
                                            })
                                            .max_w(px(200.))
                                            .flex_shrink(1.)
                                            .font_family(crate::fonts::mono())
                                            .text_size(px(12.))
                                            .gap_1()
                                            .px_1p5()
                                            .when(matches!(action, "Read" | "List"), |d| {
                                                d.child(if action == "List" {
                                                    icon("folder", 14.).into_any_element()
                                                } else {
                                                    crate::activity::file_icon(&label, 14.)
                                                })
                                            })
                                            .when(action == "Find", |d| {
                                                d.child(
                                                    icon("search", 12.)
                                                        .text_color(content(0.45))
                                                        .flex_shrink_0(),
                                                )
                                            })
                                            .child(div().min_w_0().truncate().child(label.clone()))
                                            .when_some(path, |d, path| {
                                                d.role(Role::Button)
                                                    .aria_label(format!("Open {path}"))
                                                    .cursor_pointer()
                                                    .hover(|style| style.text_color(rgb(0x7dd3fc)))
                                                    .on_click(move |_, window, cx| {
                                                        cx.stop_propagation();
                                                        window.dispatch_action(
                                                            Box::new(crate::file_editor::OpenPath(
                                                                path.clone(),
                                                            )),
                                                            cx,
                                                        );
                                                    })
                                            })
                                    },
                                ))
                                .when(hidden > 0, |d| {
                                    d.child(
                                        search_pill()
                                            .id(gpui::SharedString::from(format!(
                                                "tool-step-more:{identity}"
                                            )))
                                            .debug_selector(move || {
                                                format!("tool-step-more-{start}")
                                            })
                                            .flex_shrink_0()
                                            .font_family(crate::fonts::mono())
                                            .text_size(px(12.))
                                            .bg(content(0.))
                                            .border_color(content(0.12))
                                            .text_color(if failed {
                                                rgb(0xff6467).into()
                                            } else {
                                                content(0.5)
                                            })
                                            .aria_label(format!(
                                                "{hidden} more steps: {}",
                                                rest.join(", ")
                                            ))
                                            .child(format!("+{hidden}")),
                                    )
                                })
                        }),
                ),
            )
            .when(failed, |d| {
                d.child(icon("close", 14.).text_color(rgb(0xff6467)).flex_shrink_0())
            })
            .when(can_diff, |d| {
                let toggle_key = diff_key.clone();
                let label = format!(
                    "{} changes for {caption}",
                    if diff_open { "Hide" } else { "Show" }
                );
                d.child(
                    div()
                        .id("diff-toggle")
                        .debug_selector(move || format!("tool-diff-toggle-{start}"))
                        .role(Role::Button)
                        .aria_expanded(diff_open)
                        .aria_label(label.clone())
                        .tooltip(chrome_tip(label, None))
                        .cursor_pointer()
                        .p_1()
                        .child(
                            icon(
                                if diff_open {
                                    "chevron-down"
                                } else {
                                    "chevron-right"
                                },
                                14.,
                            )
                            .text_color(content(0.4)),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.toggle_agent_part(toggle_key.clone(), !diff_open, start, cx);
                        })),
                )
            })
            .when(has_detail, |d| {
                let toggle_key = error_key.clone();
                d.cursor_pointer()
                    .child(
                        div()
                            .id("error-toggle")
                            .role(Role::Button)
                            .aria_expanded(error_open)
                            .aria_label(format!(
                                "{} {} for {caption}",
                                if error_open { "Hide" } else { "Show" },
                                if has_error { "error details" } else { "steps" }
                            ))
                            .tooltip(chrome_tip(
                                format!(
                                    "{} {} for {caption}",
                                    if error_open { "Hide" } else { "Show" },
                                    if has_error { "error details" } else { "steps" }
                                ),
                                None,
                            ))
                            .p_1()
                            .child(
                                icon(
                                    if error_open {
                                        "chevron-down"
                                    } else {
                                        "chevron-right"
                                    },
                                    14.,
                                )
                                .text_color(if has_error {
                                    rgb(0xf87171).into()
                                } else {
                                    content(0.4)
                                }),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_agent_part(toggle_key.clone(), !error_open, start, cx);
                    }))
            });
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .child(summary)
            .when(chain && error_open, |d| {
                d.child(
                    div()
                        .debug_selector(move || format!("tool-steps-{start}"))
                        .min_w_0()
                        .pb_2()
                        .flex()
                        .flex_col()
                        .font_family(crate::fonts::mono())
                        .text_size(px(12.))
                        .line_height(px(20.))
                        .children(steps.into_iter().enumerate().map(
                            |(index, (op, command, path))| {
                                div()
                                    .id(gpui::SharedString::from(format!(
                                        "tool-step:{identity}:{index}"
                                    )))
                                    .flex()
                                    .min_w_0()
                                    .gap_2()
                                    .child(
                                        div()
                                            .w(px(16.))
                                            .flex_shrink_0()
                                            .flex()
                                            .justify_end()
                                            .text_color(content(0.3))
                                            .child(op.unwrap_or("")),
                                    )
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_color(content(0.6))
                                            .child(command),
                                    )
                                    .when_some(path, |d, path| {
                                        d.role(Role::Button)
                                            .aria_label(format!("Open {path}"))
                                            .cursor_pointer()
                                            .hover(|style| style.text_color(rgb(0x7dd3fc)))
                                            .on_click(move |_, window, cx| {
                                                cx.stop_propagation();
                                                window.dispatch_action(
                                                    Box::new(crate::file_editor::OpenPath(
                                                        path.clone(),
                                                    )),
                                                    cx,
                                                );
                                            })
                                    })
                            },
                        )),
                )
            })
            .when_some(preview, |d, preview| {
                d.child(diff_block(start, &preview, &diff_state, diff_path))
            })
            .when(has_error && error_open, |d| {
                d.child(
                    div()
                        .min_w_0()
                        .pb_2()
                        .font_family(crate::fonts::mono())
                        .text_size(px(12.))
                        .line_height(px(20.))
                        .text_color(rgb(0xf87171))
                        .child(error_detail),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::Body;
    use gpui::{TestAppContext, point};
    use monocode_core::transcript::Row;

    #[gpui::test]
    fn edits_open_their_own_diff_under_the_row(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|_| TranscriptView::new())
            })
            .unwrap()
        });
        window
            .update(cx, |view, _, cx| {
                let mut call = ToolCall::new("edit", "Edit");
                call.input = serde_json::json!({
                    "file_path": "/repo/src/lib.rs",
                    "old_string": "fn a() {}\n",
                    "new_string": "fn a() {\n    b();\n}\n",
                })
                .to_string();
                call.state = ToolState::Completed;
                view.patch(
                    0,
                    vec![
                        Row {
                            role: "user".into(),
                            text: "Edit it".into(),
                            first: true,
                            sent_at: None,
                        },
                        Row {
                            role: "tool".into(),
                            text: serde_json::to_string(&call).unwrap(),
                            first: true,
                            sent_at: None,
                        },
                    ],
                    cx,
                );
                view.work_folds.insert(1, true);
                view.phases.insert(1, true);
                view.rebuild(0, cx);
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        assert!(visual.debug_bounds("tool-diff-1").is_none());
        let toggle = visual
            .debug_bounds("tool-diff-toggle-1")
            .expect("an edit offers its diff");
        visual.simulate_click(toggle.center(), Default::default());
        visual.run_until_parked();
        let diff = visual
            .debug_bounds("tool-diff-1")
            .expect("the diff opens under the row");
        let row = visual.debug_bounds("tool-1").expect("the row stays");
        assert!(diff.top() >= row.bottom() - px(1.), "{diff:?} {row:?}");
        // Four changed lines and a header.
        assert!(diff.size.height >= px(18. * 4.), "{diff:?}");
        visual.simulate_click(toggle.center(), Default::default());
        visual.run_until_parked();
        assert!(visual.debug_bounds("tool-diff-1").is_none());
    }

    #[gpui::test]
    fn search_pills_and_overflow_count_share_a_single_compact_row(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|_| TranscriptView::new())
            })
            .unwrap()
        });
        window.update(cx, |view, _, cx| {
            let mut call = ToolCall::new("search", "Bash");
            call.input = r#"{"command":"rg -n 'model_settings_menu|render_model_settings|self.models|models.clone' src"}"#.into();
            call.state = ToolState::Completed;
            view.patch(0, vec![
                Row { role: "user".into(), text: "Find model settings".into(), first: true, sent_at: None },
                Row { role: "tool".into(), text: serde_json::to_string(&call).unwrap(), first: true, sent_at: None },
            ], cx);
            view.work_folds.insert(1, true);
            view.phases.insert(1, true);
            view.rebuild(0, cx);
        }).unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        for width in [900., 320.] {
            visual.simulate_resize(gpui::size(px(width), px(600.)));
            visual.run_until_parked();
            let row = visual.debug_bounds("tool-1").expect("search row");
            let first = visual
                .debug_bounds("tool-search-pill-1-0")
                .expect("first pattern");
            let second = visual
                .debug_bounds("tool-search-pill-1-1")
                .expect("second pattern");
            let more = visual
                .debug_bounds("tool-search-more-1")
                .expect("overflow count");
            assert_eq!(first.size.height, px(24.));
            assert_eq!(second.size.height, first.size.height);
            assert_eq!(more.size.height, first.size.height);
            assert_eq!(more.center().y, first.center().y);
            assert!(more.right() <= row.right());
            assert!(visual.debug_bounds("tool-search-pill-1-2").is_none());
            assert!(visual.debug_bounds("tool-input-toggle-1").is_none());
            visual.simulate_click(more.center(), Default::default());
            visual.run_until_parked();
            assert_eq!(
                visual.debug_bounds("tool-1").unwrap().size.height,
                row.size.height
            );
        }
    }

    #[gpui::test]
    fn chained_commands_show_step_chips_and_expand_into_steps(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|_| TranscriptView::new())
            })
            .unwrap()
        });
        window.update(cx, |view, _, cx| {
            let mut call = ToolCall::new("chain", "Bash");
            call.input = serde_json::json!({
                "command": "cd /repo && git status --short && cat package.json && sed -n '1,260p' scripts/build-native.mjs && sed -n '1,260p' scripts/dev-native.mjs",
                "description": "Inspect the native build scripts",
            })
            .to_string();
            call.state = ToolState::Completed;
            view.patch(0, vec![
                Row { role: "user".into(), text: "Check the build".into(), first: true, sent_at: None },
                Row { role: "tool".into(), text: serde_json::to_string(&call).unwrap(), first: true, sent_at: None },
            ], cx);
            view.work_folds.insert(1, true);
            view.phases.insert(1, true);
            view.rebuild(0, cx);
        }).unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        for width in [900., 320.] {
            visual.simulate_resize(gpui::size(px(width), px(600.)));
            visual.run_until_parked();
            let row = visual.debug_bounds("tool-1").expect("chain row");
            // `cd` is plumbing: git status, package.json, build-native.mjs, then +1.
            let first = visual
                .debug_bounds("tool-step-chip-1-0")
                .expect("first chip");
            let more = visual.debug_bounds("tool-step-more-1").expect("overflow");
            assert!(visual.debug_bounds("tool-step-chip-1-2").is_some());
            assert!(visual.debug_bounds("tool-step-chip-1-3").is_none());
            assert_eq!(more.center().y, first.center().y);
            assert!(more.right() <= row.right());
            assert!(row.size.height <= px(32.));
            assert!(visual.debug_bounds("tool-steps-1").is_none());
        }
        let row = visual.debug_bounds("tool-1").unwrap();
        visual.simulate_click(
            point(row.right() - px(8.), row.center().y),
            Default::default(),
        );
        visual.run_until_parked();
        let steps = visual.debug_bounds("tool-steps-1").expect("expanded steps");
        assert!(steps.size.height >= px(100.));
    }

    #[gpui::test]
    fn grouped_tool_rows_stay_compact_when_clicked(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|_| TranscriptView::new())
            })
            .unwrap()
        });
        window
            .update(cx, |view, _, cx| {
                let row = |role: &str, text: String| Row {
                    role: role.into(),
                    text,
                    first: true,
                    sent_at: None,
                };
                let mut call = ToolCall::new("read", "commandExecution");
                call.input = r#"{"command":"/bin/zsh -lc 'cat src/lib.rs'","cwd":"/repo"}"#.into();
                call.output = "pub fn main() {}".into();
                call.state = ToolState::Completed;
                let mut failed = ToolCall::new("run", "Bash");
                failed.input = r#"{"command":"npm run build"}"#.into();
                failed.output = "Build failed".into();
                failed.state = ToolState::Failed;
                view.patch(
                    0,
                    vec![
                        row("user", "Inspect".into()),
                        row("tool", serde_json::to_string(&call).unwrap()),
                        row("tool", serde_json::to_string(&failed).unwrap()),
                        row("assistant", "Done".into()),
                    ],
                    cx,
                );
                view.work_folds.insert(1, true);
                view.phases.insert(1, true);
                view.rebuild(0, cx);
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let caption = visual
            .debug_bounds("tool-1")
            .expect("the grouped tool is visible");
        // Clicking the caption does not open raw command/output details.
        visual.simulate_click(
            point(caption.right() - px(4.), caption.center().y),
            Default::default(),
        );
        visual.run_until_parked();
        window
            .update(visual, |view, _, _| {
                assert!(!view.expanded.contains(&1));
                let calls: Vec<_> = view
                    .rows
                    .iter()
                    .filter_map(|row| match &row.body {
                        Body::Tool { call, .. } => Some(call),
                        _ => None,
                    })
                    .collect();
                assert_eq!(calls.len(), 2);
                assert_eq!(calls[0].presentation().action, "Read");
                assert_eq!(calls[1].state, ToolState::Failed);
            })
            .unwrap();
        assert!(visual.debug_bounds("tool-target-1").is_some());
        assert!(visual.debug_bounds("tool-input-toggle-1").is_none());
    }

    #[gpui::test]
    fn reasoning_and_tools_share_the_phase_rail_and_command_count(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|_| TranscriptView::new())
            })
            .unwrap()
        });
        window
            .update(cx, |view, _, cx| {
                let row = |role: &str, text: &str| Row {
                    role: role.into(),
                    text: text.into(),
                    first: true,
                    sent_at: None,
                };
                let tool = |id: &str, name: &str, input: &str| {
                    let mut call = ToolCall::new(id, name);
                    call.input = input.into();
                    call.state = ToolState::Completed;
                    row("tool", &serde_json::to_string(&call).unwrap())
                };
                view.patch(
                    0,
                    vec![
                        row("user", "Check"),
                        row("assistant", "I'll inspect the connector."),
                        tool("run1", "Bash", r#"{"command":"pwd"}"#),
                        tool("read", "Read", r#"{"file_path":"src/transcript.rs"}"#),
                        row("reasoning", " \n"),
                        tool("find", "Grep", r#"{"pattern":"fn tool"}"#),
                        row(
                            "reasoning",
                            "**The connector is shared.**\n\nCheck its callers.",
                        ),
                        tool("run2", "Bash", r#"{"command":"cargo test"}"#),
                        row("assistant", "Done"),
                    ],
                    cx,
                );
                view.work_folds.insert(1, true);
                view.phases.insert(2, true);
                view.expanded.insert(6);
                view.rebuild(0, cx);
                let phases: Vec<_> = view
                    .rows
                    .iter()
                    .filter_map(|row| match &row.body {
                        Body::Phase { title, group, .. } => Some((title.as_str(), *group)),
                        _ => None,
                    })
                    .collect();
                assert_eq!(phases, [("Ran 2 commands and explored the project", 2)]);
                assert!(!view.rows.iter().any(|row| row.key.0 == 4));
                let thought: Vec<_> = view.rows.iter().filter(|row| row.key.0 == 6).collect();
                assert!(
                    thought
                        .iter()
                        .any(|row| matches!(row.body, Body::Reasoning))
                );
                assert!(
                    thought
                        .iter()
                        .any(|row| matches!(row.body, Body::Markdown(_)))
                );
                assert!(thought.iter().all(|row| row.phase_group() == Some(2)));
                assert_eq!(
                    view.rows
                        .iter()
                        .filter(|row| matches!(row.body, Body::Tool { .. }))
                        .count(),
                    4
                );
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        assert!(visual.debug_bounds("thinking-4").is_none());
        assert!(visual.debug_bounds("thinking-6").is_some());
        assert!(visual.debug_bounds("tool-7").is_some());
    }

    #[gpui::test]
    fn calm_work_folds_a_running_phase_under_its_current_step(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let view = cx.new(|_| TranscriptView::new());
        view.update(cx, |view, cx| {
            let row = |role: &str, text: &str| Row {
                role: role.into(),
                text: text.into(),
                first: true,
                sent_at: None,
            };
            let tool = |id: &str, name: &str, input: &str, state: ToolState| {
                let mut call = ToolCall::new(id, name);
                call.input = input.into();
                call.state = state;
                row("tool", &serde_json::to_string(&call).unwrap())
            };
            view.calm = true;
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
            let phase = view.rows.iter().find_map(|row| match &row.body {
                Body::Phase { title, active, .. } => Some((title.clone(), *active)),
                _ => None,
            });
            assert_eq!(phase, Some(("Editing src/lib.rs".into(), true)));
            // Folded: the steps wait behind the one line.
            assert!(
                !view
                    .rows
                    .iter()
                    .any(|row| matches!(row.body, Body::Tool { .. }))
            );
            // Opened by hand, it lists every step under the usual summary.
            view.phases.insert(1, true);
            view.rebuild(0, cx);
            assert!(
                view.rows
                    .iter()
                    .any(|row| matches!(row.body, Body::Tool { .. }))
            );
            let title = view.rows.iter().find_map(|row| match &row.body {
                Body::Phase { title, .. } => Some(title.clone()),
                _ => None,
            });
            assert_ne!(title.as_deref(), Some("Editing src/lib.rs"));
        });
    }

    #[gpui::test]
    fn first_assistant_prose_replaces_all_streamed_initial_thoughts(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let view = cx.new(|_| TranscriptView::new());
        view.update(cx, |view, cx| {
            let row = |role: &str, text: &str| Row {
                role: role.into(),
                text: text.into(),
                first: true,
                sent_at: None,
            };
            view.streaming(true, cx);
            view.patch(
                0,
                vec![
                    row("user", "Check"),
                    row("reasoning", "Plan one"),
                    row("reasoning", "Plan two"),
                ],
                cx,
            );
            view.phases.insert(1, true);
            view.rebuild(0, cx);
            assert_eq!(
                view.rows
                    .iter()
                    .filter(|r| matches!(r.body, Body::Reasoning))
                    .count(),
                2
            );
            view.patch(3, vec![row("assistant", "I'll inspect the connector.")], cx);
            assert!(
                !view
                    .rows
                    .iter()
                    .any(|r| matches!(r.body, Body::Reasoning | Body::Phase { .. }))
            );
        });
    }

    #[gpui::test]
    fn a_lone_tool_call_gets_its_own_group_header(cx: &mut TestAppContext) {
        let view = cx.new(|_| TranscriptView::new());
        let row = |role: &str, text: String| Row {
            role: role.into(),
            text,
            first: true,
            sent_at: None,
        };
        let step = |id: &str, state: ToolState| {
            let mut call = ToolCall::new(id, "Bash");
            call.input = r#"{"command":"ls"}"#.into();
            call.state = state;
            row("tool", serde_json::to_string(&call).unwrap())
        };
        // The header's activity, if there is one.
        let header = |view: &TranscriptView| {
            view.rows.iter().find_map(|r| match &r.body {
                Body::Phase { active, .. } => Some(*active),
                _ => None,
            })
        };
        let ungrouped = |view: &TranscriptView| {
            view.rows
                .iter()
                .any(|r| matches!(r.body, Body::Tool { group: None, .. }))
        };
        view.update(cx, |view, cx| {
            view.smooth_reveal = false;
            view.work_folds.insert(1, true);
            view.streaming(true, cx);
            view.patch(
                0,
                vec![
                    row("user", "List files".into()),
                    step("first", ToolState::Completed),
                ],
                cx,
            );
            assert_eq!(
                header(view),
                Some(true),
                "one call already has a live header"
            );
            assert!(!ungrouped(view), "the call sits under its header");

            view.patch(2, vec![row("assistant", "Done".into())], cx);
            assert_eq!(header(view), Some(false), "an answer settles the header");

            cx.set_reduce_motion(true);
            view.work_folds.insert(1, false);
            view.rebuild(1, cx);
            assert_eq!(header(view), None, "collapsed work hides the header too");
        });
    }
}
