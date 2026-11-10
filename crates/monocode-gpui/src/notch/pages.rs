//! The open notch's Agents and Chat pages. Agents is the user's own agents
//! (Monos): who needs them, what each does now. Chat is the coding chats:
//! ask a new one from where the user is, answer what waits, follow what
//! works and pick up a recent chat.
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
    session_row_keyed(id, "detail", mark, title, detail, detail_color)
}

/// [`session_row`] for a list whose rows change places: `detail_id` follows
/// the thing the row is about, so its word morphs only when that thing's own
/// word changes.
pub(super) fn session_row_keyed(
    id: impl Into<gpui::ElementId>,
    detail_id: impl Into<gpui::ElementId>,
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
                // The word on the right changes in place: "Text · 2m" to
                // "Copied", a state to the next, a minute to the one after.
                .child(crate::text_morph::text_morph(detail_id, detail).within(px(150.))),
        )
}

impl Notch {
    /// Whether a chat belongs to one of the user's own agents (a Mono, its
    /// habits, a seat in a group) rather than to a coding session.
    pub(super) fn agent_chat(&self, id: i64) -> bool {
        let feed = &self.feed;
        feed.agents.iter().any(|agent| agent.session == Some(id))
            || feed
                .activities
                .iter()
                .any(|activity| activity.id == id && activity.agent.is_some())
    }

    /// The asks of the agents (`agents`) or of the coding chats.
    pub(super) fn asks_of(&self, agents: bool) -> Vec<model::Ask> {
        self.feed
            .asks
            .iter()
            .filter(|ask| self.agent_chat(ask.session) == agents)
            .cloned()
            .collect()
    }

    /// A pending ask as a row: open its chat, or answer an approval here.
    fn ask_row(&self, ask: model::Ask, mark: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        let (open, allow, deny) = (ask.session, ask.clone(), ask.clone());
        let row = session_row(
            gpui::SharedString::from(format!("notch-ask-{}-{}", ask.session, ask.request)),
            mark,
            &ask.title,
            ask.label.clone(),
            accent_color(),
        )
        .on_click(cx.listener(move |this, _, _, cx| this.emit(NotchEvent::OpenSession(open), cx)));
        if !ask.approval {
            return row.into_any_element();
        }
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
        .into_any_element()
    }

    /// Agents working that are not one of the listed agents' own chats: a
    /// habit, a group member in its seat.
    pub(super) fn other_agent_work(&self) -> Vec<model::Activity> {
        self.feed
            .activities
            .iter()
            .filter(|activity| {
                activity.agent.is_some()
                    && !activity.done
                    && !self
                        .feed
                        .agents
                        .iter()
                        .any(|agent| agent.session == Some(activity.id))
            })
            .cloned()
            .collect()
    }

    /// The coding chats' activities, at work or finished.
    pub(super) fn coding_work(&self, done: bool) -> Vec<model::Activity> {
        self.feed
            .activities
            .iter()
            .filter(|activity| activity.agent.is_none() && activity.done == done)
            .cloned()
            .collect()
    }

    /// The Agents page as tall as its sections, up to where it scrolls.
    pub(super) fn agents_height(&self) -> f32 {
        // The chat with one agent takes the page's whole room.
        if self.talking_with().is_some() {
            return AGENTS_MAX;
        }
        (section(self.asks_of(true).len())
            + section(self.feed.agents.len().max(1))
            + section(self.other_agent_work().len())
            + 4.)
            .min(AGENTS_MAX)
    }

    /// The Chat page: its field and the quick composer, then the coding
    /// chats' sections, up to where they scroll.
    pub(super) fn chat_height(&self) -> f32 {
        (CHAT_FIELD
            + section(self.asks_of(false).len())
            + section(self.coding_work(false).len())
            + section(self.coding_work(true).len())
            + section(self.feed.recent.len().clamp(1, RECENT_ON_CHAT))
            + 4.)
            .min(AGENTS_MAX)
    }

