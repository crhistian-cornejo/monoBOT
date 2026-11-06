//! The reply to a message (the desktop app's MailReply and MailPeople): who it goes from
//! and to, with faces; a subject and text AI can write as the user watches; and Send, with
//! a few seconds to undo. Who it goes to is the core's to decide; nothing leaves without
//! the user's press.
use super::*;
use crate::reveal::{PacedText, WordFades};
use gpui::{Animation, AnimationExt, HighlightStyle, StyledText};
use monocode_core::{
    inbox::mail::{Person, Recipients, ReplyInput},
    session_title::TextModel,
};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

/// Seconds to change one's mind after pressing Send.
const UNDO_SECONDS: u32 = 5;

pub(crate) enum ReplyEvent {
    Close,
    Sent,
}

pub(crate) struct MailReply {
    service: model::Service,
    message: Message,
    /// The user's account the reply goes from.
    account: Option<AccountView>,
    photo: Option<Arc<Image>>,
    opened: Option<Opened>,
    subject: Entity<Input>,
    text: Entity<Input>,
    ask: Entity<Input>,
    writing: bool,
    /// What AI has written so far this run, shown as it streams.
    streamed: Option<String>,
    started: Instant,
    /// The model finished its message, though its harness may not have exited yet.
    arrived: bool,
    /// The streamed subject and text, word by word, as a chat reply streams.
    reveal: Option<(PacedText, PacedText)>,
    /// What AI wrote and what the fields held before, waiting for the last streamed word
    /// to settle before the text becomes the editable box in its place.
    settling: Option<(String, (String, String))>,
    /// What the box and the subject held before AI last wrote in them, to undo to.
    before: Option<(String, String)>,
    /// The request AI last wrote for, to try again with.
    last_ask: Option<String>,
    people: Option<Recipients>,
    all: bool,
    status: Option<(String, bool)>,
    countdown: Option<u32>,
    cancel: Option<Arc<AtomicBool>>,
    run: u64,
    send_run: u64,
    /// Changes the reply made to its own fields, which are not the user's edits.
    quiet: usize,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ReplyEvent> for MailReply {}
impl Focusable for MailReply {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The picture for an address: for one of the user's own accounts, the photo they chose;
/// else Gravatar's (the initial shows when it has none).
pub(super) fn face(
    name: &str,
    address: &str,
    photo: Option<Arc<Image>>,
    side: f32,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    if let Some(photo) = photo {
        return gpui::img(photo)
            .size(px(side))
            .flex_shrink_0()
            .rounded_full()
            .object_fit(gpui::ObjectFit::Cover)
            .into_any_element();
    }
    let label = if name.trim().is_empty() {
        address
    } else {
        name
    };
    let gravatar = model::gravatar_url(address, 64).unwrap_or_default();
    div()
        .flex_shrink_0()
        .child(super::super::avatar_image(
            "", label, &gravatar, side, window, cx,
        ))
        .into_any_element()
}

struct Logo;
impl gpui::Asset for Logo {
    type Source = String;
    type Output = Option<Arc<Image>>;
    #[expect(
        clippy::manual_async_fn,
        reason = "An async method captures the non-Send App reference."
    )]
    fn load(
        address: String,
        _: &mut App,
    ) -> impl std::future::Future<Output = Self::Output> + Send + 'static {
        async move { model::logo(&address).map(png_image) }
    }
}

