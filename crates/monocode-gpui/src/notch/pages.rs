//! The open notch's Agents and Chat pages. Agents lists what waits for the
//! user, what works and what was chatted lately, each a click from its chat.
//! Chat asks a new agent from where the user is, without the workspace.
use super::*;

/// A section's small caps title.
pub(super) fn caption(text: &'static str) -> gpui::Div {
    div()
        .h(px(CAPTION))
        .pt(px(8.))
        .text_size(px(9.5))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(ink(0.4))
        .child(text.to_uppercase())
}

/// One chat as a row: its provider, its title, and a word on the right.
pub(super) fn session_row(
    id: impl Into<gpui::ElementId>,
    mark: AnyElement,
    title: &str,
    detail: String,
    detail_color: gpui::Hsla,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(ROW))
        .px(px(6.))
        .flex()
        .items_center()
        .gap(px(8.))
        .cursor_pointer()
        .hover(|style| style.bg(ink(0.07)))
        .child(
            div()
                .size(px(14.))
                .flex()
                .items_center()
                .justify_center()
                .child(mark),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .truncate()
                .text_size(px(12.))
                .text_color(ink(0.9))
                .child(title.to_owned()),
        )
        .child(
            div()
                .flex_shrink_0()
                .max_w(px(150.))
                .truncate()
                .text_size(px(11.))
                .text_color(detail_color)
                .child(detail),
        )
}

impl Notch {
    pub(super) fn agents_page(&mut self, _width: f32, cx: &mut Context<Self>) -> AnyElement {
        let feed = self.feed().clone();
        let now = now_ms();
        let mut list = div()
            .id("notch-agents-page")
            .debug_selector(|| "notch-agents-page".into())
            .h(px(self.agents_height()))
            .overflow_y_scroll()
            .flex()
            .flex_col();
        if !feed.asks.is_empty() {
            list = list.child(caption("Needs you"));
            for ask in feed.asks {
                let (open, allow, deny) = (ask.session, ask.clone(), ask.clone());
                let row = session_row(
                    gpui::SharedString::from(format!("notch-ask-{}-{}", ask.session, ask.request)),
                    provider_glyph(&ask.provider, 13.),
                    &ask.title,
                    ask.label.clone(),
                    accent_color(),
                )
                .on_click(
                    cx.listener(move |this, _, _, cx| this.emit(NotchEvent::OpenSession(open), cx)),
                );
                let row = if ask.approval {
                    row.child(
                        pill(
                            gpui::SharedString::from(format!("notch-page-deny-{}", ask.request)),
                            "Deny",
                            false,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.emit(
                                NotchEvent::Decide {
                                    session: deny.session,
                                    request: deny.request.clone(),
                                    decision: Decision::Deny,
                                },
                                cx,
                            )
                        })),
                    )
                    .child(
                        pill(
                            gpui::SharedString::from(format!("notch-page-allow-{}", ask.request)),
                            "Allow",
                            true,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.emit(
                                NotchEvent::Decide {
                                    session: allow.session,
                                    request: allow.request.clone(),
                                    decision: Decision::Allow,
                                },
                                cx,
                            )
                        })),
                    )
                } else {
                    row
                };
                list = list.child(row);
            }
        }
        let working: Vec<_> = feed.activities.iter().filter(|a| !a.done).collect();
        if !working.is_empty() {
            list = list.child(caption("Working"));
            for activity in working {
                let id = activity.id;
                let detail = match activity.started_at {
                    Some(at) => format!("{} · {}", activity.step, model::ago(at, now)),
                    None => activity.step.clone(),
                };
                list =
                    list.child(
                        session_row(
                            ("notch-working", id as u64),
                            activity_mark(activity, cx),
                            &activity.title,
                            detail,
                            ink(0.5),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.emit(NotchEvent::OpenSession(id), cx)
                        })),
                    );
            }
        }
        let finished: Vec<_> = feed.activities.iter().filter(|a| a.done).collect();
        if !finished.is_empty() {
            list = list.child(caption("Finished"));
            for activity in finished {
                let id = activity.id;
                list =
                    list.child(
                        session_row(
                            ("notch-finished", id as u64),
                            activity_mark(activity, cx),
                            &activity.title,
                            "Done".into(),
                            ink(0.5),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.emit(NotchEvent::OpenSession(id), cx)
                        })),
                    );
            }
        }
        list = list.child(caption("Recent"));
        if feed.recent.is_empty() {
            list = list.child(
                div()
                    .px(px(6.))
                    .text_size(px(11.5))
                    .text_color(ink(0.4))
                    .child("No chats yet."),
            );
        }
        for recent in feed.recent {
            let id = recent.id;
            list = list.child(
                session_row(
                    ("notch-recent", id as u64),
                    provider_glyph(&recent.provider, 13.),
                    &recent.title,
                    model::ago(recent.updated_at, now),
                    ink(0.4),
                )
                .on_click(
                    cx.listener(move |this, _, _, cx| this.emit(NotchEvent::OpenSession(id), cx)),
                ),
            );
        }
        list.into_any_element()
    }

    pub(super) fn chat_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let feed = self.feed().clone();
        let now = now_ms();
        let field = self.field.clone();
        div()
            .debug_selector(|| "notch-chat-page".into())
            .h(px(self.chat_height()))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .mt(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        // The field keeps the theme's own surface, so what is
                        // typed reads in any theme.
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(px(32.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .bg(background_base())
                            .border_1()
                            .border_color(ink(0.16))
                            .child(field),
                    )
                    .child(
                        pill("notch-ask", "Ask", true)
                            .h(px(32.))
                            .on_click(cx.listener(|this, _, _, cx| this.ask(cx))),
                    ),
            )
            .child(
                div().flex().gap(px(6.)).child(
                    pill("notch-quick-composer", "Quick composer…", false)
                        .on_click(cx.listener(|this, _, _, cx| this.emit(NotchEvent::NewChat, cx))),
                ),
            )
            .child(caption("Continue"))
            .children(feed.recent.into_iter().take(3).map(|recent| {
                let id = recent.id;
                session_row(
                    ("notch-chat-recent", id as u64),
                    provider_glyph(&recent.provider, 13.),
                    &recent.title,
                    model::ago(recent.updated_at, now),
                    ink(0.4),
                )
                .on_click(
                    cx.listener(move |this, _, _, cx| this.emit(NotchEvent::OpenSession(id), cx)),
                )
            }))
            .into_any_element()
    }
}
