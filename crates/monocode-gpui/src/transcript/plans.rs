//! PlanPreview for local sessions: a plan turn's plan as a card with Open,
//! Build and "Build with another model".
use super::*;
use gpui::AnyElement;
use monocode_core::plan::{self, Status};

/// A `plan` row, ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Card {
    /// The row's index in the transcript, which names the plan.
    pub at: usize,
    pub text: String,
    pub status: Status,
}

impl Card {
    pub(super) fn parse(source: &[Row], at: usize, text: &str, streaming: bool) -> Option<Self> {
        let row = serde_json::from_str::<plan::PlanRow>(text).ok()?;
        (!row.text.trim().is_empty()).then(|| Self {
            at,
            status: plan::status(source, at, streaming),
            text: row.text,
        })
    }
    pub(super) fn can_build(&self, busy: bool) -> bool {
        !busy && self.status == Status::Ready
    }
    fn build_label(&self) -> &'static str {
        match self.status {
            Status::Ready => "Build",
            Status::Building => "Building…",
            Status::Built => "Built",
        }
    }
}

/// Whether a turn's plan was promoted from its final answer.
pub(super) fn promoted(turn: &[Row]) -> bool {
    turn.iter().enumerate().any(|(ix, row)| {
        row.first && row.role == plan::ROLE && {
            let end = turn[ix + 1..]
                .iter()
                .position(|row| row.first)
                .map_or(turn.len(), |n| ix + 1 + n);
            let text: String = turn[ix..end].iter().map(|r| r.text.as_str()).collect();
            serde_json::from_str::<plan::PlanRow>(&text).is_ok_and(|row| row.answer)
        }
    })
}

/// Claude's `ExitPlanMode` step, whose plan has its own row.
pub(super) fn hands_back_plan(tool: &ToolCall) -> bool {
    plan::is_exit_plan_mode(&tool.name) && plan::exit_plan_mode_plan(&tool.input).is_some()
}

/// The card both local and host plans share: an icon, the title, a short
/// summary and the actions under it.
pub(super) fn card_shell(
    id: SharedString,
    streaming: bool,
    title: String,
    summary: String,
    actions: gpui::Div,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id.clone())
        .role(Role::Group)
        .aria_label(format!("Plan: {title}"))
        .w_full()
        .my_2()
        .p_3()
        .rounded(px(12.))
        .border_1()
        .border_color(content(0.1))
        .bg(content(0.06))
        .flex()
        .items_start()
        .gap_2()
        .child(icon(if streaming { "loader" } else { "file-text" }, 16.).text_color(content(0.45)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(content(0.9))
                        .truncate()
                        .child(title),
                )
                .when(!summary.is_empty(), |d| {
                    d.child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .text_color(content(0.5))
                            .child(summary),
                    )
                })
                .child(actions.flex().justify_end().gap_2()),
        )
}