/// The organisation's icon beside an address: its own web's, else its initial.
fn organisation(address: &str, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let domain = model::organisation_domain(address)?;
    let logo = window
        .use_asset::<Logo>(&address.trim().to_lowercase(), cx)
        .flatten();
    let mark = div()
        .size(px(16.))
        .flex_shrink_0()
        .rounded_full()
        .overflow_hidden()
        .border_1()
        .border_color(content(0.1))
        .flex()
        .items_center()
        .justify_center();
    Some(match logo {
        Some(logo) => mark
            .bg(background_base())
            .child(
                gpui::img(logo)
                    .size(px(12.))
                    .object_fit(gpui::ObjectFit::Contain),
            )
            .into_any_element(),
        None => mark
            .bg(content(0.15))
            .text_size(px(9.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(content(0.7))
            .child(
                domain
                    .chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
                    .to_string(),
            )
            .into_any_element(),
    })
}

/// A person on a reply: their face, who they are, and their organisation's icon.
fn chip(
    person: &Person,
    photo: Option<Arc<Image>>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let label = model::person_label(&person.name, &person.address);
    let named = label != person.address;
    div()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(face(&label, &person.address, photo, 20., window, cx))
        .when(named, |d| {
            d.child(
                div()
                    .flex_shrink_0()
                    .truncate()
                    .text_color(content(0.85))
                    .child(label.clone()),
            )
        })
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_color(content(if named { 0.45 } else { 0.85 }))
                .child(person.address.clone()),
        )
        .children(organisation(&person.address, window, cx))
        .into_any_element()
}

/// One line of a reply's header: «From», «To» or «Cc», then its people.
fn people_row(label: &'static str, people: Vec<AnyElement>, end: Option<AnyElement>) -> Div {
    div()
        .min_h(px(36.))
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(8.))
        .py(px(4.))
        .text_size(px(12.))
        .child(
            div()
                .w(px(36.))
                .flex_shrink_0()
                .text_color(content(0.45))
                .child(label),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_x(px(12.))
                .gap_y(px(4.))
                .children(people),
        )
        .when_some(end, |d, end| {
            d.child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(end),
            )
        })
}

/// The byte ranges of a text's words, split on whitespace.
fn word_ranges(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut words = Vec::new();
    let mut start = None;
    for (at, c) in text.char_indices() {
        match (c.is_whitespace(), start) {
            (true, Some(from)) => {
                words.push(from..at);
                start = None;
            }
            (false, None) => start = Some(at),
            _ => {}
        }
    }
    if let Some(from) = start {
        words.push(from..text.len());
    }
    words
}

/// Streamed text whose newest words are still coming into focus and wear the accent.
fn faded(text: &str, fades: &WordFades, base: gpui::Hsla) -> StyledText {
    let highlights: Vec<_> = word_ranges(text)
        .into_iter()
        .enumerate()
        .filter_map(|(word, range)| {
            let (opacity, trail) = (fades.opacity(word), fades.trail(word));
            (opacity < 1. || trail > 0.).then(|| {
                let mut color = crate::reveal::trail_color(base, trail);
                color.a *= opacity;
                (
                    range,
                    HighlightStyle {
                        color: Some(color),
                        ..Default::default()
                    },
                )
            })
        })
        .collect();
    StyledText::new(text.to_string()).with_highlights(highlights)
}

/// How bright the light inside the field is, `t` through its 2.4s breath: dimmest halfway,
/// on the web's ease-in-out (`ai-glow-breathe`).
fn breathe(t: f32) -> f32 {
    let ease = |t: f32| crate::reveal::bezier(t, 0.42, 0., 0.58, 1.);
    if t < 0.5 {
        1. - 0.55 * ease(t * 2.)
    } else {
        0.45 + 0.55 * ease((t - 0.5) * 2.)
    }
}

/// The soft light inside the reply's field while AI writes (`.ai-glow::after`).
fn glow(reduce_motion: bool) -> AnyElement {
    let light = div().absolute().inset_0().shadow(vec![
        gpui::BoxShadow::new(px(0.), px(0.), accent_color().opacity(0.22))
            .blur_radius(px(12.))
            .spread_radius(px(1.))
            .inset(),
    ]);
    if reduce_motion {
        return light.into_any_element();
    }
    light
        .with_animation(
            "mail-reply-glow",
            Animation::new(Duration::from_millis(2400)).repeat(),
            |light, t| light.opacity(breathe(t)),
        )
        .into_any_element()
}

