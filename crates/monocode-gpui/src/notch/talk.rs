//! A chat with one of the user's agents inside the notch. A click on an
//! agent in the Agents page slides this in instead of bringing the app
//! forward: the latest things said, a field to answer, and a way back. What
//! is typed goes straight to the agent's own chat in the app, which stays
//! where it is. The app fills in what was said (`Feed::talk`).
use super::*;

/// The chat's header: back, who it is, and Open.
const TALK_HEADER: f32 = 30.;
/// How long the chat takes to slide in.
const SLIDE: f32 = 0.18;

/// Who the notch talks with.
pub(super) struct Talking {
    pub session: i64,
    name: String,
    face: model::Face,
    /// When the chat was opened, for its slide.
    since: Instant,
    /// Why the last message could not be sent.
    error: Option<String>,
}

impl Notch {
    /// The agent whose chat is open in the notch.
    pub(crate) fn talking_with(&self) -> Option<i64> {
        self.talk.as_ref().map(|talk| talk.session)
    }

    /// Slides in the chat with an agent, and asks the app for what was said.
    pub(super) fn open_talk(&mut self, agent: &model::Personal, cx: &mut Context<Self>) {
        let Some(session) = agent.session else {
            return;
        };
        self.talk = Some(Talking {
            session,
            name: agent.name.clone(),
            face: agent.face.clone(),
            since: Instant::now(),
            error: None,
        });
        self.talk_field.update(cx, |field, cx| {
            field.set_placeholder(format!("Message {}", agent.name), cx);
            field.set_text(String::new(), cx);
        });
        cx.emit(NotchEvent::Watch(Some(session)));
        cx.notify();
    }

    /// Back to the agents.
    pub(super) fn close_talk(&mut self, cx: &mut Context<Self>) {
        if self.talk.take().is_some() {
            cx.emit(NotchEvent::Watch(None));
            cx.notify();
        }
    }

    /// Sends what was typed to the agent's chat. The app stays where it is.
    pub(super) fn tell(&mut self, cx: &mut Context<Self>) {
        let Some(talk) = self.talk.as_mut() else {
            return;
        };
        let text = self.talk_field.read(cx).content.trim().to_owned();
        if text.is_empty() {
            return;
        }
        talk.error = None;
        let session = talk.session;
        self.talk_field
            .update(cx, |field, cx| field.set_text(String::new(), cx));
        cx.emit(NotchEvent::Tell { session, text });
        cx.notify();
    }

    /// The app could not deliver the last message: say why, and give the
    /// words back.
    pub(crate) fn tell_failed(&mut self, text: String, error: String, cx: &mut Context<Self>) {
        let Some(talk) = self.talk.as_mut() else {
            return;
        };
        talk.error = Some(error);
        if self.talk_field.read(cx).content.is_empty() {
            self.talk_field
                .update(cx, |field, cx| field.set_text(text, cx));
        }
        cx.notify();
    }

    /// What was said, as the app last told it, if it is this chat's.
    fn talk_feed(&self) -> Option<&model::Talk> {
        let session = self.talking_with()?;
        self.feed
            .talk
            .as_ref()
            .filter(|talk| talk.session == session)
    }

    pub(super) fn talk_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(talk) = self.talk.as_ref() else {
            return div().into_any_element();
        };
        // Slides down into place; under reduced motion it is simply there.
        let shown = if cx.reduce_motion() {
            1.
        } else {
            let t = (talk.since.elapsed().as_secs_f32() / SLIDE).min(1.);
            if t < 1. {
                window.request_animation_frame();
            }
            // Ease out.
            1. - (1. - t) * (1. - t)
        };
        let session = talk.session;
        let feed = self.talk_feed();
        let busy = feed.is_some_and(|talk| talk.busy);
        let said = feed.map(|talk| talk.said.clone());
        let header = div()
            .h(px(TALK_HEADER))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                glyph(
                    "notch-talk-back",
                    "Back to your agents",
                    "chevron-left",
                    false,
                )
                .size(px(22.))
                .on_click(cx.listener(|this, _, _, cx| this.close_talk(cx))),
            )
            .child(face_or_provider(Some(&talk.face), "", 16., cx))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.5))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(ink(0.95))
                    .child(talk.name.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.))
                    .text_color(if busy { accent_color() } else { ink(0.4) })
                    .child(if busy { "Answering…" } else { "Resting" }),
            )
            .child(pill("notch-talk-open", "Open in app", false).on_click(
                cx.listener(move |this, _, _, cx| this.emit(NotchEvent::OpenSession(session), cx)),
            ));
        // The latest words rest on the field; older ones leave at the top.
        let mut lines = div()
            .debug_selector(|| "notch-talk-said".into())
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .justify_end()
            .gap(px(6.))
            .py(px(4.));
        match said {
            None => {
                lines = lines.child(note("Loading the chat…"));
            }
            Some(said) if said.is_empty() => {
                lines = lines.child(note("Nothing said yet. Write below to start."));
            }
            Some(said) => {
                for said in said {
                    lines = lines.child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .when(said.mine, |row| row.justify_end())
                            .child(
                                div()
                                    .max_w(gpui::relative(0.86))
                                    .px(px(8.))
                                    .py(px(4.))
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .line_clamp(if said.mine { 3 } else { 8 })
                                    .when(said.mine, |bubble| {
                                        bubble.bg(ink(0.12)).text_color(ink(0.95))
                                    })
                                    .when(!said.mine, |bubble| {
                                        bubble.px(px(2.)).text_color(ink(0.82))
                                    })
                                    .child(said.text),
                            ),
                    );
                }
            }
        }
        let error = talk.error.clone();
        div()
            .debug_selector(|| "notch-talk".into())
            .h(px(self.agents_height()))
            .relative()
            .top(px((shown - 1.) * 12.))
            .opacity(shown)
            .flex()
            .flex_col()
            .child(header)
            .child(lines)
            .children(error.map(|error| {
                div()
                    .debug_selector(|| "notch-talk-error".into())
                    .flex_shrink_0()
                    .pb(px(4.))
                    .text_size(px(11.))
                    .text_color(accent_color())
                    .child(error)
            }))
            .child(
                div()
                    .flex_shrink_0()
                    .mb(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        // The field keeps the theme's own surface, so what
                        // is typed reads in any theme.
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
                            .child(self.talk_field.clone()),
                    )
                    .child(
                        pill("notch-talk-send", "Send", true)
                            .h(px(32.))
                            .on_click(cx.listener(|this, _, _, cx| this.tell(cx))),
                    ),
            )
            .into_any_element()
    }
}

fn note(text: &'static str) -> gpui::Div {
    div()
        .px(px(2.))
        .text_size(px(11.5))
        .text_color(ink(0.4))
        .child(text)
}
