//! The reply to a message (the desktop app's MailReply and MailPeople): who it goes from
//! and to, with faces; a subject and text AI can write as the user watches; and Send, with
//! a few seconds to undo. Who it goes to is the core's to decide; nothing leaves without
//! the user's press.
use super::*;
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
        let (tx, rx) = async_channel::unbounded::<String>();
        let task = cx.background_executor().spawn(async move {
            let text = match chosen {
                Some(text) => text,
                None => TextModel::automatic(
                    model::text_harness()
                        .ok_or("Connect an agent (Claude, Codex…) to write with AI.")?,
                    Default::default(),
                ),
            };
            model::write_reply(text, prompt, cancel, |so_far| {
                let _ = tx.try_send(so_far);
            })
        });
        cx.spawn(async move |this, cx| {
            while let Ok(so_far) = rx.recv().await {
                let alive = this.update(cx, |this, cx| {
                    if this.run == run && this.writing {
                        this.streamed = Some(so_far);
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
                this.streamed = None;
                match result {
                    Ok(written) => {
                        let reply = model::split_subject(&written);
                        this.set_fields(reply.body, reply.subject, cx);
                        this.before = Some(original);
                        window.focus(&this.text.focus_handle(cx), cx);
                    }
                    Err(error) => this.status = Some((error, true)),
                }
                cx.notify();
            });
        })
        .detach();
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
        let fields: AnyElement = match streaming {
            Some(streamed) => div()
                .flex()
                .flex_col()
                .child(match streamed.subject.as_deref() {
                    Some(subject) if !subject.is_empty() => {
                        subject_text(div().child(subject.to_string()).into_any_element())
                    }
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
                        .child(streamed.body),
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
        div()
            .relative()
            .border_1()
            .border_color(if self.writing {
                accent_color()
            } else if focused {
                content(0.25)
            } else {
                content(0.1)
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
