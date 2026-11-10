//! Connecting a mail account: the Email tab's form and the dialog a bot opens with
//! `mail.connect` are this one view. The owner of the mailbox types its app password here;
//! the server has to take it, and then it goes straight to the system's store of secrets
//! (Keychain on macOS, Credential Manager on Windows), never to a file, the chat or a model.
//! Who may read the account is chosen here too, and only here or in the Email tab.
use super::{alert, primary};
use crate::{
    input::{self, Input},
    modal,
    theme::*,
};
use gpui::{
    AnyElement, App, AppContext, Context, Div, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight, KeyDownEvent, Role, SharedString, Stateful, Subscription, Window, div, prelude::*,
    px,
};
use monocode_core::inbox::mail as model;

const PROVIDERS: [(&str, &str); 3] = [
    ("gmail", "Gmail"),
    ("icloud", "iCloud"),
    ("imap", "Other (IMAP)"),
];

/// How to make the app password each provider asks for.
fn steps(provider: &str) -> (&'static str, Option<(&'static str, &'static str)>) {
    match provider {
        "gmail" => (
            "Gmail needs 2-Step Verification on, then an app password: create one named monoBOT and paste its 16 letters here.",
            Some((
                "Create a Gmail app password",
                "https://myaccount.google.com/apppasswords",
            )),
        ),
        "icloud" => (
            "Sign in to your Apple Account, open Sign-In and Security, then App-Specific Passwords, and create one for monoBOT.",
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

/// Where the password goes, in the words of the system the user is on.
fn keychain_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "your Mac's Keychain"
    } else if cfg!(windows) {
        "Windows Credential Manager"
    } else {
        "your system's keyring"
    }
}

/// An agent that may be given an account to read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Agent {
    pub id: String,
    pub label: String,
}

/// The agents a mailbox can be kept for: the user's bots, Principal first.
pub(crate) fn agents_of(roster: &monocode_core::mono::Roster) -> Vec<Agent> {
    let mut bots: Vec<&monocode_core::mono::Mono> =
        roster.monos.iter().filter(|m| m.is_bot()).collect();
    bots.sort_by_key(|m| !m.is_principal());
    bots.into_iter()
        .map(|m| Agent {
            id: m.id.clone(),
            label: m.label(),
        })
        .collect()
}

/// Who reads an account: every agent, or only some.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Readers {
    All,
    Only(Vec<String>),
}

impl Readers {
    pub(crate) fn of(access: &model::Access, address: &str) -> Self {
        model::readers_of(access, address)
            .map(|ids| Self::Only(ids.clone()))
            .unwrap_or(Self::All)
    }

    /// As the core keeps it: `None` for every agent.
    pub(crate) fn ids(&self) -> Option<Vec<String>> {
        match self {
            Self::All => None,
            Self::Only(ids) => Some(ids.clone()),
        }
    }

    /// `id` pressed: it joins or leaves the readers. The last one cannot leave (the
    /// account would be read by nobody); «All agents» opens it again.
    pub(crate) fn toggled(&self, id: &str) -> Self {
        match self {
            Self::All => Self::Only(vec![id.to_owned()]),
            Self::Only(ids) if ids.iter().any(|x| x == id) => {
                if ids.len() == 1 {
                    self.clone()
                } else {
                    Self::Only(ids.iter().filter(|x| *x != id).cloned().collect())
                }
            }
            Self::Only(ids) => {
                let mut ids = ids.clone();
                ids.push(id.to_owned());
                Self::Only(ids)
            }
        }
    }

    fn reads(&self, id: &str) -> bool {
        matches!(self, Self::Only(ids) if ids.iter().any(|x| x == id))
    }