impl TranscriptView {
    pub(super) fn plan_card(&mut self, card: &Card, cx: &mut Context<Self>) -> AnyElement {
        let at = card.at;
        let enabled = card.can_build(self.streaming);
        let open_text = card.text.clone();
        let build_text = card.text.clone();
        let anchor = self
            .action_anchors
            .entry((at, "build-plan"))
            .or_default()
            .clone();
        let kind = monocode_core::response::TransferKind::Build;
        let picking = self.target_action == Some((at, kind));
        let click_anchor = anchor.clone();
        let actions = div()
            .child(
                crate::settings::widgets::secondary_button(
                    SharedString::from(format!("open-local-plan-{at}")),
                    "Open plan in pane",
                    false,
                    false,
                )
                .debug_selector(move || format!("open-local-plan-{at}"))
                .child("Open")
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(ResponseRequest::LocalPlan {
                        at,
                        text: open_text.clone(),
                        build: false,
                    })
                })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        crate::settings::widgets::secondary_button(
                            SharedString::from(format!("build-local-plan-{at}")),
                            "Build this plan",
                            false,
                            !enabled,
                        )
                        .debug_selector(move || format!("build-local-plan-{at}"))
                        .child(card.build_label())
                        .on_click(cx.listener(move |_, _, _, cx| {
                            if enabled {
                                cx.emit(ResponseRequest::LocalPlan {
                                    at,
                                    text: build_text.clone(),
                                    build: true,
                                });
                            }
                        })),
                    )
                    .child(
                        crate::settings::widgets::secondary_button(
                            SharedString::from(format!("build-local-plan-with-{at}")),
                            "Build with another model",
                            false,
                            !enabled,
                        )
                        .px_1()
                        .aria_expanded(picking)
                        .tooltip(chrome_tip("Build with another model", None))
                        .child(anchor.probe())
                        .child(icon("chevron-down", 14.))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            if enabled {
                                cx.emit(ResponseRequest::Target {
                                    turn: at,
                                    kind,
                                    anchor: click_anchor.clone(),
                                });
                            }
                        })),
                    ),
            );
        card_shell(
            SharedString::from(format!("local-plan-{at}")),
            false,
            plan::title(&card.text),
            plan::summary(&card.text),
            actions,
        )
        .debug_selector(move || format!("local-plan-{at}"))
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn row(role: &str, text: &str) -> Row {
        Row {
            role: role.into(),
            text: text.into(),
            first: true,
            sent_at: None,
        }
    }

    fn cards(view: &TranscriptView) -> Vec<Card> {
        view.rows
            .iter()
            .filter_map(|row| match &row.body {
                Body::Plan(card) => Some((**card).clone()),
                _ => None,
            })
            .collect()
    }

    fn plain_texts(view: &TranscriptView) -> Vec<String> {
        view.rows
            .iter()
            .filter_map(|row| match &row.body {
                Body::Markdown(md) => Some(markdown::row_text(md)),
                Body::Plain(text) => Some(text.to_string()),
                _ => None,
            })
            .collect()
    }

    #[gpui::test]
    fn a_promoted_answer_is_shown_as_its_plan_card(cx: &mut TestAppContext) {
        let plan = "# Add dark mode\n\n- Read the palette\n- Wire the toggle";
        let view = cx.new(|cx| {
            let mut view = TranscriptView::new();
            view.patch(
                0,
                vec![
                    row("user", "Plan dark mode"),
                    row("assistant", plan),
                    row(
                        plan::ROLE,
                        &serde_json::to_string(&plan::PlanRow {
                            text: plan.into(),
                            answer: true,
                        })
                        .unwrap(),
                    ),
                ],
                cx,
            );
            view
        });
        view.read_with(cx, |view, _| {
            let cards = cards(view);
            assert_eq!(cards.len(), 1);
            assert_eq!(cards[0].at, 2);
            assert_eq!(cards[0].status, Status::Ready);
            // The answer is not drawn twice.
            assert!(
                !plain_texts(view)
                    .iter()
                    .any(|text| text.contains("Read the palette")),
                "{:?}",
                plain_texts(view)
            );
        });
    }

    #[gpui::test]
    fn exit_plan_mode_steps_give_way_to_their_card(cx: &mut TestAppContext) {
        let mut tool = ToolCall::new("t", "ExitPlanMode");
        tool.input = serde_json::json!({"plan":"# Ship\n- a\n- b"}).to_string();
        tool.state = monocode_core::tool::ToolState::Completed;
        let view = cx.new(|cx| {
            let mut view = TranscriptView::new();
            view.patch(
                0,
                vec![
                    row("user", "Plan it"),
                    row("tool", &serde_json::to_string(&tool).unwrap()),
                    row(plan::ROLE, r##"{"text":"# Ship\n- a\n- b"}"##),
                    row("assistant", "Here is the plan."),
                ],
                cx,
            );
            view
        });
        view.read_with(cx, |view, _| {
            assert!(
                !view
                    .rows
                    .iter()
                    .any(|row| matches!(row.body, Body::Tool { .. }))
            );
            assert_eq!(cards(view).len(), 1);
            // A plan that was not promoted keeps the answer.
            assert!(
                plain_texts(view)
                    .iter()
                    .any(|t| t.contains("Here is the plan"))
            );
        });
    }

    #[gpui::test]
    fn build_and_open_emit_the_plan_and_built_plans_lock(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let plan_row = r##"{"text":"# Ship\n- a\n- b"}"##;
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = TranscriptView::new();
                    view.patch(
                        0,
                        vec![row("user", "Plan it"), row(plan::ROLE, plan_row)],
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
                if let ResponseRequest::LocalPlan { at, text, build } = request {
                    seen.borrow_mut().push((*at, text.clone(), *build));
                }
            })
            .detach()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let build = visual.debug_bounds("build-local-plan-1").expect("Build");
        visual.simulate_click(build.center(), Default::default());
        let open = visual.debug_bounds("open-local-plan-1").expect("Open");
        visual.simulate_click(open.center(), Default::default());
        assert_eq!(
            requests.borrow().as_slice(),
            &[
                (1, "# Ship\n- a\n- b".to_string(), true),
                (1, "# Ship\n- a\n- b".to_string(), false)
            ]
        );
        view.update(visual, |view, cx| {
            view.patch(
                2,
                vec![row("user", plan::BUILD_MESSAGE), row("assistant", "Done")],
                cx,
            );
        });
        visual.run_until_parked();
        view.read_with(visual, |view, _| {
            let card = cards(view).remove(0);
            assert_eq!(card.status, Status::Built);
            assert!(!card.can_build(false));
        });
        // A built plan's Build does nothing.
        requests.borrow_mut().clear();
        let build = visual.debug_bounds("build-local-plan-1").expect("Build");
        visual.simulate_click(build.center(), Default::default());
        assert!(requests.borrow().is_empty());
    }
}