/// The reply's small text buttons (`h-7 px-2 text-[12px] text-content/70`).
fn tool_button(id: &'static str, label: impl Into<SharedString>, enabled: bool) -> Stateful<Div> {
    let label: SharedString = label.into();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .h(px(28.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(8.))
        .text_size(px(12.))
        .text_color(content(0.7))
        .when(!enabled, |d| d.opacity(0.4))
        .when(enabled, |d| {
            d.cursor_pointer()
                .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
        })
        .child(label)
}

impl MailReply {
    pub(crate) fn new(
        service: model::Service,
        message: Message,
        account: Option<AccountView>,
        photo: Option<Arc<Image>>,
        opened: Option<Opened>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let first_subject = model::reply_subject(&message.subject);
        let subject = cx.new(|cx| {
            let mut input = Input::command_field("Subject", "Subject", cx);
            input.set_text(first_subject, cx);
            input
        });
        let text = cx.new(|cx| {
            let mut input = Input::commit_message(cx);
            input.set_placeholder("Write your reply, or let AI draft it", cx);
            input.set_font_size(13.);
            input.set_line_height(21.);
            input.set_min_height(63.);
            input.set_max_height(320.);
            input
        });
        let ask = cx.new(|cx| {
            let mut input = Input::field(
                "What AI should write",
                "Ask AI to write it",
                content(0.4),
                cx,
            );
            input.set_font_size(12.);
            input
        });
        let subscriptions = vec![
            cx.subscribe(&subject, |this, _, _: &input::Changed, cx| {
                if !this.quieted() {
                    this.status = None;
                    this.before = None;
                }
                cx.notify();
            }),
            cx.subscribe_in(
                &subject,
                window,
                |this, _, _: &input::Submitted, window, cx| {
                    window.focus(&this.text.focus_handle(cx), cx);
                },
            ),
            cx.subscribe(&text, |this, _, _: &input::Changed, cx| {
                if !this.quieted() {
                    this.status = None;
                    // Once the user edits AI's text it is theirs: no undo over it.
                    this.before = None;
                }
                let placeholder = if this.has_text(cx) {
                    "Ask AI to change it"
                } else {
                    "Ask AI to write it"
                };
                this.ask
                    .update(cx, |input, cx| input.set_placeholder(placeholder, cx));
                cx.notify();
            }),
            cx.subscribe(&text, |this, _, _: &input::Submitted, cx| this.send(cx)),
            cx.subscribe_in(&ask, window, |this, _, _: &input::Submitted, window, cx| {
                if !this.busy() {
                    let request = this.ask_text(cx);
                    this.write(request, false, window, cx);
                }
            }),
            cx.subscribe(&ask, |_, _, _: &input::Changed, cx| cx.notify()),
        ];
        window.focus(&text.focus_handle(cx), cx);
        let mut reply = Self {
            service,
            message,
            account,
            photo,
            opened,
            subject,
            text,
            ask,
            writing: false,
            streamed: None,
            started: Instant::now(),
            arrived: false,
            reveal: None,
            settling: None,
            before: None,
            last_ask: None,
            people: None,
            all: false,
            status: None,
            countdown: None,
            cancel: None,
            run: 0,
            send_run: 0,
            quiet: 0,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        reply.load_people(cx);
        reply
    }

    fn quieted(&mut self) -> bool {
        if self.quiet > 0 {
            self.quiet -= 1;
            true
        } else {
            false
        }
    }

    fn has_text(&self, cx: &App) -> bool {
        !self.text.read(cx).content.trim().is_empty()
    }

    fn ask_text(&self, cx: &App) -> Option<String> {
        let ask = self.ask.read(cx).content.trim().to_string();
        (!ask.is_empty()).then_some(ask)
    }

    fn busy(&self) -> bool {
        self.writing || self.streamed.is_some()
    }

    fn load_people(&mut self, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let id = self.message.id.clone();
        let task = cx
            .background_executor()
            .spawn(async move { service.recipients(&id, true) });
        cx.spawn(async move |this, cx| {
            if let Ok(people) = task.await {
                let _ = this.update(cx, |this, cx| {
                    this.people = Some(people);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Puts AI's words in the fields without counting them as the user's edits.
    fn set_fields(&mut self, body: String, subject: Option<String>, cx: &mut Context<Self>) {
        self.quiet += 1;
        self.text.update(cx, |input, cx| input.set_text(body, cx));
        if let Some(subject) = subject.filter(|s| !s.trim().is_empty()) {
            self.quiet += 1;
            self.subject
                .update(cx, |input, cx| input.set_text(subject, cx));
        }
    }

    /// `retry` writes again from what the box held before AI's last try.
    fn write(
        &mut self,
        request: Option<String>,
        retry: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(cancel.clone());
        self.run += 1;
        let run = self.run;
        let original = match (&self.before, retry) {
            (Some(before), true) => before.clone(),
            _ => (
                self.text.read(cx).content.to_string(),
                self.subject.read(cx).content.to_string(),
            ),
        };
        self.writing = true;
        self.streamed = Some(String::new());
        self.started = Instant::now();
        self.arrived = false;
        self.reveal = None;
        self.settling = None;
        self.status = None;
        self.last_ask = request.clone();
        let prompt = model::reply_prompt(&ReplyInput {
            from: format!("{} <{}>", self.message.from, self.message.address),
            subject: self.message.subject.clone(),
            body: model::mail_text(&self.message, self.opened.as_ref()),
            ask: request,
            draft: Some(original.0.clone()),
            draft_subject: Some(original.1.clone()),
        });
        // The background model picked in Settings, else the first installed harness.
        let chosen = cx
            .try_global::<crate::settings::Preferences>()
            .and_then(|p| p.background_text_model());
        let chosen = chosen.map(|(kind, model)| {
            let home = monocode_core::accounts::home()
                .map(|home| home.to_string_lossy().into_owned())
                .unwrap_or_default();
            TextModel {
                kind,
                model,
                account: crate::accounts::get(cx)
                    .map(|center| center.read(cx).selected(kind, &home))
                    .unwrap_or_default(),
            }
        });
        let (tx, rx) = async_channel::unbounded::<model::ReplyProgress>();
        let task = cx.background_executor().spawn(async move {
            let text = match chosen {
                Some(text) => text,
                None => TextModel::automatic(
                    model::text_harness()
                        .ok_or("Connect an agent (Claude, Codex…) to write with AI.")?,
                    Default::default(),
                ),
            };
            model::write_reply(text, prompt, cancel, |progress| {
                let _ = tx.try_send(progress);
            })
        });
        cx.spawn(async move |this, cx| {
            while let Ok(progress) = rx.recv().await {
                let alive = this.update(cx, |this, cx| {
                    if this.run == run && this.writing {
                        match progress {
                            model::ReplyProgress::Text(so_far) => this.streamed = Some(so_far),
                            model::ReplyProgress::Arrived(arrived) => this.arrived = arrived,
                        }
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.run != run || !this.writing {
                    return;
                }
                this.writing = false;
                this.cancel = None;
                match result {
                    Ok(written) => {
                        // The streamed text stays up until its last word has settled.
                        this.streamed = Some(written.clone());
                        this.settling = Some((written, original));
                        this.settle_when_revealed(run, window, cx);
                    }
                    Err(error) => {
                        this.streamed = None;
                        this.reveal = None;
                        this.status = Some((error, true));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// The field lights while AI writes, and goes out once the model's message has arrived
    /// and its last word has come onto the screen, not when the harness exits.
    fn glowing(&self, now: Instant) -> bool {
        self.writing && !(self.arrived && !self.revealing(now))
    }

    fn revealing(&self, now: Instant) -> bool {
        self.reveal
            .as_ref()
            .is_some_and(|(subject, body)| subject.active(now) || body.active(now))
    }

    fn settle_when_revealed(&mut self, run: u64, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                let done = this
                    .update_in(cx, |this, window, cx| {
                        if this.run != run || this.settling.is_none() {
                            return true;
                        }
                        if this.revealing(Instant::now()) {
                            return false;
                        }
                        this.settle(window, cx);
                        true
                    })
                    .unwrap_or(true);
                if done {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
            }
        })
        .detach();
    }

    /// The last word has settled: the text becomes the editable box in its place.
    fn settle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((written, original)) = self.settling.take() else {
            return;
        };
        let reply = model::split_subject(&written);
        self.set_fields(reply.body, reply.subject, cx);
        self.before = Some(original);
        self.streamed = None;
        self.reveal = None;
        window.focus(&self.text.focus_handle(cx), cx);
        cx.notify();
    }

    /// Stops the model and keeps what it wrote so far, still undoable.
    fn stop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.writing {
            return;
        }
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        let streamed = model::split_subject(self.streamed.take().as_deref().unwrap_or_default());
        if !streamed.body.is_empty() {
            self.before = Some((
                self.text.read(cx).content.to_string(),
                self.subject.read(cx).content.to_string(),
            ));
            self.set_fields(streamed.body, streamed.subject, cx);
        }
        self.writing = false;
        self.reveal = None;
        self.settling = None;
        window.focus(&self.text.focus_handle(cx), cx);
        cx.notify();
    }

    fn undo_ai(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((text, subject)) = self.before.take() else {
            return;
        };
        self.quiet += 2;
        self.text.update(cx, |input, cx| input.set_text(text, cx));
        self.subject
            .update(cx, |input, cx| input.set_text(subject, cx));
        window.focus(&self.text.focus_handle(cx), cx);
        cx.notify();
    }

    fn select_next_gap(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (gaps, caret) = {
            let input = self.text.read(cx);
            (model::reply_gaps(&input.content), input.selection().end)
        };
        if let Some(gap) = model::next_gap(&gaps, caret) {
            window.focus(&self.text.focus_handle(cx), cx);
            self.text
                .update(cx, |input, cx| input.select_range(gap, cx));
        }
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        if !self.has_text(cx) || self.countdown.is_some() || self.busy() {
            return;
        }
        self.send_run += 1;
        let run = self.send_run;
        self.countdown = Some(UNDO_SECONDS);
        self.status = None;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let go = this
                    .update(cx, |this, cx| {
                        if this.send_run != run {
                            return false;
                        }
                        let left = this.countdown.unwrap_or(1).saturating_sub(1);
                        if left > 0 {
                            this.countdown = Some(left);
                            cx.notify();
                            return true;
                        }
                        this.countdown = None;
                        this.status = Some(("Sending…".into(), false));
                        this.deliver(cx);
                        false
                    })
                    .unwrap_or(false);
                if !go {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn deliver(&mut self, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let id = self.message.id.clone();
        let text = self.text.read(cx).content.to_string();
        let subject = self.subject.read(cx).content.to_string();
        let all = self.all;
        let task = cx
            .background_executor()
            .spawn(async move { service.send(&id, &text, Some(&subject), all) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => cx.emit(ReplyEvent::Sent),
                    Err(error) => this.status = Some((error, true)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn undo_send(&mut self, cx: &mut Context<Self>) {
        self.send_run += 1;
        self.countdown = None;
        self.status = Some(("Not sent.".into(), false));
        cx.notify();
    }

    fn save_draft(&mut self, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let id = self.message.id.clone();
        let text = self.text.read(cx).content.to_string();
        let subject = self.subject.read(cx).content.to_string();
        let all = self.all;
        let task = cx
            .background_executor()
            .spawn(async move { service.draft(&id, &text, Some(&subject), all) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.status = Some(match result {
                    Ok(()) => ("Saved as a draft on your mail server.".into(), false),
                    Err(error) => (error, true),
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn people_box(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let fallback = Person {
            name: self.message.from.clone(),
            address: self.message.address.clone(),
        };
        let to = self
            .people
            .as_ref()
            .filter(|p| !p.to.is_empty())
            .map(|p| p.to.clone())
            .unwrap_or_else(|| vec![fallback]);
        let copies = self
            .people
            .as_ref()
            .map(|p| p.cc.clone())
            .unwrap_or_default();
        let from: Vec<AnyElement> = match &self.account {
            Some(account) => vec![chip(
                &Person {
                    name: account.name.clone(),
                    address: account.address.clone(),
                },
                self.photo.clone(),
                window,
                cx,
            )],
            None => vec![
                div()
                    .text_color(content(0.45))
                    .child("Your account")
                    .into_any_element(),
            ],
        };
        let close = tool("mail-reply-close", "Close", "x", content(0.5))
            .size(px(24.))
            .on_click(cx.listener(|_, _, _, cx| cx.emit(ReplyEvent::Close)))
            .into_any_element();
        let all = self.all;
        let reply_all = (!copies.is_empty()).then(|| {
            div()
                .id("mail-reply-all")
                .role(Role::Switch)
                .aria_label("Reply all")
                .aria_toggled(if all {
                    gpui::Toggled::True
                } else {
                    gpui::Toggled::False
                })
                .px(px(6.))
                .py(px(2.))
                .cursor_pointer()
                .when(all, |d| {
                    d.bg(accent_color().opacity(0.12))
                        .text_color(accent_color())
                })
                .when(!all, |d| {
                    d.text_color(content(0.55))
                        .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                })
                .child("Reply all")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.all = !this.all;
                    cx.notify();
                }))
                .into_any_element()
        });
        let to: Vec<AnyElement> = to
            .iter()
            .map(|person| chip(person, None, window, cx))
            .collect();
        let cc: Vec<AnyElement> = if all {
            copies
                .iter()
                .map(|person| chip(person, None, window, cx))
                .collect()
        } else {
            Vec::new()
        };
        div()
            .flex()
            .flex_col()
            .border_1()
            .border_color(content(0.1))
            .child(people_row("From", from, Some(close)))
            .child(
                people_row("To", to, reply_all)
                    .border_t_1()
                    .border_color(content(0.08)),
            )
            .when(!cc.is_empty(), |d| {
                d.child(
                    people_row("Cc", cc, None)
                        .border_t_1()
                        .border_color(content(0.08)),
                )
            })
            .into_any_element()
    }

    /// Paces what AI has written so far: words come out and into focus, wearing the accent
    /// as a trail, as a chat reply streams (`crate::reveal`).
    fn step_reveal(&mut self, window: &mut Window, cx: &App) {
        let Some(streamed) = self.streamed.as_deref() else {
            self.reveal = None;
            return;
        };
        let now = Instant::now();
        let animate = !cx.reduce_motion();
        let split = model::split_subject(streamed);
        let (subject, body) = self.reveal.get_or_insert_with(|| {
            (
                PacedText::new("", animate, now),
                PacedText::new("", animate, now),
            )
        });
        for (paced, text) in [
            (&mut *subject, split.subject.as_deref().unwrap_or_default()),
            (&mut *body, split.body.as_str()),
        ] {
            paced.update(text, now);
            paced.step(now, self.writing);
            let words = word_ranges(paced.text()).len();
            paced.words(words, now);
        }
        if self.revealing(now) {
            window.request_animation_frame();
        }
    }

    fn editor(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let streaming = self.streamed.as_deref().map(model::split_subject);
        let gaps = if streaming.is_some() {
            0
        } else {
            model::reply_gaps(&self.text.read(cx).content).len()
        };
        let ai_wrote = self.before.is_some()
            && !self.busy()
            && self.countdown.is_none()
            && self.status.is_none();
        let focused = self.text.focus_handle(cx).contains_focused(window, cx)
            || self.subject.focus_handle(cx).contains_focused(window, cx);
        let subject_text = |text: AnyElement| {
            div()
                .w_full()
                .px(px(10.))
                .pt(px(10.))
                .text_size(px(17.))
                .line_height(px(24.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(content(1.))
                .child(text)
        };
        let shown = |pick: fn(&(PacedText, PacedText)) -> &PacedText, base: gpui::Hsla| {
            self.reveal.as_ref().map(|reveal| {
                let paced = pick(reveal);
                faded(paced.text(), &paced.fades, base)
            })
        };
        let fields: AnyElement = match streaming {
            Some(streamed) => div()
                .flex()
                .flex_col()
                .child(match streamed.subject.as_deref() {
                    Some(subject) if !subject.is_empty() => subject_text(
                        div()
                            .child(
                                shown(|r| &r.0, content(1.))
                                    .unwrap_or_else(|| StyledText::new(subject.to_string())),
                            )
                            .into_any_element(),
                    ),
                    _ if streamed.body.is_empty() => subject_text(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(13.))
                            .font_weight(FontWeight::NORMAL)
                            .child(icon("sparkles", 14.).text_color(accent_color()))
                            .child(crate::activity::shimmer(
                                "Reading the email…".into(),
                                self.started.elapsed().as_secs_f32(),
                            ))
                            .into_any_element(),
                    ),
                    _ => subject_text(
                        div()
                            .child(self.subject.read(cx).content.clone())
                            .into_any_element(),
                    ),
                })
                .child(
                    div()
                        .id("mail-reply-streamed")
                        .min_h(px(80.))
                        .max_h(px(320.))
                        .overflow_y_scroll()
                        .px(px(10.))
                        .py(px(8.))
                        .text_size(px(13.))
                        .line_height(px(21.))
                        .text_color(content(0.9))
                        .child(
                            shown(|r| &r.1, content(0.9))
                                .unwrap_or_else(|| StyledText::new(streamed.body)),
                        ),
                )
                .into_any_element(),
            None => div()
                .flex()
                .flex_col()
                .child(div().px(px(10.)).pt(px(10.)).child(self.subject.clone()))
                .child(
                    div()
                        .px(px(10.))
                        .py(px(8.))
                        .when(ai_wrote || gaps > 0, |d| d.pb(px(40.)))
                        .child(self.text.clone()),
                )
                .into_any_element(),
        };
        // The border lights while words are still coming, and goes out with the last of them.
        let glowing = self.glowing(Instant::now());
        div()
            .relative()
            .border_1()
            .border_color(if glowing {
                accent_color().opacity(0.)
            } else if focused {
                content(0.25)
            } else {
                content(0.1)
            })
            .when(glowing, |d| {
                d.shadow(vec![
                    gpui::BoxShadow::new(px(0.), px(0.), accent_color().opacity(0.6))
                        .spread_radius(px(1.))
                        .inset(),
                ])
                .child(glow(cx.reduce_motion()))
            })
            .child(fields)
            .when(ai_wrote || gaps > 0, |d| {
                d.child(
                    div()
                        .absolute()
                        .left(px(6.))
                        .right(px(6.))
                        .bottom(px(6.))
                        .flex()
                        .items_end()
                        .gap(px(8.))
                        .when(ai_wrote, |d| {
                            d.child(
                                div()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .gap(px(2.))
                                    .py(px(2.))
                                    .pl(px(6.))
                                    .pr(px(2.))
                                    .border_1()
                                    .border_color(content(0.1))
                                    .bg(background_base())
                                    .text_size(px(11.))
                                    .text_color(content(0.55))
                                    .child(icon("sparkles", 12.).text_color(accent_color()))
                                    .child(div().mx(px(4.)).truncate().child("Written with AI"))
                                    .child(
                                        div()
                                            .id("mail-reply-undo-ai")
                                            .role(Role::Button)
                                            .aria_label("Undo")
                                            .flex()
                                            .items_center()
                                            .gap(px(4.))
                                            .px(px(6.))
                                            .py(px(2.))
                                            .text_color(content(0.7))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                                            .child(icon("undo", 12.))
                                            .child("Undo")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.undo_ai(window, cx)
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("mail-reply-retry")
                                            .role(Role::Button)
                                            .aria_label("Try again")
                                            .px(px(6.))
                                            .py(px(2.))
                                            .text_color(content(0.7))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                                            .child("Try again")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                let ask = this.last_ask.clone();
                                                this.write(ask, true, window, cx)
                                            })),
                                    ),
                            )
                        })
                        .when(gaps > 0, |d| {
                            d.child(
                                div()
                                    .id("mail-reply-gaps")
                                    .role(Role::Button)
                                    .aria_label("Select the next blank to fill in")
                                    .ml_auto()
                                    .flex_shrink_0()
                                    .px(px(6.))
                                    .py(px(2.))
                                    .border_1()
                                    .border_color(accent_color().opacity(0.3))
                                    .bg(background_base())
                                    .text_size(px(11.))
                                    .text_color(accent_color())
                                    .cursor_pointer()
                                    .hover(|s| s.bg(accent_color().opacity(0.1)))
                                    .child(format!("{gaps} to fill"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.select_next_gap(window, cx)
                                    })),
                            )
                        }),
                )
            })
            .into_any_element()
    }

    fn tools(&self, cx: &mut Context<Self>) -> AnyElement {
        let busy = self.busy();
        let has_text = self.has_text(cx);
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(4.))
            .child(
                div()
                    .relative()
                    .h(px(28.))
                    .min_w(px(144.))
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(8.))
                    .bg(content(0.05))
                    .child(icon("sparkles", 14.).text_color(accent_color()))
                    .child(div().min_w_0().flex_1().child(self.ask.clone())),
            )
            .child(if self.writing {
                tool_button("mail-reply-stop", "Stop", true)
                    .child(icon("square", 12.))
                    .on_click(cx.listener(|this, _, window, cx| this.stop(window, cx)))
                    .into_any_element()
            } else {
                tool_button(
                    "mail-reply-write",
                    if has_text { "Rewrite" } else { "Write" },
                    !busy,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    if !this.busy() {
                        let request = this.ask_text(cx);
                        this.write(request, false, window, cx);
                    }
                }))
                .into_any_element()
            })
            .when(has_text && !busy, |d| {
                d.children(model::QUICK_CHANGES.into_iter().map(|(label, ask)| {
                    tool_button(
                        match label {
                            "Shorter" => "mail-reply-shorter",
                            "Warmer" => "mail-reply-warmer",
                            _ => "mail-reply-fix",
                        },
                        label,
                        true,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.write(Some(ask.to_string()), false, window, cx)
                    }))
                }))
            })
            .child(
                div()
                    .ml_auto()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .map(|d| {
                        if self.countdown.is_some() {
                            d.child(
                                tool_button("mail-reply-undo-send", "Undo", true)
                                    .on_click(cx.listener(|this, _, _, cx| this.undo_send(cx))),
                            )
                        } else {
                            let ready = has_text && !busy;
                            d.child(tool_button("mail-reply-draft", "Draft", ready).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    if ready {
                                        this.save_draft(cx)
                                    }
                                }),
                            ))
                            .child(
                                primary("mail-reply-send", "Send", ready)
                                    .child(
                                        icon("mono-send", 14.).text_color(
                                            primary_action(ready)
                                                .map(|(_, ink)| ink)
                                                .unwrap_or_else(|| floating_fill().opacity(1.)),
                                        ),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| this.send(cx))),
                            )
                        }
                    }),
            )
            .into_any_element()
    }
}

impl gpui::Render for MailReply {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.step_reveal(window, cx);
        let status: Option<(String, gpui::Hsla)> = match (&self.countdown, &self.status) {
            (Some(left), _) => Some((format!("Sending in {left}s…"), content(0.55))),
            (None, Some((text, true))) => Some((text.clone(), rgb(0xf87171).into())),
            (None, Some((text, false))) => Some((text.clone(), content(0.5))),
            _ => None,
        };
        div()
            .id("mail-reply")
            .role(Role::Dialog)
            .aria_label(format!("Reply to {}", self.message.from))
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .gap(px(6.))
            .p(px(8.))
            .border_1()
            .border_color(content(0.1))
            .bg(background_base())
            .shadow(vec![
                gpui::BoxShadow::new(px(0.), px(12.), black(if is_light() { 0.12 } else { 0.35 }))
                    .blur_radius(px(28.))
                    .spread_radius(px(-8.)),
            ])
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    if this.writing {
                        this.stop(window, cx);
                    } else {
                        cx.emit(ReplyEvent::Close);
                    }
                    cx.stop_propagation();
                }
            }))
            .child(self.people_box(window, cx))
            .child(self.editor(window, cx))
            .child(self.tools(cx))
            .when_some(status, |d, (text, ink)| {
                d.child(
                    div()
                        .px(px(4.))
                        .truncate()
                        .text_size(px(11.))
                        .text_color(ink)
                        .child(text),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[test]
    fn words_and_the_breath_of_the_light() {
        assert_eq!(word_ranges("  Hola  Ana,\nsí "), [2..6, 8..12, 13..16]);
        assert_eq!(breathe(0.), 1.);
        assert!((breathe(0.5) - 0.45).abs() < 1e-3);
        assert!((breathe(1.) - 1.).abs() < 1e-3);
        assert!(breathe(0.25) < 1. && breathe(0.25) > 0.45);
    }

    #[gpui::test]
    fn the_field_lights_until_the_message_arrives_and_its_last_word_settles(
        cx: &mut TestAppContext,
    ) {
        let message: Message = serde_json::from_value(serde_json::json!({
            "id": "a.inbox.1", "accountId": "a", "folder": "inbox", "from": "Ana",
            "address": "ana@mina.pe", "subject": "Pozas", "preview": "", "body": [],
            "at": 0, "unread": false, "starred": false, "triage": "routine", "labels": []
        }))
        .unwrap();
        let service = model::Service::new(std::env::temp_dir().join("monobot-native-reply"));
        let (reply, cx) = cx.add_window_view(|window, cx| {
            MailReply::new(service, message, None, None, None, window, cx)
        });
        reply.update(cx, |reply, cx| {
            let now = Instant::now();
            assert!(!reply.glowing(now));
            reply.writing = true;
            reply.streamed = Some("Subject: Sí\n\nHola Ana".into());
            assert!(reply.glowing(now), "lit while words come");
            reply.arrived = true;
            assert!(
                !reply.glowing(now),
                "out once arrived with nothing left to reveal"
            );
            reply.reveal = Some((
                PacedText::new("", true, now),
                PacedText::new("Hola Ana", true, now),
            ));
            assert!(
                reply.glowing(now),
                "still lit while the last words come out"
            );
            assert_eq!(
                reply.subject.read(cx).content.as_ref(),
                "Re: Pozas",
                "the subject starts as «Re: » and theirs"
            );
        });
    }
}