    /// «All agents», or the names of the ones that read it.
    pub(crate) fn summary(&self, agents: &[Agent]) -> String {
        match self {
            Self::All => "All agents".into(),
            Self::Only(ids) => ids
                .iter()
                .map(|id| {
                    agents
                        .iter()
                        .find(|a| &a.id == id)
                        .map(|a| a.label.clone())
                        .unwrap_or_else(|| "an agent no longer here".into())
                })
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}

/// The small selectable pill of the provider and reader rows.
fn pill(id: SharedString, label: SharedString, chosen: bool, role: Role) -> Stateful<Div> {
    let selector = id.to_string();
    div()
        .id(id)
        .debug_selector(move || selector.clone())
        .role(role)
        .aria_label(label.clone())
        .aria_toggled(if chosen {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .px(px(12.))
        .py(px(6.))
        .text_size(px(12.))
        .cursor_pointer()
        .text_color(content(if chosen { 1. } else { 0.6 }))
        .when(chosen, |d| d.bg(content(0.1)))
        .when(!chosen, |d| d.hover(|s| s.bg(content(0.08))))
        .child(label)
}

/// «All agents» and one pill per agent; `pick` hears the new choice. The connect form and
/// each account of the Email tab share it.
pub(crate) fn readers_row(
    id: &str,
    agents: &[Agent],
    readers: &Readers,
    pick: impl Fn(Readers, &mut Window, &mut App) + Clone + 'static,
) -> AnyElement {
    let all = {
        let pick = pick.clone();
        pill(
            SharedString::from(format!("{id}-all")),
            "All agents".into(),
            *readers == Readers::All,
            Role::Switch,
        )
        .on_click(move |_, window, cx| pick(Readers::All, window, cx))
    };
    div()
        .id(SharedString::from(id.to_owned()))
        .role(Role::Group)
        .aria_label("Who can read it")
        .flex()
        .flex_wrap()
        .gap(px(4.))
        .child(all)
        .children(agents.iter().map(|agent| {
            let pick = pick.clone();
            let next = readers.toggled(&agent.id);
            pill(
                SharedString::from(format!("{id}-{}", agent.id)),
                agent.label.clone().into(),
                readers.reads(&agent.id),
                Role::Switch,
            )
            .on_click(move |_, window, cx| pick(next.clone(), window, cx))
        }))
        .into_any_element()
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

pub(crate) enum ConnectEvent {
    /// The account is connected, its password in the system's store.
    Connected(String),
    Close,
}

pub(crate) struct ConnectForm {
    service: model::Service,
    provider: &'static str,
    address: Entity<Input>,
    password: Entity<Input>,
    host: Entity<Input>,
    name: Entity<Input>,
    readers: Readers,
    agents: Vec<Agent>,
    /// The bot that opened it with `mail.connect`: the form is then a dialog.
    asked_by: Option<Agent>,
    busy: bool,
    error: Option<String>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ConnectEvent> for ConnectForm {}
impl Focusable for ConnectForm {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ConnectForm {
    /// The Email tab's form: a new account is read by every agent unless the user picks.
    pub(crate) fn new(service: model::Service, cx: &mut Context<Self>) -> Self {
        let field = |label: &str, placeholder: &str, cx: &mut Context<Self>| {
            let label = label.to_owned();
            let placeholder = placeholder.to_owned();
            cx.new(move |cx| Input::field(label, placeholder, content(0.35), cx))
        };
        let password = field("App password", "", cx);
        password.update(cx, |input, _| input.set_secret(true));
        let address = field("Email address", "", cx);
        let host = field("IMAP server", "imap.example.com", cx);
        let name = field("Name to show (optional)", "", cx);
        let mut subscriptions = Vec::new();
        for input in [&address, &password, &host, &name] {
            subscriptions.push(cx.subscribe(input, |_, _, _: &input::Changed, cx| cx.notify()));
            subscriptions
                .push(cx.subscribe(input, |this, _, _: &input::Submitted, cx| this.connect(cx)));
        }
        Self {
            service,
            provider: "gmail",
            address,
            password,
            host,
            name,
            readers: Readers::All,
            agents: Vec::new(),
            asked_by: None,
            busy: false,
            error: None,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    /// The dialog a bot opened: the address, provider and name it asked for, and kept for
    /// that bot alone unless it asked for every agent (the user can change both).
    pub(crate) fn asked(
        service: model::Service,
        request: &model::ConnectRequest,
        bot: Agent,
        agents: Vec<Agent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut form = Self::new(service, cx);
        form.provider = PROVIDERS
            .iter()
            .map(|(p, _)| *p)
            .find(|p| *p == request.provider)
            .unwrap_or_else(|| {
                PROVIDERS
                    .iter()
                    .map(|(p, _)| *p)
                    .find(|p| *p == model::provider_for(&request.address))
                    .unwrap_or("imap")
            });
        form.address
            .update(cx, |input, cx| input.set_text(request.address.clone(), cx));
        form.name
            .update(cx, |input, cx| input.set_text(request.name.clone(), cx));
        form.readers = if request.shared {
            Readers::All
        } else {
            Readers::Only(vec![bot.id.clone()])
        };
        form.agents = agents;
        form.asked_by = Some(bot);
        // The address is there: the password is what the owner types first.
        window.focus(&form.password.focus_handle(cx), cx);
        form
    }

    pub(crate) fn busy(&self) -> bool {
        self.busy
    }

    pub(crate) fn readers(&self) -> &Readers {
        &self.readers
    }

    pub(crate) fn set_agents(&mut self, agents: Vec<Agent>, cx: &mut Context<Self>) {
        self.agents = agents;
        cx.notify();
    }

    fn ready(&self, cx: &App) -> bool {
        !self.busy
            && !self.address.read(cx).content.trim().is_empty()
            && !self.password.read(cx).content.trim().is_empty()
            && (self.provider != "imap" || !self.host.read(cx).content.trim().is_empty())
    }

    fn connect(&mut self, cx: &mut Context<Self>) {
        if !self.ready(cx) {
            return;
        }
        let read = |input: &Entity<Input>| input.read(cx).content.to_string();
        let (address, password, name, host) = (
            read(&self.address),
            read(&self.password),
            read(&self.name),
            read(&self.host),
        );
        let provider = self.provider;
        let readers = self.readers.ids();
        self.busy = true;
        self.error = None;
        let service = self.service.clone();
        let task = cx.background_executor().spawn({
            let address = address.clone();
            async move {
                service.add_account(
                    &address,
                    &password,
                    &name,
                    provider,
                    (provider == "imap").then_some(host.as_str()),
                    readers,
                )
            }
        });
        // The password leaves the field at once, whatever the server says.
        self.password
            .update(cx, |input, cx| input.set_text(String::new(), cx));
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(()) => {
                        for input in [&this.address, &this.name, &this.host] {
                            input.update(cx, |input, cx| input.set_text(String::new(), cx));
                        }
                        if this.asked_by.is_none() {
                            this.readers = Readers::All;
                        }
                        cx.emit(ConnectEvent::Connected(address.trim().to_owned()));
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// The provider, the steps, the fields, who reads it and where the password goes.
    fn fields(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let (text, link) = steps(self.provider);
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
                        pill(
                            SharedString::from(format!("mail-provider-{provider}")),
                            label.into(),
                            self.provider == provider,
                            Role::RadioButton,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.provider = provider;
                            cx.notify();
                        }))
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .text_size(px(12.))
                    .line_height(px(19.))
                    .text_color(content(0.6))
                    .child(div().child(text))
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
            .child(labelled("Email address", &self.address, window, cx))
            .child(labelled("App password", &self.password, window, cx))
            .when(self.provider == "imap", |d| {
                d.child(labelled("IMAP server", &self.host, window, cx))
            })
            .child(labelled("Name to show (optional)", &self.name, window, cx))
            .when(!self.agents.is_empty(), |d| {
                let weak = cx.weak_entity();
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(content(LABEL))
                                .child("Who can read it"),
                        )
                        .child(readers_row(
                            "mail-connect-readers",
                            &self.agents,
                            &self.readers,
                            move |readers, _, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    this.readers = readers;
                                    cx.notify();
                                });
                            },
                        ))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(content(0.45))
                                .child("Agents can never give themselves access. You can change it later in Inbox › Email › Accounts."),
                        ),
                )
            })
            .child(
                div()
                    .text_size(px(11.))
                    .line_height(px(17.))
                    .text_color(content(0.45))
                    .child(format!(
                        "The password goes to {} and nowhere else: not to a file, the chat or any model. monoBOT reads the mail and sends only the replies you send yourself; it never deletes mail.",
                        keychain_name()
                    )),
            )
            .when_some(self.error.clone(), |d, error| d.child(alert(error)))
    }
}

impl Render for ConnectForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ready = self.ready(cx);
        let label = if self.busy {
            "Connecting…"
        } else {
            "Connect account"
        };
        let Some(bot) = self.asked_by.clone() else {
            return div()
                .track_focus(&self.focus)
                .child(
                    self.fields(window, cx).child(
                        div().child(
                            primary("mail-connect", label, ready)
                                .on_click(cx.listener(|this, _, _, cx| this.connect(cx))),
                        ),
                    ),
                )
                .into_any_element();
        };
        div()
            .id("mail-connect-dialog")
            .debug_selector(|| "mail-connect-dialog".into())
            .key_context("MailConnect")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && !this.busy {
                    cx.emit(ConnectEvent::Close);
                    cx.stop_propagation();
                }
            }))
            .size_full()
            .flex()
            .flex_col()
            .text_color(content(1.))
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(20.))
                    .pt(px(16.))
                    .pb(px(12.))
                    .child(
                        div()
                            .text_size(px(15.))
                            .line_height(px(20.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Connect a mailbox"),
                    )
                    .child(
                        div()
                            .mt_1()
                            .text_size(px(12.))
                            .text_color(content(0.55))
                            .child(format!(
                                "{} asked to read this mailbox. Its owner types the app password here; {} never sees it.",
                                bot.label, bot.label
                            )),
                    ),
            )
            .child(
                div()
                    .id("mail-connect-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(20.))
                    .pb(px(16.))
                    .child(self.fields(window, cx)),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .border_t_1()
                    .border_color(content(0.1))
                    .px(px(20.))
                    .py(px(12.))
                    .child(
                        modal::disabled(
                            modal::ghost_button("mail-connect-cancel", "Cancel")
                                .debug_selector(|| "mail-connect-cancel".into()),
                            self.busy,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            if !this.busy {
                                cx.emit(ConnectEvent::Close);
                            }
                        })),
                    )
                    .child(
                        modal::disabled(modal::primary_button("mail-connect", label), !ready)
                            .on_click(cx.listener(|this, _, _, cx| this.connect(cx))),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_reader_stays_and_all_agents_opens_it_again() {
        let all = Readers::All;
        assert_eq!(all.ids(), None);
        let finance = all.toggled("finance");
        assert_eq!(finance, Readers::Only(vec!["finance".into()]));
        assert_eq!(finance.toggled("finance"), finance);
        let both = finance.toggled("principal");
        assert_eq!(both.ids().unwrap(), ["finance", "principal"]);
        assert_eq!(
            both.toggled("finance"),
            Readers::Only(vec!["principal".into()])
        );
        let agents = [Agent {
            id: "finance".into(),
            label: "Finance".into(),
        }];
        assert_eq!(both.summary(&agents), "Finance, an agent no longer here");
        assert_eq!(all.summary(&agents), "All agents");
        let mut access = model::Access::new();
        access.insert("ana@gmail.com".into(), vec!["finance".into()]);
        assert_eq!(Readers::of(&access, "ANA@gmail.com"), finance);
        assert_eq!(Readers::of(&access, "luis@x.test"), Readers::All);
    }
}
