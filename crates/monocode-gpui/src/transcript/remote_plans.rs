//! The original PlanPreview actions, backed by host-owned plan identities.
use super::*;
use crate::conversation::remote_plans::Plan;
use gpui::AnyElement;

impl TranscriptView {
    pub(super) fn remote_plan_card(&self, plan: &Plan, cx: &mut Context<Self>) -> AnyElement {
        let open = plan.clone();
        let build = plan.clone();
        let enabled = plan.can_build(self.streaming);
        let actions = div()
            .child(
                crate::settings::widgets::secondary_button(
                    SharedString::from(format!("open-plan-{}", plan.id)),
                    "Open plan",
                    false,
                    false,
                )
                .child("Open")
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(ResponseRequest::RemotePlan {
                        id: open.id.clone(),
                        text: open.text.clone(),
                        build: false,
                    })
                })),
            )
            .child(
                crate::settings::widgets::secondary_button(
                    SharedString::from(format!("build-plan-{}", plan.id)),
                    "Build approved plan",
                    false,
                    !enabled,
                )
                .child(plan.build_label())
                .on_click(cx.listener(move |_, _, _, cx| {
                    if enabled {
                        cx.emit(ResponseRequest::RemotePlan {
                            id: build.id.clone(),
                            text: build.text.clone(),
                            build: true,
                        });
                    }
                })),
            );
        super::plans::card_shell(
            SharedString::from(format!("remote-plan-{}", plan.id)),
            plan.streaming,
            plan.title(),
            plan.summary(),
            actions,
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[gpui::test]
    fn remote_plans_render_as_cards_in_their_source_position(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let view=cx.new(|cx| {
                let mut view=TranscriptView::new();
                view.patch(0,vec![
                    Row{role:"user".into(),text:"Make a plan".into(),first:true,sent_at:None},
                    Row{role:"remote-plan".into(),text:serde_json::json!({"id":"host-plan","text":"# Plan\n\n- Step","status":"ready","streaming":false}).to_string(),first:true,sent_at:None},
                    Row{role:"assistant".into(),text:"Ready for review".into(),first:true,sent_at:None}
                ],cx);
                view
            });
            let cards:Vec<_>=view.read(cx).rows.iter().filter(|row|matches!(row.body,Body::RemotePlan(_))).collect();
            assert_eq!(cards.len(),1);
            assert_eq!(cards[0].key.0,1);
            assert_eq!(view.read(cx).source_rows()[2].text,"Ready for review");
        });
    }
}