    /// The user's own agents: who needs them, each agent with what it does
    /// now, and any other agent at work.
    pub(super) fn agents_page(&mut self, _width: f32, cx: &mut Context<Self>) -> AnyElement {
        let mut list = div()
            .id("notch-agents-page")
            .debug_selector(|| "notch-agents-page".into())
            .h(px(self.agents_height()))
            .overflow_y_scroll()
            .flex()
            .flex_col();
        let asks = self.asks_of(true);
        if !asks.is_empty() {
            list = list.child(caption("Needs you"));
            for ask in asks {
                let face = self
                    .feed
                    .agents
                    .iter()
                    .find(|agent| agent.session == Some(ask.session))
                    .map(|agent| agent.face.clone());
                let mark = face_or_provider(face.as_ref(), &ask.provider, 14., false, cx);
                list = list.child(self.ask_row(ask, mark, cx));
            }
        }
        list = list.child(caption("Your agents"));
        let agents = self.feed.agents.clone();
        if agents.is_empty() {
            list = list.child(
                div()
                    .px(px(6.))
                    .text_size(px(11.5))
                    .text_color(ink(0.4))
                    .child("No agents yet. Make one in Monos."),
            );
        }
        for (index, agent) in agents.into_iter().enumerate() {
            let (detail, color) = match (&agent.doing, agent.needs_you) {
                (_, true) => ("Needs you".to_owned(), accent_color()),
                (Some(doing), _) => (doing.clone(), ink(0.6)),
                (None, _) => ("Resting".to_owned(), ink(0.35)),
            };
            let mut row = session_row(
                ("notch-agent", index as u64),
                face_or_provider(
                    Some(&agent.face),
                    "",
                    14.,
                    agent.doing.is_some() && !agent.needs_you,
                    cx,
                ),
                &agent.name,
                detail,
                color,
            );
            if agent.session.is_some() {
                // Its chat slides in here; the app stays where it is.
                let agent = agent.clone();
                row = row
                    .debug_selector(move || format!("notch-agent-{index}"))
                    .on_click(cx.listener(move |this, _, _, cx| this.open_talk(&agent, cx)));
            }
            list = list.child(row);
        }
        let others = self.other_agent_work();
        if !others.is_empty() {
            list = list.child(caption("Also working"));
            let now = now_ms();
            for activity in others {
                let id = activity.id;
                let detail = match activity.started_at {
                    Some(at) => format!("{} · {}", activity.step, model::ago(at, now)),
                    None => activity.step.clone(),
                };
                list =
                    list.child(
                        session_row(
                            ("notch-agent-work", id as u64),
                            activity_mark(&activity, cx),
                            activity.agent.as_deref().unwrap_or(&activity.title),
                            detail,
                            ink(0.5),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.emit(NotchEvent::OpenSession(id), cx)
                        })),
                    );
            }
        }
        list.into_any_element()
    }

    /// The coding chats: ask a new one, answer what waits, follow what
    /// works and finished, and pick up a recent chat.
    pub(super) fn chat_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let now = now_ms();
        let field = self.field.clone();
        let mut list = div()
            .id("notch-chat-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col();
        let asks = self.asks_of(false);
        if !asks.is_empty() {
            list = list.child(caption("Needs you"));
            for ask in asks {
                let mark = provider_glyph(&ask.provider, 13.);
                list = list.child(self.ask_row(ask, mark, cx));
            }
        }
        for (done, title, prefix) in [
            (false, "Working", "notch-working"),
            (true, "Finished", "notch-finished"),
        ] {
            let work = self.coding_work(done);
            if work.is_empty() {
                continue;
            }
            list = list.child(caption(title));
            for activity in work {
                let id = activity.id;
                let detail = match (done, activity.started_at) {
                    (true, _) => "Done".to_owned(),
                    (false, Some(at)) => format!("{} · {}", activity.step, model::ago(at, now)),
                    (false, None) => activity.step.clone(),
                };
                list =
                    list.child(
                        session_row(
                            (prefix, id as u64),
                            activity_mark(&activity, cx),
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
        list = list.child(caption("Recent"));
        let recent: Vec<_> = self
            .feed
            .recent
            .iter()
            .take(RECENT_ON_CHAT)
            .cloned()
            .collect();
        if recent.is_empty() {
            list = list.child(
                div()
                    .px(px(6.))
                    .text_size(px(11.5))
                    .text_color(ink(0.4))
                    .child("No chats yet."),
            );
        }
        for recent in recent {
            let id = recent.id;
            list = list.child(
                session_row(
                    ("notch-chat-recent", id as u64),
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
                        // The field keeps the theme's own surface and ink, so
                        // what is typed reads in any theme.
                        div()
                            .debug_selector(|| "notch-chat-field".into())
                            .flex_1()
                            .min_w_0()
                            .h(px(32.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .bg(background_base())
                            .text_color(crate::theme::content(0.95))
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
            .child(list)
            .into_any_element()
    }
}

/// The room the Chat page's field and composer button take above its lists.
const CHAT_FIELD: f32 = 40. + 28.;
/// The recent chats the Chat page lists.
const RECENT_ON_CHAT: usize = 5;

/// A section's height: its caption and rows, or nothing when empty.
fn section(rows: usize) -> f32 {
    if rows == 0 {
        0.
    } else {
        CAPTION + ROW * rows as f32
    }
}
