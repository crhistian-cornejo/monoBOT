//! A new message, or the forward of one, from one of the user's accounts: who gets it, a
//! copy, the subject and the text. It leaves only on the user's press (Send), or waits on
//! the account's server as a draft. A forward carries the message's text, not its files.
use super::*;

pub(crate) enum ComposeEvent {
    Close,
    /// It left: what it said, for the notice.
    Sent(String),
}

pub(crate) struct MailCompose {
    service: model::Service,
    accounts: Vec<AccountView>,
    /// The account it goes from.
    from: usize,
    to: Entity<Input>,
    cc: Entity<Input>,
    subject: Entity<Input>,
    text: Entity<Input>,
    /// The message it forwards, to say so above the fields.
    forwarding: Option<String>,
    busy: bool,
    status: Option<(String, bool)>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ComposeEvent> for MailCompose {}
impl Focusable for MailCompose {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl MailCompose {
    /// `from`: the account it starts on; `forward`: the subject and text of the message it
    /// forwards.
    pub(crate) fn new(
        service: model::Service,
        accounts: Vec<AccountView>,
        from: Option<&str>,
        forward: Option<(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let field = |label: &str, cx: &mut Context<Self>| {
            let label = label.to_owned();
            cx.new(move |cx| Input::command_field(&label, &label, cx))
        };
        let to = field("To", cx);
        let cc = field("Cc", cx);
        let subject = field("Subject", cx);
        let text = cx.new(|cx| {
            let mut input = Input::commit_message(cx);
            input.set_placeholder("Write your message", cx);
            input.set_font_size(13.);
            input.set_line_height(21.);
            input.set_min_height(160.);
            input.set_max_height(420.);
            input
        });
        let forwarding = forward.as_ref().map(|(subject, _)| subject.clone());
        if let Some((title, body)) = forward {
            subject.update(cx, |input, cx| input.set_text(title, cx));
            text.update(cx, |input, cx| input.set_text(body, cx));
        }
        let mut subscriptions = Vec::new();
        for input in [&to, &cc, &subject, &text] {
            subscriptions.push(cx.subscribe(input, |this, _, _: &input::Changed, cx| {
                this.status = None;
                cx.notify();
            }));
        }
        for (from, next) in [(&to, &cc), (&cc, &subject), (&subject, &text)] {
            let next = next.clone();
            subscriptions.push(cx.subscribe_in(
                from,
                window,
                move |_, _, _: &input::Submitted, window, cx| {
                    window.focus(&next.focus_handle(cx), cx);
                },
            ));
        }
        window.focus(&to.focus_handle(cx), cx);
        let from = from
            .and_then(|id| accounts.iter().position(|a| a.id == id))
            .unwrap_or(0);
        Self {
            service,
            accounts,
            from,
            to,
            cc,
            subject,
            text,
            forwarding,
            busy: false,
            status: None,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    /// The message as the fields hold it, its addresses checked.
    fn outgoing(&self, cx: &App) -> Result<(String, model::Outgoing), String> {
        let read = |input: &Entity<Input>| input.read(cx).content.to_string();
        let account = self
            .accounts
            .get(self.from)
            .ok_or("Connect an account first.")?;
        Ok((
            account.id.clone(),
            model::Outgoing {
                to: model::parse_people(&read(&self.to))?,
                cc: model::parse_people(&read(&self.cc))?,
                subject: read(&self.subject),
                text: read(&self.text),
            },
        ))
    }

    pub(crate) fn busy(&self) -> bool {
        self.busy
    }

    fn ready(&self, cx: &App) -> bool {
        !self.busy
            && !self.to.read(cx).content.trim().is_empty()
            && !self.text.read(cx).content.trim().is_empty()
    }

    /// Sends it (`draft`: saves it on the account's server instead).
    fn finish(&mut self, draft: bool, cx: &mut Context<Self>) {
        if !self.ready(cx) {
            return;
        }
        let (account, out) = match self.outgoing(cx) {
            Ok(parts) => parts,
            Err(error) => {
                self.status = Some((error, true));
                cx.notify();
                return;
            }
        };
        self.busy = true;
        self.status = None;
        let service = self.service.clone();
        let to = out
            .to
            .iter()
            .map(|p| p.address.clone())
            .collect::<Vec<_>>()
            .join(", ");
        let task = cx.background_executor().spawn(async move {
            if draft {
                service.draft_new(&account, &out)
            } else {
                service.send_new(&account, &out)
            }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(()) if draft => {
                        this.status = Some(("Saved in Drafts on the server.".into(), false))
                    }
                    Ok(()) => cx.emit(ComposeEvent::Sent(format!("Sent to {to}."))),
                    Err(error) => this.status = Some((error, true)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn row(label: &'static str, input: &Entity<Input>) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .min_h(px(36.))
            .px(px(16.))
            .border_b_1()
            .border_color(content(0.07))
            .child(
                div()
                    .w(px(56.))
                    .flex_shrink_0()
                    .text_size(px(12.))
                    .text_color(content(0.5))
                    .child(label),
            )
            .child(div().min_w_0().flex_1().child(input.clone()))
    }
}

impl Render for MailCompose {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ready = self.ready(cx);
        let from = self
            .accounts
            .get(self.from)
            .map(|a| model::person_label(&a.name, &a.address) + &format!(" <{}>", a.address))
            .unwrap_or_default();
        let several = self.accounts.len() > 1;
        div()
            .id("mail-compose")
            .debug_selector(|| "mail-compose".into())
            .key_context("MailCompose")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && !this.busy {
                    cx.emit(ComposeEvent::Close);
                    cx.stop_propagation();
                }
            }))
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(40.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .border_b_1()
                    .border_color(content(0.07))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(content(1.))
                            .child(if self.forwarding.is_some() {
                                "Forward"
                            } else {
                                "New message"
                            }),
                    )
                    .child(
                        tool("mail-compose-close", "Close", "close", content(0.5)).on_click(
                            cx.listener(|this, _, _, cx| {
                                if !this.busy {
                                    cx.emit(ComposeEvent::Close)
                                }
                            }),
                        ),
                    ),
            )
            .child(
                div()
                    .id("mail-compose-from")
                    .role(Role::Button)
                    .aria_label("From")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(36.))
                    .px(px(16.))
                    .border_b_1()
                    .border_color(content(0.07))
                    .text_size(px(12.))
                    .child(div().w(px(56.)).text_color(content(0.5)).child("From"))
                    .child(div().min_w_0().flex_1().truncate().child(from))
                    .when(several, |d| {
                        d.cursor_pointer()
                            .child(icon("chevrons-up-down", 12.))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.from = (this.from + 1) % this.accounts.len().max(1);
                                cx.notify();
                            }))
                    }),
            )
            .child(Self::row("To", &self.to))
            .child(Self::row("Cc", &self.cc))
            .child(Self::row("Subject", &self.subject))
            .child(
                div()
                    .id("mail-compose-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(16.))
                    .py(px(12.))
                    .child(self.text.clone()),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(10.))
                    .border_t_1()
                    .border_color(content(0.07))
                    .child(
                        primary(
                            "mail-compose-send",
                            if self.busy { "Sending…" } else { "Send" },
                            ready,
                        )
                        .debug_selector(|| "mail-compose-send".into())
                        .on_click(cx.listener(|this, _, _, cx| this.finish(false, cx))),
                    )
                    .child(
                        text_button("mail-compose-draft", "Save as draft", 0.7)
                            .on_click(cx.listener(|this, _, _, cx| this.finish(true, cx))),
                    )
                    .when(self.forwarding.is_some(), |d| {
                        d.child(
                            div()
                                .text_size(px(11.))
                                .text_color(content(0.45))
                                .child("Its text goes along; its files do not."),
                        )
                    })
                    .child(div().flex_1())
                    .when_some(self.status.clone(), |d, (text, error)| {
                        d.child(if error {
                            alert(text)
                        } else {
                            div()
                                .text_size(px(12.))
                                .text_color(content(0.6))
                                .child(text)
                        })
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn a_forward_starts_with_its_subject_and_text_and_checks_every_address(
        cx: &mut TestAppContext,
    ) {
        let accounts = vec![AccountView {
            id: "a".into(),
            name: "Ana".into(),
            address: "ana@gmail.com".into(),
            provider: "gmail".into(),
        }];
        let (compose, cx) = cx.add_window_view(|window, cx| {
            MailCompose::new(
                model::Service::new(std::env::temp_dir().join("monobot-native-compose")),
                accounts,
                Some("a"),
                Some(("Fwd: Factura".into(), "\n\nHola".into())),
                window,
                cx,
            )
        });
        compose.update(cx, |compose, cx| {
            assert_eq!(compose.subject.read(cx).content.to_string(), "Fwd: Factura");
            assert!(!compose.ready(cx), "it needs someone to send it to");
            compose.to.update(cx, |input, cx| {
                input.set_text("Luis <luis@x.test>, no".into(), cx)
            });
            assert!(compose.ready(cx));
            assert!(compose.outgoing(cx).is_err());
            compose.to.update(cx, |input, cx| {
                input.set_text("Luis <luis@x.test>".into(), cx)
            });
            let (account, out) = compose.outgoing(cx).unwrap();
            assert_eq!(account, "a");
            assert_eq!(out.to[0].address, "luis@x.test");
            assert!(out.cc.is_empty());
        });
    }
}
