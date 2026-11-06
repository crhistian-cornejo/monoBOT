//! Connect a mail account with an app password, or remove one (the desktop app's
//! MailAccounts). The password goes straight to the system's store; MonoBOT only reads mail
//! and sends the replies the user sends.
use super::*;

const PROVIDERS: [(&str, &str); 3] = [
    ("gmail", "Gmail"),
    ("icloud", "iCloud"),
    ("imap", "Other (IMAP)"),
];

/// How to make the app password each provider asks for.
fn steps(provider: &str) -> (&'static str, Option<(&'static str, &'static str)>) {
    match provider {
        "gmail" => (
            "Gmail needs 2-Step Verification on, then an app password: create one named MonoBOT and paste its 16 letters here.",
            Some((
                "Create a Gmail app password",
                "https://myaccount.google.com/apppasswords",
            )),
        ),
        "icloud" => (
            "Sign in to your Apple Account, open Sign-In and Security, then App-Specific Passwords, and create one for MonoBOT.",
            Some((
                "Open your Apple Account",
                "https://account.apple.com/account/manage",
            )),
        ),
        _ => (
            "Use your mail server's IMAP address (port 993, TLS) and the password or app password your provider gives for mail apps.",
            None,
        ),
    }
}

pub(super) struct Form {
    provider: &'static str,
    address: Entity<Input>,
    password: Entity<Input>,
    host: Entity<Input>,
    name: Entity<Input>,
    pub(super) busy: bool,
    error: Option<String>,
}

impl Form {
    pub(super) fn new(cx: &mut Context<Mailbox>) -> Self {
        let field = |label: &str, placeholder: &str, cx: &mut Context<Mailbox>| {
            let label = label.to_owned();
            let placeholder = placeholder.to_owned();
            cx.new(move |cx| Input::field(label, placeholder, content(0.35), cx))
        };
        let password = field("App password", "", cx);
        password.update(cx, |input, _| input.set_secret(true));
        Self {
            provider: "gmail",
            address: field("Email address", "", cx),
            password,
            host: field("IMAP server", "imap.example.com", cx),
            name: field("Name to show (optional)", "", cx),
            busy: false,
            error: None,
        }
    }

    pub(super) fn subscriptions(&self, cx: &mut Context<Mailbox>) -> Vec<Subscription> {
        let mut subscriptions = Vec::new();
        for input in [&self.address, &self.password, &self.host, &self.name] {
            subscriptions.push(cx.subscribe(input, |_, _, _: &input::Changed, cx| cx.notify()));
            subscriptions
                .push(cx.subscribe(input, |this, _, _: &input::Submitted, cx| this.connect(cx)));
        }
        subscriptions
    }

    fn ready(&self, cx: &App) -> bool {
        !self.busy
            && !self.address.read(cx).content.trim().is_empty()
            && !self.password.read(cx).content.trim().is_empty()
    }
}

const LABEL: f32 = 0.7;

/// A labelled field: `text-[12px] text-content/70` over an `h-9` bordered box.
fn labelled(label: &'static str, input: &Entity<Input>, window: &Window, cx: &App) -> AnyElement {
    let focused = input.focus_handle(cx).is_focused(window);
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(12.))
                .text_color(content(LABEL))
                .child(label),
        )
        .child(
            div()
                .h(px(36.))
                .px(px(10.))
                .flex()
                .items_center()
                .border_1()
                .border_color(content(if focused { 0.25 } else { 0.1 }))
                .bg(background_base())
                .child(div().w_full().child(input.clone())),
        )
        .into_any_element()
}

/// The 36×20 switch of the app's settings.
fn switch(id: &'static str, label: &'static str, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Switch)
        .aria_label(label)
        .aria_toggled(if on {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .relative()
        .h(px(20.))
        .w(px(36.))
        .flex_shrink_0()
        .cursor_pointer()
        .bg(if on { accent_color() } else { content(0.2) })
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(if on { 18. } else { 2. }))
                .size(px(16.))
                .bg(rgb(0xffffff)),
        )
}

