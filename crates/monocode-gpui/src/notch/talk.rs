//! A chat with one of the user's agents inside the notch. A click on an
//! agent in the Agents page slides this in instead of bringing the app
//! forward: the latest things said, a field to answer, and a way back. What
//! is typed goes straight to the agent's own chat in the app, which stays
//! where it is. The app fills in what was said (`Feed::talk`), each thing
//! with what came with it: pictures show themselves and files are chips, a
//! click away from opening.
//!
//! What was said scrolls. The chat opens at its end and stays there while
//! the answer streams in; scrolling up to read leaves it where it is until
//! the reader comes back down or sends something. It follows the way the
//! transcript's live phases do (`transcript::live_phase::LiveScroll`).
use super::*;
use crate::transcript::attachments::{Origin, Photo, picture_mime};
use crate::transcript::live_phase::LiveScroll;

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
    /// What was said, and whether it follows the end.
    pub(super) scroll: LiveScroll,
    /// The pictures that came with what was said, by their file: each
    /// decoded once, off the interface thread, and let go when its message
    /// leaves the chat.
    photos: std::collections::HashMap<std::path::PathBuf, Entity<Photo>>,
}

#[cfg(test)]
impl Talking {
    /// How many pictures the chat holds decoded.
    pub(super) fn photo_count(&self) -> usize {
        self.photos.len()
    }
}