impl Mailbox {
    fn connect(&mut self, cx: &mut Context<Self>) {
        if !self.form.ready(cx) {
            return;
        }
        let read = |input: &Entity<Input>| input.read(cx).content.to_string();
        let (address, password, name, host) = (
            read(&self.form.address),
            read(&self.form.password),
            read(&self.form.name),
            read(&self.form.host),
        );
        let provider = self.form.provider;
        self.form.busy = true;
        self.form.error = None;
        let service = self.service.clone();
        let task = cx.background_executor().spawn(async move {
            service.add_account(
                &address,
                &password,
                &name,
                provider,
                (provider == "imap").then_some(host.as_str()),
            )
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.form.busy = false;
                match result {
                    Ok(()) => {
                        for input in [
                            &this.form.address,
                            &this.form.password,
                            &this.form.name,
                            &this.form.host,
                        ] {
                            input.update(cx, |input, cx| input.set_text(String::new(), cx));
                        }
                        this.load_accounts(cx);
                    }
                    Err(error) => this.form.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn remove_account(&mut self, id: String, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let task = cx
            .background_executor()
            .spawn(async move { service.remove_account(&id) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.form.error = Some(error);
                }
                this.overview = None;
                this.close_message(cx);
                this.load_accounts(cx);
            });
        })
        .detach();
    }

    /// The accounts, the pictures switch, and the form that connects another.
    pub(super) fn accounts_view(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let accounts = self.accounts.clone().unwrap_or_default();
        let form = &self.form;
        let (text, link) = steps(form.provider);
        let ready = form.ready(cx);
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .when(!accounts.is_empty(), |d| {
                d.child(div().flex().flex_col().gap(px(4.)).children(
                    accounts.iter().map(|account| {
                        let id = account.id.clone();
                        let photo = self.photos.get(&account.address.to_lowercase()).cloned();
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .px(px(12.))
                            .py(px(8.))
                            .border_1()
                            .border_color(content(0.1))
                            .text_size(px(13.))
                            .child(reply::face(
                                &account.name,
                                &account.address,
                                photo,
                                32.,
                                window,
                                cx,
                            ))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_color(content(1.))
                                            .child(account.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(12.))
                                            .text_color(content(0.5))
                                            .child(account.address.clone()),
                                    ),
                            )
                            .child(
                                tool(
                                    SharedString::from(format!("mail-remove-{}", account.id)),
                                    &format!("Remove {}", account.address),
                                    "trash",
                                    content(0.5),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_account(id.clone(), cx)
                                })),
                            )
                    }),
                ))
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(12.))
                        .px(px(12.))
                        .py(px(8.))
                        .border_1()
                        .border_color(content(0.1))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .text_color(content(1.))
                                        .child("Load pictures automatically"),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(19.))
                                        .text_color(content(0.55))
                                        .child("MonoBOT fetches them for the message, with no cookies. A sender can still tell you opened it. Mail held back as suspicious never loads them on its own."),
                                ),
                        )
                        .child(
                            switch(
                                "mail-auto-images",
                                "Load pictures automatically",
                                self.auto_images,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_auto_images(!this.auto_images, cx)
                            })),
                        ),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .id("mail-providers")
                            .role(Role::RadioGroup)
                            .aria_label("Mail provider")
                            .flex()
                            .gap(px(4.))
                            .children(PROVIDERS.into_iter().map(|(provider, label)| {
                                let chosen = form.provider == provider;
                                div()
                                    .id(SharedString::from(format!("mail-provider-{provider}")))
                                    .role(Role::RadioButton)
                                    .aria_label(label)
                                    .px(px(12.))
                                    .py(px(6.))
                                    .text_size(px(12.))
                                    .cursor_pointer()
                                    .text_color(content(if chosen { 1. } else { 0.6 }))
                                    .when(chosen, |d| d.bg(content(0.1)))
                                    .when(!chosen, |d| d.hover(|s| s.bg(content(0.08))))
                                    .child(label)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.form.provider = provider;
                                        cx.notify();
                                    }))
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_x(px(4.))
                            .text_size(px(12.))
                            .line_height(px(19.))
                            .text_color(content(0.6))
                            .child(text)
                            .when_some(link, |d, (label, url)| {
                                d.child(
                                    div()
                                        .id("mail-provider-link")
                                        .role(Role::Link)
                                        .aria_label(label)
                                        .text_color(accent_color())
                                        .cursor_pointer()
                                        .hover(|s| s.underline())
                                        .child(label)
                                        .on_click(move |_, _, cx| cx.open_url(url)),
                                )
                            }),
                    )
                    .child(labelled("Email address", &form.address, window, cx))
                    .child(labelled("App password", &form.password, window, cx))
                    .when(form.provider == "imap", |d| {
                        d.child(labelled("IMAP server", &form.host, window, cx))
                    })
                    .child(labelled("Name to show (optional)", &form.name, window, cx))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(content(0.45))
                            .child("The password stays in your system's keychain. MonoBOT reads your mail and sends only the replies you send yourself; it never deletes mail."),
                    )
                    .when_some(form.error.clone(), |d, error| d.child(alert(error)))
                    .child(
                        div().child(
                            primary(
                                "mail-connect",
                                if form.busy {
                                    "Connecting…"
                                } else {
                                    "Connect account"
                                },
                                ready,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.connect(cx))),
                        ),
                    ),
            )
            .into_any_element()
    }
}