/// The square a picture in the chat fits in, and how many attachments one
/// message shows before the rest are counted.
const TALK_PHOTO: f32 = 132.;
const TALK_FILES: usize = 6;

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
            scroll: LiveScroll::default(),
            photos: Default::default(),
        });
        self.talk_field.update(cx, |field, cx| {
            field.set_placeholder(format!("Message {}", agent.name), cx);
            field.set_text(String::new(), cx);
        });
        cx.emit(NotchEvent::Watch(Some(session)));
        cx.notify();
    }

    /// Puts the caret in the chat's field and gives the notch the keyboard,
    /// so a chat opened to write in is written in at once.
    pub(super) fn write_to_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let field = gpui::Focusable::focus_handle(&self.talk_field, cx);
        window.focus(&field, cx);
        platform::take_keys(window);
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
        // What was sent is read where it lands, at the end.
        talk.scroll.follow();
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
        let Some(talk) = self.talk.as_mut() else {
            return div().into_any_element();
        };
        // Following, the end is kept in view after this frame's layout, so
        // a longer answer is read as it grows.
        talk.scroll.enable(true);
        let wheel = talk.scroll.wheeler();
        let scroll = talk.scroll.handle.clone();
        let feed = self.talk_feed();
        let busy = feed.is_some_and(|talk| talk.busy);
        let said = feed.map(|talk| talk.said.clone());
        let photos = self.talk_photos(said.as_deref().unwrap_or_default(), cx);
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
            .child(face_or_provider(Some(&talk.face), "", 16., busy, cx))
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
                    .child(crate::text_morph::text_morph(
                        "notch-talk-status",
                        if busy { "Answering…" } else { "Resting" },
                    )),
            )
            .child(pill("notch-talk-open", "Open in app", false).on_click(
                cx.listener(move |this, _, _, cx| this.emit(NotchEvent::OpenSession(session), cx)),
            ));
        // A few words rest on the field; a longer history scrolls up to its
        // first line. The wheel is used here before the scroller sees it.
        let view = cx.entity_id();
        let mut lines = div()
            .id("notch-talk-lines")
            .debug_selector(|| "notch-talk-lines".into())
            .min_h_full()
            .flex()
            .flex_col()
            .justify_end()
            .gap(px(6.))
            .py(px(4.))
            .on_scroll_wheel(move |event, window, cx| {
                if wheel(event.delta.pixel_delta(window.line_height()).y) {
                    cx.notify(view);
                    cx.stop_propagation();
                }
            });
        match said {
            None => {
                lines = lines.child(note("Loading the chat…"));
            }
            Some(said) if said.is_empty() => {
                lines = lines.child(note("Nothing said yet. Write below to start."));
            }
            Some(said) => {
                for (at, said) in said.into_iter().enumerate() {
                    // The words, then what came with them, on the side of
                    // who said it.
                    let words = (!said.text.is_empty()).then(|| {
                        div()
                            .max_w(gpui::relative(0.86))
                            .px(px(8.))
                            .py(px(4.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .when(said.mine, |bubble| {
                                bubble.bg(ink(0.12)).text_color(ink(0.95))
                            })
                            .when(!said.mine, |bubble| bubble.px(px(2.)).text_color(ink(0.82)))
                            .child(said.text)
                    });
                    let files = (!said.files.is_empty())
                        .then(|| attached_row(at, said.mine, &said.files, &photos, session, cx));
                    lines = lines.child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .when(said.mine, |column| column.items_end())
                            .when(!said.mine, |column| column.items_start())
                            .children(words)
                            .children(files),
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
            .child(
                div()
                    .id("notch-talk-said")
                    .debug_selector(|| "notch-talk-said".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .child(lines),
            )
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
                        // The field keeps the theme's own surface and ink,
                        // so what is typed reads in any theme.
                        div()
                            .debug_selector(|| "notch-talk-field".into())
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
                            .on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
                                platform::take_keys(window)
                            })
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

impl Notch {
    /// The pictures of what was said, one decoded photo per file: the ones
    /// no longer said are let go (and their textures with them), the new
    /// ones start decoding off the interface thread.
    fn talk_photos(
        &mut self,
        said: &[model::Said],
        cx: &mut Context<Self>,
    ) -> std::collections::HashMap<std::path::PathBuf, Entity<Photo>> {
        let Some(talk) = self.talk.as_mut() else {
            return Default::default();
        };
        let shown: Vec<&std::path::PathBuf> = said
            .iter()
            .flat_map(|said| said.files.iter().take(TALK_FILES))
            .filter(|file| file.image)
            .filter_map(|file| file.path.as_ref())
            .collect();
        talk.photos.retain(|path, _| shown.contains(&path));
        for path in shown {
            if !talk.photos.contains_key(path) {
                let origin = Origin::Path(path.clone());
                let mime = picture_mime(path).into();
                let photo = cx.new(|cx| Photo::framed(origin, mime, TALK_PHOTO, 2., cx));
                // Decoded, or not a picture after all: the chat draws again.
                cx.observe(&photo, |_, _, cx| cx.notify()).detach();
                talk.photos.insert(path.clone(), photo);
            }
        }
        talk.photos.clone()
    }
}

/// What came with something said: its pictures as they are, and anything
/// else as a chip with its name. A click opens the file with its app; what
/// lives only in the app (an agent's document) opens the chat there.
fn attached_row(
    at: usize,
    mine: bool,
    files: &[model::Attached],
    photos: &std::collections::HashMap<std::path::PathBuf, Entity<Photo>>,
    session: i64,
    cx: &mut Context<Notch>,
) -> gpui::Div {
    let mut row = div()
        .debug_selector(move || format!("notch-talk-files-{at}"))
        .max_w_full()
        .flex()
        .flex_wrap()
        .gap(px(4.))
        .when(mine, |row| row.justify_end());
    for (index, file) in files.iter().take(TALK_FILES).enumerate() {
        let id = at * (TALK_FILES + 1) + index;
        let open = file.path.clone();
        let click = cx.listener(move |this, _: &gpui::ClickEvent, _, cx| match &open {
            Some(path) => cx.open_with_system(path),
            None => this.emit(NotchEvent::OpenSession(session), cx),
        });
        // A picture that can be read shows itself; one that cannot, or
        // that is kept inside the transcript, is named like any file.
        let photo = file
            .path
            .as_ref()
            .filter(|_| file.image)
            .and_then(|path| photos.get(path))
            .filter(|photo| !photo.read(cx).failed());
        row = row.child(match photo {
            Some(photo) => div()
                .id(("notch-talk-photo", id))
                .debug_selector(move || format!("notch-talk-photo-{at}-{index}"))
                .role(Role::Button)
                .aria_label(format!("Open {}", file.name))
                .flex_shrink_0()
                .cursor_pointer()
                .hover(|style| style.opacity(0.85))
                .on_click(click)
                .child(photo.clone()),
            None => div()
                .id(("notch-talk-file", id))
                .debug_selector(move || format!("notch-talk-file-{at}-{index}"))
                .role(Role::Button)
                .aria_label(format!("Open {}", file.name))
                .h(px(22.))
                .flex_shrink_0()
                .px(px(7.))
                .flex()
                .items_center()
                .gap(px(5.))
                .bg(ink(0.1))
                .cursor_pointer()
                .hover(|style| style.bg(ink(0.16)))
                .on_click(click)
                .child(icon("file", 11.).text_color(ink(0.6)))
                .child(
                    div()
                        .max_w(px(150.))
                        .truncate()
                        .text_size(px(11.))
                        .text_color(ink(0.85))
                        .child(file.name.clone()),
                ),
        });
    }
    if files.len() > TALK_FILES {
        row = row.child(
            div()
                .h(px(22.))
                .px(px(7.))
                .flex()
                .items_center()
                .text_size(px(11.))
                .text_color(ink(0.5))
                .child(format!("+{}", files.len() - TALK_FILES)),
        );
    }
    row
}

fn note(text: &'static str) -> gpui::Div {
    div()
        .px(px(2.))
        .text_size(px(11.5))
        .text_color(ink(0.4))
        .child(text)
}
