//! The Inbox's Agenda tab and the mail workspace's setup: a local agenda (reminders and time
//! blocks that never invite anyone), calendar files in and out, and the settings for
//! Mailflare, HTTP sending, the inbound mailbox and the bots that own an address.
//! Everything the user types goes to `monocode_core::inbox::workspace`; credentials are
//! typed into masked fields, handed to the core once and never read back or shown.
mod view;

use crate::{
    input::{self, Input},
    runtime::{Database, locked},
    theme::*,
};
use chrono::{
    DateTime, Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, Offset, SecondsFormat,
    TimeZone, Timelike, Utc,
};
use gpui::{App, AppContext, ClipboardItem, Context, Entity, Subscription};
use monocode_core::{
    connectors,
    inbox::workspace::{self as model, CalendarEvent, Config, NewEvent, Route},
    mono::Roster,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

/// How far ahead the agenda looks. The core accepts at most 90 days.
const AGENDA_DAYS: i64 = 30;
/// An agenda that is newer than this is not fetched again when the tab reopens.
const FRESH: Duration = Duration::from_secs(60);
/// A calendar file larger than this is not a calendar the user means to import.
const ICS_LIMIT: usize = 2 * 1024 * 1024;
/// The inbound secret is a bearer token for a public Worker, so the core wants it long.
const INBOUND_MIN: usize = 32;
/// The connector id the core registers for the Mailflare MCP server.
const MAILFLARE: &str = "mailflare";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Page {
    Agenda,
    Setup,
}

/// The one thing running in the background, which disables the buttons that would race it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Busy {
    Event,
    Import,
    Export,
    Save,
    Remove,
}

/// A credential field. The core never says whether one is stored, so `saved` only
/// remembers what this session stored.
pub(super) struct Secret {
    pub kind: &'static str,
    pub input: Entity<Input>,
    pub saved: bool,
}

/// A route as it is being edited.
pub(super) struct RouteDraft {
    pub alias: Entity<Input>,
    pub senders: Entity<Input>,
    pub bot_id: String,
    pub enabled: bool,
    _subscriptions: Vec<Subscription>,
}

pub(crate) struct Workspace {
    service: model::Service,
    store: Database,
    page: Page,
    busy: Option<Busy>,
    notice: Option<String>,
    error: Option<String>,

    events: Vec<CalendarEvent>,
    loading: bool,
    refreshed: Option<Instant>,
    /// The newest agenda request wins over one that comes back late.
    ticket: u64,
    /// The event whose Delete is waiting for a second click.
    deleting: Option<String>,
    form_open: bool,
    form_error: Option<String>,
    title: Entity<Input>,
    start: Entity<Input>,
    end: Entity<Input>,
    notes: Entity<Input>,

    config: Config,
    config_loaded: bool,
    config_loading: bool,
    mailflare_url: Entity<Input>,
    inbound_url: Entity<Input>,
    cloudflare_account: Entity<Input>,
    secrets: [Secret; 4],
    send_provider: String,
    /// Whether bots and sessions may use the Mailflare connector, as the store has it.
    mailflare_on: bool,
    mailflare_on_saved: bool,
    routes: Vec<RouteDraft>,
    /// Bots a route can hand mail to: (id, name).
    bots: Vec<(String, String)>,
    validated: bool,
    removing: Option<&'static str>,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    pub(crate) fn new(data_dir: PathBuf, store: Database, cx: &mut Context<Self>) -> Self {
        let field = |label: &str, placeholder: &str, cx: &mut Context<Self>| {
            let label = label.to_owned();
            let placeholder = placeholder.to_owned();
            cx.new(move |cx| Input::field(label, placeholder, content(0.35), cx))
        };
        let secret = |kind: &'static str, label: &str, cx: &mut Context<Self>| {
            let input = field(label, "", cx);
            input.update(cx, |input, _| input.set_secret(true));
            Secret {
                kind,
                input,
                saved: false,
            }
        };
        let title = field("Event title", "What is it?", cx);
        let start = field("Start", "YYYY-MM-DD HH:MM", cx);
        let end = field("End", "YYYY-MM-DD HH:MM", cx);
        let notes = field("Notes", "Optional", cx);
        let mailflare_url = field("Mailflare URL", "https://mailflare.example.com", cx);
        let inbound_url = field("Inbound Worker URL", "https://inbound.example.com", cx);
        let cloudflare_account = field("Cloudflare account ID", "32 hexadecimal characters", cx);
        let secrets = [
            secret(MAILFLARE, "Mailflare access token", cx),
            secret("resend", "Resend API key", cx),
            secret("cloudflare", "Cloudflare API token", cx),
            secret("inbound", "Inbound shared secret", cx),
        ];
        let mut subscriptions = Vec::new();
        for input in [&title, &start, &end, &notes] {
            subscriptions.push(cx.subscribe(input, |this, _, _: &input::Changed, cx| {
                this.form_error = None;
                cx.notify();
            }));
            subscriptions.push(cx.subscribe(input, |this, _, _: &input::Submitted, cx| {
                this.add_event(cx)
            }));
        }
        let setup = [&mailflare_url, &inbound_url, &cloudflare_account]
            .into_iter()
            .chain(secrets.iter().map(|secret| &secret.input));
        for input in setup {
            subscriptions.push(cx.subscribe(input, |this, _, _: &input::Changed, cx| {
                this.error = None;
                this.notice = None;
                cx.notify();
            }));
            subscriptions
                .push(cx.subscribe(input, |this, _, _: &input::Submitted, cx| this.save(cx)));
        }
        Self {
            service: model::Service::new(data_dir),
            store,
            page: Page::Agenda,
            busy: None,
            notice: None,
            error: None,
            events: Vec::new(),
            loading: false,
            refreshed: None,
            ticket: 0,
            deleting: None,
            form_open: false,
            form_error: None,
            title,
            start,
            end,
            notes,
            config: Config::default(),
            config_loaded: false,
            config_loading: false,
            mailflare_url,
            inbound_url,
            cloudflare_account,
            secrets,
            send_provider: "smtp".into(),
            mailflare_on: false,
            mailflare_on_saved: false,
            routes: Vec::new(),
            bots: Vec::new(),
            validated: false,
            removing: None,
            _subscriptions: subscriptions,
        }
    }

    /// The tab opened: read what is stored, and refresh an agenda that has gone stale.
    pub(crate) fn activate(&mut self, cx: &mut Context<Self>) {
        if !self.config_loaded && !self.config_loading {
            self.load_config(cx);
        }
        if self.page == Page::Agenda
            && !self.loading
            && self.refreshed.is_none_or(|at| at.elapsed() > FRESH)
        {
            self.load_agenda(cx);
        }
    }

    /// Esc closes the innermost open thing; false when there was none.
    pub(crate) fn escape(&mut self, cx: &mut Context<Self>) -> bool {
        let closed = self.removing.take().is_some()
            | self.deleting.take().is_some()
            | std::mem::take(&mut self.form_open);
        if closed {
            cx.notify();
        }
        closed
    }

    fn set_page(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.page == page {
            return;
        }
        self.page = page;
        self.removing = None;
        self.deleting = None;
        self.error = None;
        self.notice = None;
        self.activate(cx);
        cx.notify();
    }

    // Agenda -------------------------------------------------------------------------

    fn load_agenda(&mut self, cx: &mut Context<Self>) {
        self.ticket += 1;
        let ticket = self.ticket;
        self.loading = true;
        let (from, to) = window(Local::now());
        let service = self.service.clone();
        // The core asks Mailflare over the network when it is set up.
        let task = cx
            .background_executor()
            .spawn(async move { service.agenda(&from, &to) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.ticket != ticket {
                    return;
                }
                this.loading = false;
                this.refreshed = Some(Instant::now());
                match result {
                    Ok(events) => {
                        this.events = events;
                        if this.page == Page::Agenda {
                            this.error = None;
                        }
                    }
                    // The events already on screen stay: a failed refresh is not an empty agenda.
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn open_form(&mut self, cx: &mut Context<Self>) {
        let now = Local::now().naive_local();
        let start = now
            .date()
            .and_hms_opt(now.hour(), 0, 0)
            .unwrap_or(now)
            .checked_add_signed(chrono::Duration::hours(1))
            .unwrap_or(now);
        let end = start
            .checked_add_signed(chrono::Duration::hours(1))
            .unwrap_or(start);
        for (input, text) in [
            (&self.title, String::new()),
            (&self.start, wall_field(start)),
            (&self.end, wall_field(end)),
            (&self.notes, String::new()),
        ] {
            input.update(cx, |input, cx| input.set_text(text, cx));
        }
        self.form_open = true;
        self.form_error = None;
        self.notice = None;
        self.deleting = None;
        cx.notify();
    }

    fn add_event(&mut self, cx: &mut Context<Self>) {
        if !self.form_open || self.busy.is_some() {
            return;
        }
        let read = |input: &Entity<Input>| input.read(cx).content.trim().to_string();
        let (title, start, end, notes) = (
            read(&self.title),
            read(&self.start),
            read(&self.end),
            read(&self.notes),
        );
        let event = match event_draft(&title, &start, &end, &notes) {
            Ok(event) => event,
            Err(error) => {
                self.form_error = Some(error);
                cx.notify();
                return;
            }
        };
        self.busy = Some(Busy::Event);
        self.form_error = None;
        let service = self.service.clone();
        let name = event.title.clone();
        let task = cx
            .background_executor()
            .spawn(async move { service.create_event(event) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(_) => {
                        this.form_open = false;
                        this.notice = Some(format!("Added \u{201c}{name}\u{201d} to your agenda."));
                        this.load_agenda(cx);
                    }
                    Err(error) => this.form_error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Removes a local or imported event. Mailflare's own are read-only here.
    fn delete_event(&mut self, id: String, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        self.deleting = None;
        self.busy = Some(Busy::Remove);
        self.error = None;
        self.notice = None;
        let service = self.service.clone();
        let removed = id.clone();
        let task = cx
            .background_executor()
            .spawn(async move { service.remove_event(&removed) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(()) => {
                        this.events.retain(|event| event.id != id);
                        this.notice = Some("Removed from your agenda.".into());
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Imports the calendar file whose text is on the clipboard.
    fn import_ics(&mut self, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        let text = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap_or_default();
        if let Err(error) = check_ics(&text) {
            self.error = Some(error);
            self.notice = None;
            cx.notify();
            return;
        }
        self.busy = Some(Busy::Import);
        self.error = None;
        self.notice = None;
        let service = self.service.clone();
        let task = cx
            .background_executor()
            .spawn(async move { service.import_ics(&text) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(count) => {
                        this.notice = Some(match count {
                            1 => "Imported 1 event.".to_owned(),
                            n => format!("Imported {n} events."),
                        });
                        this.load_agenda(cx);
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Puts the whole local agenda on the clipboard as a calendar file.
    fn export_ics(&mut self, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(Busy::Export);
        self.error = None;
        self.notice = None;
        let service = self.service.clone();
        let task = cx
            .background_executor()
            .spawn(async move { service.export_ics() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(text) => {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                        this.notice = Some(
                            "Copied your agenda as an .ics calendar. Paste it into a file or another calendar app."
                                .into(),
                        );
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    // Setup --------------------------------------------------------------------------

    fn load_config(&mut self, cx: &mut Context<Self>) {
        self.config_loading = true;
        let service = self.service.clone();
        let store = self.store.clone();
        let task = cx.background_executor().spawn(async move {
            let config = service.config();
            let (bots, on) = match locked(&store) {
                Ok(db) => (
                    // The Principal can create bots and hand out work, so mail from outside
                    // never routes to it from here.
                    Roster::load(&db)
                        .map(|roster| {
                            roster
                                .monos
                                .iter()
                                .filter(|mono| mono.is_bot() && !mono.is_principal())
                                .map(|mono| (mono.id.clone(), mono.label()))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default(),
                    connectors::saved_value(&db, &connectors::enabled_key(MAILFLARE)).as_deref()
                        == Some("true"),
                ),
                Err(_) => (Vec::new(), false),
            };
            (config, bots, on)
        });
        cx.spawn(async move |this, cx| {
            let (config, bots, on) = task.await;
            let _ = this.update(cx, |this, cx| {
                this.config_loading = false;
                this.bots = bots;
                this.mailflare_on = on;
                this.mailflare_on_saved = on;
                match config {
                    Ok(config) => {
                        this.config_loaded = true;
                        this.apply_config(config, cx);
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn apply_config(&mut self, config: Config, cx: &mut Context<Self>) {
        for (input, text) in [
            (&self.mailflare_url, config.mailflare_url.clone()),
            (&self.inbound_url, config.inbound_url.clone()),
            (
                &self.cloudflare_account,
                config.cloudflare_account_id.clone(),
            ),
        ] {
            input.update(cx, |input, cx| input.set_text(text, cx));
        }
        self.send_provider = match config.send_provider.as_str() {
            provider @ ("resend" | "cloudflare") => provider.to_owned(),
            _ => "smtp".to_owned(),
        };
        self.routes = config
            .routes
            .iter()
            .map(|route| Self::route_draft(Some(route), cx))
            .collect();
        self.config = config;
    }

    fn route_draft(route: Option<&Route>, cx: &mut Context<Self>) -> RouteDraft {
        let alias =
            cx.new(|cx| Input::field("Bot address", "support@your-domain.com", content(0.35), cx));
        let senders = cx.new(|cx| {
            Input::field(
                "Allowed senders",
                "ana@example.com, luis@example.com",
                content(0.35),
                cx,
            )
        });
        if let Some(route) = route {
            alias.update(cx, |input, cx| input.set_text(route.alias.clone(), cx));
            senders.update(cx, |input, cx| {
                input.set_text(route.allowed_senders.join(", "), cx)
            });
        }
        let subscriptions = [&alias, &senders]
            .into_iter()
            .flat_map(|input| {
                [
                    cx.subscribe(input, |this, _, _: &input::Changed, cx| {
                        this.error = None;
                        this.notice = None;
                        cx.notify();
                    }),
                    cx.subscribe(input, |this, _, _: &input::Submitted, cx| this.save(cx)),
                ]
            })
            .collect();
        RouteDraft {
            alias,
            senders,
            bot_id: route.map(|route| route.bot_id.clone()).unwrap_or_default(),
            // A new route never processes mail until the user turns it on.
            enabled: route.is_some_and(|route| route.enabled),
            _subscriptions: subscriptions,
        }
    }

    fn add_route(&mut self, cx: &mut Context<Self>) {
        let draft = Self::route_draft(None, cx);
        self.routes.push(draft);
        cx.notify();
    }

    fn secret(&self, kind: &str) -> &Secret {
        self.secrets
            .iter()
            .find(|secret| secret.kind == kind)
            .unwrap_or(&self.secrets[0])
    }

    /// The settings as the form shows them now, to compare with what is stored.
    fn draft(&self, cx: &App) -> Config {
        let read = |input: &Entity<Input>| input.read(cx).content.trim().to_string();
        let mut config = self.config.clone();
        config.mailflare_url = read(&self.mailflare_url);
        config.inbound_url = read(&self.inbound_url);
        // The core stores "" for the default; SMTP is that default.
        config.send_provider =
            if self.send_provider == "smtp" && self.config.send_provider.is_empty() {
                String::new()
            } else {
                self.send_provider.clone()
            };
        config.cloudflare_account_id = read(&self.cloudflare_account);
        config.routes = self
            .routes
            .iter()
            .map(|route| Route {
                alias: normalize_alias(&read(&route.alias)),
                bot_id: route.bot_id.clone(),
                allowed_senders: split_senders(&read(&route.senders)),
                enabled: route.enabled,
            })
            .collect();
        config
    }

    /// Whether anything differs from what is stored, including a credential being typed.
    fn pending(&self, cx: &App) -> bool {
        self.draft(cx) != self.config
            || self.mailflare_on != self.mailflare_on_saved
            || self
                .secrets
                .iter()
                .any(|secret| !secret.input.read(cx).content.trim().is_empty())
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        if self.busy.is_some() || !self.config_loaded {
            return;
        }
        self.validated = true;
        let mut config = self.draft(cx);
        let rows = self
            .routes
            .iter()
            .map(|route| {
                (
                    route.alias.read(cx).content.to_string(),
                    route.senders.read(cx).content.to_string(),
                    route.bot_id.clone(),
                    route.enabled,
                )
            })
            .collect::<Vec<_>>();
        let secrets = self
            .secrets
            .iter()
            .filter_map(|secret| {
                let value = secret.input.read(cx).content.trim().to_string();
                (!value.is_empty()).then_some((secret.kind, value))
            })
            .collect::<Vec<_>>();
        if let Err(error) = check_setup(&config, self.mailflare_on, &rows, &secrets) {
            self.error = Some(error);
            self.notice = None;
            cx.notify();
            return;
        }
        // Validated above, so the strict parse cannot fail; it also normalizes.
        config.routes = rows
            .iter()
            .map(|(alias, senders, bot_id, enabled)| Route {
                alias: normalize_alias(alias),
                bot_id: bot_id.clone(),
                allowed_senders: parse_senders(senders).unwrap_or_default(),
                enabled: *enabled,
            })
            .collect();
        let stored = secrets.iter().map(|(kind, _)| *kind).collect::<Vec<_>>();
        let mailflare_on = self.mailflare_on;
        self.busy = Some(Busy::Save);
        self.error = None;
        self.notice = None;
        let service = self.service.clone();
        let store = self.store.clone();
        let task = cx.background_executor().spawn(async move {
            service.save_config(&config)?;
            for (kind, value) in &secrets {
                service.set_secret(kind, value)?;
            }
            // The connector reads the URL the config just stored, so this comes last.
            let db = locked(&store)?;
            connectors::save_enabled(&db, MAILFLARE, mailflare_on)?;
            drop(db);
            service.config()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(config) => {
                        for secret in &mut this.secrets {
                            if stored.contains(&secret.kind) {
                                secret.saved = true;
                                secret
                                    .input
                                    .update(cx, |input, cx| input.set_text(String::new(), cx));
                            }
                        }
                        this.mailflare_on_saved = this.mailflare_on;
                        this.validated = false;
                        this.apply_config(config, cx);
                        this.notice = Some("Saved.".into());
                        this.refreshed = None;
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn remove_secret(&mut self, kind: &'static str, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        self.removing = None;
        self.busy = Some(Busy::Remove);
        self.error = None;
        self.notice = None;
        let service = self.service.clone();
        let task = cx
            .background_executor()
            .spawn(async move { service.set_secret(kind, "") });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(()) => {
                        if let Some(secret) = this.secrets.iter_mut().find(|s| s.kind == kind) {
                            secret.saved = false;
                            secret
                                .input
                                .update(cx, |input, cx| input.set_text(String::new(), cx));
                        }
                        this.notice = Some("Removed the saved credential.".into());
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// The bots a route can pick, plus any the stored routes name that are no longer in the roster.
    fn bot_options(&self) -> Vec<(String, String)> {
        let mut options = self.bots.clone();
        for route in &self.routes {
            if !route.bot_id.is_empty() && !options.iter().any(|(id, _)| *id == route.bot_id) {
                options.push((
                    route.bot_id.clone(),
                    format!("Unknown bot ({})", route.bot_id),
                ));
            }
        }
        options
    }
}

// Pure helpers ------------------------------------------------------------------------

/// How a time is written in the form's fields.
fn wall_field(time: NaiveDateTime) -> String {
    time.format("%Y-%m-%d %H:%M").to_string()
}

/// A point in an event's time as the agenda shows it, on this computer's clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Moment {
    Timed(NaiveDateTime),
    AllDay(NaiveDate),
}

impl Moment {
    pub(super) fn date(self) -> NaiveDate {
        match self {
            Self::Timed(time) => time.date(),
            Self::AllDay(date) => date,
        }
    }
    fn time(self) -> NaiveTime {
        match self {
            Self::Timed(time) => time.time(),
            Self::AllDay(_) => NaiveTime::MIN,
        }
    }
}

/// A time written without an offset: the form's own, ISO 8601, or a bare date.
fn parse_naive(text: &str) -> Option<Moment> {
    let text = text.trim();
    let naive = text.trim_end_matches('Z');
    for format in [
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y%m%dT%H%M%S",
    ] {
        if let Ok(time) = NaiveDateTime::parse_from_str(naive, format) {
            return Some(Moment::Timed(time));
        }
    }
    ["%Y-%m-%d", "%Y%m%d"]
        .into_iter()
        .find_map(|format| NaiveDate::parse_from_str(text, format).ok())
        .map(Moment::AllDay)
}

/// A start or end the core returned, on this computer's clock. A time that carries an
/// offset is an instant, so it is converted; one that does not is taken as it is.
pub(super) fn parse_moment(text: &str) -> Option<Moment> {
    match DateTime::parse_from_rfc3339(text.trim()) {
        Ok(time) => Some(Moment::Timed(time.with_timezone(&Local).naive_local())),
        Err(_) => parse_naive(text),
    }
}

/// An instant from what the form holds: a wall-clock time on this computer's clock, or a
/// full ISO 8601 time that names its own offset. The core wants an explicit offset.
fn instant(text: &str) -> Result<DateTime<chrono::FixedOffset>, String> {
    let text = text.trim();
    if let Ok(time) = DateTime::parse_from_rfc3339(text) {
        return Ok(time);
    }
    let Some(Moment::Timed(naive)) = parse_naive(text) else {
        return Err("Write the date and time like 2026-05-14 15:00.".into());
    };
    Local
        .from_local_datetime(&naive)
        .single()
        .map(|time| time.with_timezone(&time.offset().fix()))
        .ok_or_else(|| {
            "That time does not exist, or happens twice, when the clocks change. Pick another."
                .into()
        })
}

/// An IANA-style zone name, loosely: the core has the zone database and has the last word.
fn valid_zone(zone: &str) -> bool {
    !zone.is_empty()
        && zone.len() <= 64
        && zone
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '+'))
}

fn zone_from_link(target: &str) -> Option<String> {
    let (_, zone) = target.rsplit_once("zoneinfo/")?;
    valid_zone(zone).then(|| zone.to_owned())
}

/// This computer's zone name, which new events are filed under. Their instants are exact
/// either way; the name only says where the user made them.
pub(super) fn local_zone() -> String {
    static ZONE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ZONE.get_or_init(|| {
        std::env::var("TZ")
            .ok()
            .map(|zone| zone.trim_start_matches(':').to_owned())
            .filter(|zone| valid_zone(zone))
            .or_else(|| {
                std::fs::read_link("/etc/localtime")
                    .ok()
                    .and_then(|target| zone_from_link(&target.to_string_lossy()))
            })
            .unwrap_or_else(|| "UTC".into())
    })
    .clone()
}

/// The event the form describes, or what is wrong with it.
fn event_draft(title: &str, start: &str, end: &str, notes: &str) -> Result<NewEvent, String> {
    if title.is_empty() {
        return Err("Give the event a title.".into());
    }
    let start = instant(start).map_err(|e| format!("Start: {e}"))?;
    let end = instant(end).map_err(|e| format!("End: {e}"))?;
    if end <= start {
        return Err("The end has to come after the start.".into());
    }
    let wire =
        |time: DateTime<chrono::FixedOffset>| time.to_rfc3339_opts(SecondsFormat::Secs, false);
    Ok(NewEvent {
        title: title.to_owned(),
        start: wire(start),
        end: wire(end),
        timezone: local_zone(),
        description: notes.to_owned(),
    })
}

/// A calendar file's text, or why it cannot be one.
fn check_ics(text: &str) -> Result<(), String> {
    if text.len() > ICS_LIMIT {
        return Err("That text is too large to be a calendar file (2 MiB at most).".into());
    }
    if !text.to_ascii_uppercase().contains("BEGIN:VCALENDAR") {
        return Err("The clipboard does not hold a calendar. Open the .ics file in a text editor, copy all of it, then try again.".into());
    }
    Ok(())
}

/// The agenda's window as UTC instants: from the start of today to a month ahead.
fn window(now: DateTime<Local>) -> (String, String) {
    let midnight = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|time| time.and_local_timezone(Local).earliest())
        .unwrap_or(now);
    let end = midnight + chrono::Duration::days(AGENDA_DAYS);
    let wire = |time: DateTime<Local>| {
        time.with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Secs, true)
    };
    (wire(midnight), wire(end))
}

pub(super) struct Entry {
    pub event: CalendarEvent,
    pub start: Option<Moment>,
    pub end: Option<Moment>,
    /// The event's own clock, when its zone is not this computer's at that moment.
    pub origin: Option<String>,
}

impl Entry {
    /// Whether the user can remove it here; Mailflare's schedule is read-only.
    pub(super) fn removable(&self) -> bool {
        !self.event.id.is_empty() && !self.event.source.eq_ignore_ascii_case("mailflare")
    }
}

fn entry_for(event: &CalendarEvent) -> Entry {
    let parsed = DateTime::parse_from_rfc3339(event.start.trim())
        .ok()
        .zip(DateTime::parse_from_rfc3339(event.end.trim()).ok());
    if let Some((start, end)) = parsed {
        if event.all_day {
            return Entry {
                event: event.clone(),
                start: Some(Moment::AllDay(start.date_naive())),
                end: Some(Moment::AllDay(end.date_naive())),
                origin: None,
            };
        }
        let local_offset = start.with_timezone(&Local).offset().local_minus_utc();
        let origin = (start.offset().local_minus_utc() != local_offset
            && !event.timezone.is_empty())
        .then(|| format!("{} {}", start.naive_local().format("%H:%M"), event.timezone));
        return Entry {
            event: event.clone(),
            start: parse_moment(&event.start),
            end: parse_moment(&event.end),
            origin,
        };
    }
    Entry {
        event: event.clone(),
        start: parse_moment(&event.start),
        end: parse_moment(&event.end),
        origin: None,
    }
}

pub(super) struct Day {
    /// None for events whose time could not be read.
    pub date: Option<NaiveDate>,
    pub entries: Vec<Entry>,
}

/// The events by day, earliest first; all-day events lead their day and the unreadable come last.
pub(super) fn group_days(events: &[CalendarEvent]) -> Vec<Day> {
    let mut entries = events.iter().map(entry_for).collect::<Vec<_>>();
    entries.sort_by_key(|entry| {
        (
            entry.start.is_none(),
            entry.start.map(Moment::date),
            entry.start.map(Moment::time),
            entry.event.title.to_lowercase(),
        )
    });
    let mut days: Vec<Day> = Vec::new();
    for entry in entries {
        let date = entry.start.map(Moment::date);
        match days.last_mut() {
            Some(day) if day.date == date => day.entries.push(entry),
            _ => days.push(Day {
                date,
                entries: vec![entry],
            }),
        }
    }
    days
}

pub(super) fn day_heading(date: NaiveDate, today: NaiveDate) -> String {
    let name = date.format("%a %-d %b").to_string();
    match (date - today).num_days() {
        0 => format!("Today \u{b7} {name}"),
        1 => format!("Tomorrow \u{b7} {name}"),
        -1 => format!("Yesterday \u{b7} {name}"),
        _ if date.year() != today.year() => date.format("%a %-d %b %Y").to_string(),
        _ => name,
    }
}

/// When an entry happens, as its row reads it.
pub(super) fn when_label(entry: &Entry) -> String {
    match (entry.start, entry.end) {
        (Some(Moment::AllDay(_)), _) => "All day".into(),
        (Some(Moment::Timed(start)), Some(Moment::Timed(end))) if end.date() == start.date() => {
            format!("{} \u{2013} {}", start.format("%H:%M"), end.format("%H:%M"))
        }
        (Some(Moment::Timed(start)), Some(Moment::Timed(end))) => format!(
            "{} \u{2013} {}",
            start.format("%H:%M"),
            end.format("%-d %b %H:%M")
        ),
        (Some(Moment::Timed(start)), _) => start.format("%H:%M").to_string(),
        (None, _) => entry.event.start.clone(),
    }
}

/// Where an event came from, when that is worth saying; the user's own events are the default.
pub(super) fn source_label(source: &str) -> Option<String> {
    match source.trim().to_lowercase().as_str() {
        "" | "local" => None,
        "ics" => Some("Imported".into()),
        "mailflare" => Some("Mailflare".into()),
        other => {
            let mut chars = other.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
        }
    }
}

/// A server address the user typed: empty, or the https origin the core accepts.
fn url_issue(url: &str) -> Option<&'static str> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    if url.chars().any(char::is_whitespace) {
        return Some("The address cannot contain spaces.");
    }
    if !url.starts_with("https://") || url.len() == "https://".len() {
        return Some("Use an https:// address.");
    }
    None
}

/// A bot's address as the core keeps it.
pub(super) fn normalize_alias(alias: &str) -> String {
    alias.trim().to_lowercase()
}

/// A complete address, loosely: the core has the last word.
fn valid_address(address: &str) -> bool {
    address.len() <= 254
        && address.matches('@').count() == 1
        && !address.contains('*')
        && !address.chars().any(|c| c.is_whitespace() || c.is_control())
        && address.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        })
}

fn alias_issue(alias: &str) -> Option<&'static str> {
    let alias = normalize_alias(alias);
    if alias.is_empty() {
        return Some("Write the address this bot receives mail at, like support@your-domain.com.");
    }
    (!valid_address(&alias)).then_some("The bot address has to be a complete email address.")
}

/// The addresses in a field, lowercased and without repeats; nothing is validated.
pub(super) fn split_senders(text: &str) -> Vec<String> {
    let mut senders: Vec<String> = Vec::new();
    for part in text.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
        let part = part
            .trim()
            .trim_matches(|c: char| c == '<' || c == '>')
            .to_lowercase();
        if !part.is_empty() && !senders.contains(&part) {
            senders.push(part);
        }
    }
    senders
}

/// The allowlist: complete addresses only. A domain or a wildcard would let in people the
/// user never named.
pub(super) fn parse_senders(text: &str) -> Result<Vec<String>, String> {
    let senders = split_senders(text);
    for sender in &senders {
        if !valid_address(sender) {
            return Err(format!(
                "\u{201c}{sender}\u{201d} is not a full email address. Domains and wildcards are not accepted."
            ));
        }
    }
    Ok(senders)
}

/// What is wrong with a route, if anything.
pub(super) fn route_issue(
    alias: &str,
    senders: &str,
    bot_id: &str,
    enabled: bool,
) -> Option<String> {
    if let Some(issue) = alias_issue(alias) {
        return Some(issue.to_owned());
    }
    match parse_senders(senders) {
        Err(error) => return Some(error),
        Ok(list) if enabled && list.is_empty() => {
            return Some("Add at least one allowed sender before turning this route on.".into());
        }
        Ok(_) => {}
    }
    bot_id
        .is_empty()
        .then(|| "Choose the bot that handles this route.".to_owned())
}

/// Whether a Cloudflare account ID looks like one: 32 hexadecimal characters.
fn account_id_ok(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|c| c.is_ascii_hexdigit())
}

/// The first problem that keeps the setup from being saved.
fn check_setup(
    config: &Config,
    mailflare_on: bool,
    routes: &[(String, String, String, bool)],
    secrets: &[(&str, String)],
) -> Result<(), String> {
    for (label, url) in [
        ("Mailflare URL", &config.mailflare_url),
        ("Inbound Worker URL", &config.inbound_url),
    ] {
        if let Some(issue) = url_issue(url) {
            return Err(format!("{label}: {issue}"));
        }
    }
    if mailflare_on && config.mailflare_url.trim().is_empty() {
        return Err("Add the Mailflare URL before letting bots use it.".into());
    }
    if config.send_provider == "cloudflare" && !account_id_ok(&config.cloudflare_account_id) {
        return Err(
            "The Cloudflare account ID is 32 hexadecimal characters; copy it from the dashboard."
                .into(),
        );
    }
    if secrets
        .iter()
        .any(|(kind, value)| *kind == "inbound" && value.chars().count() < INBOUND_MIN)
    {
        return Err(format!(
            "The inbound shared secret needs at least {INBOUND_MIN} characters."
        ));
    }
    let mut seen: Vec<String> = Vec::new();
    for (alias, senders, bot_id, enabled) in routes {
        if let Some(issue) = route_issue(alias, senders, bot_id, *enabled) {
            return Err(issue);
        }
        let alias = normalize_alias(alias);
        if seen.contains(&alias) {
            return Err(format!("Two routes use the address {alias}."));
        }
        seen.push(alias);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(title: &str, start: &str, end: &str) -> CalendarEvent {
        CalendarEvent {
            id: title.into(),
            title: title.into(),
            start: start.into(),
            end: end.into(),
            timezone: "UTC".into(),
            source: "local".into(),
            ..Default::default()
        }
    }

    fn at(h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 5, 14)
            .unwrap()
            .and_hms_opt(h, m, 0)
            .unwrap()
    }

    #[test]
    fn times_without_an_offset_are_read_as_written_and_instants_move_to_local() {
        for text in [
            "2026-05-14 15:30",
            "2026-05-14T15:30",
            "2026-05-14T15:30:00",
            "20260514T153000",
        ] {
            assert_eq!(
                parse_moment(text),
                Some(Moment::Timed(at(15, 30))),
                "{text}"
            );
        }
        // ISO 8601 fractions are meaningful, not implicit rounding to a minute.
        let fractional = NaiveDate::from_ymd_opt(2026, 5, 14)
            .unwrap()
            .and_hms_milli_opt(15, 30, 0, 250)
            .unwrap();
        assert_eq!(
            parse_moment("2026-05-14T15:30:00.250"),
            Some(Moment::Timed(fractional))
        );
        assert_eq!(
            instant("2026-05-14T15:30:00.250")
                .unwrap()
                .with_timezone(&Local)
                .naive_local(),
            fractional
        );
        assert_eq!(
            parse_moment("2026-05-14"),
            Some(Moment::AllDay(at(0, 0).date()))
        );
        assert_eq!(parse_moment("tomorrow at nine"), None);
        let zoned = DateTime::parse_from_rfc3339("2026-05-14T15:30:00+02:00").unwrap();
        assert_eq!(
            parse_moment("2026-05-14T15:30:00+02:00"),
            Some(Moment::Timed(zoned.with_timezone(&Local).naive_local()))
        );
    }

    #[test]
    fn a_new_event_gets_an_exact_instant_with_an_explicit_offset() {
        let ok = event_draft("Review", "2026-05-14 15:00", "2026-05-14 16:00", "notes").unwrap();
        let start = DateTime::parse_from_rfc3339(&ok.start).expect("RFC 3339 with an offset");
        let end = DateTime::parse_from_rfc3339(&ok.end).unwrap();
        assert_eq!(end - start, chrono::Duration::hours(1));
        // Unless daylight saving made the wall time impossible, it is this computer's 15:00.
        assert_eq!(
            start.with_timezone(&Local).naive_local(),
            at(15, 0),
            "{}",
            ok.start
        );
        assert!(valid_zone(&ok.timezone));
        assert_eq!(ok.description, "notes");

        let given = event_draft(
            "A",
            "2026-05-14T09:00:00-04:00",
            "2026-05-14T10:00:00-04:00",
            "",
        )
        .unwrap();
        assert_eq!(given.start, "2026-05-14T09:00:00-04:00");
    }

    #[test]
    fn a_new_event_needs_a_title_and_a_forward_range() {
        let draft = |title, start, end| event_draft(title, start, end, "");
        assert!(draft("", "2026-05-14 15:00", "2026-05-14 16:00").is_err());
        assert!(draft("A", "soon", "2026-05-14 16:00").is_err());
        assert!(draft("A", "2026-05-14", "2026-05-14 16:00").is_err());
        assert!(draft("A", "2026-05-14 16:00", "2026-05-14 16:00").is_err());
        assert!(draft("A", "2026-05-14 16:00", "2026-05-14 15:00").is_err());
    }

    #[test]
    fn days_group_and_order_events() {
        let events = [
            event("Later", "2026-05-15T09:00:00", "2026-05-15T10:00:00"),
            event("Standup", "2026-05-14T10:00:00", "2026-05-14T10:15:00"),
            event("Holiday", "2026-05-14", ""),
            event("Lost", "never", ""),
            event("Lunch", "2026-05-14T13:00:00", "2026-05-14T14:00:00"),
        ];
        let days = group_days(&events);
        let titles = |day: &Day| {
            day.entries
                .iter()
                .map(|entry| entry.event.title.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(days.len(), 3);
        assert_eq!(titles(&days[0]), ["Holiday", "Standup", "Lunch"]);
        assert_eq!(titles(&days[1]), ["Later"]);
        assert_eq!(days[2].date, None);
        assert_eq!(titles(&days[2]), ["Lost"]);
    }

    #[test]
    fn rows_say_when_and_what_can_be_removed() {
        let days = group_days(&[
            event("A", "2026-05-14T10:00:00", "2026-05-14T11:30:00"),
            event("B", "2026-05-14T22:00:00", "2026-05-15T02:00:00"),
            event("C", "2026-05-14", ""),
        ]);
        let labels = days[0].entries.iter().map(when_label).collect::<Vec<_>>();
        assert_eq!(
            labels,
            [
                "All day",
                "10:00 \u{2013} 11:30",
                "22:00 \u{2013} 15 May 02:00"
            ]
        );
        let today = at(0, 0).date();
        assert_eq!(day_heading(today, today), "Today \u{b7} Thu 14 May");
        assert_eq!(
            day_heading(today.succ_opt().unwrap(), today),
            "Tomorrow \u{b7} Fri 15 May"
        );
        assert_eq!(source_label("local"), None);
        assert_eq!(source_label("mailflare").as_deref(), Some("Mailflare"));
        assert_eq!(source_label("ics").as_deref(), Some("Imported"));

        let remote = CalendarEvent {
            source: "mailflare".into(),
            ..event("R", "2026-05-14T10:00:00", "2026-05-14T11:00:00")
        };
        assert!(!entry_for(&remote).removable());
        assert!(entry_for(&event("L", "2026-05-14", "")).removable());
    }

    #[test]
    fn date_only_calendar_entries_read_as_all_day() {
        let mut holiday = event(
            "Holiday",
            "2026-05-14T00:00:00+00:00",
            "2026-05-15T00:00:00+00:00",
        );
        holiday.all_day = true;
        let all_day = entry_for(&holiday);
        assert_eq!(all_day.start, Some(Moment::AllDay(at(0, 0).date())));
        assert_eq!(when_label(&all_day), "All day");
        let timed = entry_for(&event(
            "Call",
            "2026-05-14T09:30:00+00:00",
            "2026-05-14T10:30:00+00:00",
        ));
        assert!(matches!(timed.start, Some(Moment::Timed(_))));
        let overnight = entry_for(&event(
            "Night shift",
            "2026-05-14T00:00:00+00:00",
            "2026-05-15T00:00:00+00:00",
        ));
        assert!(matches!(overnight.start, Some(Moment::Timed(_))));
    }

    #[test]
    fn the_agenda_window_is_a_month_of_utc_instants() {
        let (from, to) = window(Local::now());
        let from = DateTime::parse_from_rfc3339(&from).unwrap();
        let to = DateTime::parse_from_rfc3339(&to).unwrap();
        assert!(from <= Local::now());
        assert!(to - from >= chrono::Duration::days(AGENDA_DAYS - 1));
        assert!(to - from <= chrono::Duration::days(90));
    }

    #[test]
    fn zones_come_from_the_localtime_link() {
        assert_eq!(
            zone_from_link("/var/db/timezone/zoneinfo/Europe/Madrid").as_deref(),
            Some("Europe/Madrid")
        );
        assert_eq!(
            zone_from_link("/usr/share/zoneinfo/UTC").as_deref(),
            Some("UTC")
        );
        assert_eq!(zone_from_link("/etc/localtime"), None);
        assert!(!valid_zone("Mars time"));
    }

    #[test]
    fn the_allowlist_takes_full_addresses_only() {
        assert_eq!(
            parse_senders("Ana@Example.com, luis@example.com;ana@example.com <eva@example.org>"),
            Ok(vec![
                "ana@example.com".to_owned(),
                "luis@example.com".into(),
                "eva@example.org".into()
            ])
        );
        assert_eq!(parse_senders("  "), Ok(vec![]));
        for bad in [
            "example.com",
            "@example.com",
            "*@example.com",
            "a@b",
            "a@@b.com",
        ] {
            assert!(parse_senders(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_route_needs_an_address_and_a_bot_and_runs_only_with_senders() {
        let issue = route_issue;
        assert!(issue("support@example.com", "", "bot", false).is_none());
        assert!(issue("support@example.com", "", "bot", true).is_some());
        assert!(issue("support@example.com", "a@example.com", "", false).is_some());
        assert!(issue("support@example.com", "a@example.com", "bot", true).is_none());
        assert!(issue("", "a@example.com", "bot", false).is_some());
        assert!(issue("support", "a@example.com", "bot", false).is_some());
        assert!(issue("Support@Example.com", "a@example.com", "bot", true).is_none());
    }

    #[test]
    fn setup_checks_addresses_providers_secrets_and_duplicate_routes() {
        let route = |alias: &str| (alias.to_owned(), String::new(), "bot".to_owned(), false);
        let check = |config: &Config, on: bool, routes: &[(String, String, String, bool)]| {
            check_setup(config, on, routes, &[])
        };
        let config = Config::default();
        let (a, b) = (route("a@example.com"), route("b@example.com"));
        assert!(check(&config, false, &[a.clone(), b]).is_ok());
        assert!(check(&config, false, &[a, route("A@Example.com")]).is_err());
        assert!(check(&config, true, &[]).is_err(), "no URL to use");

        let mut config = Config {
            mailflare_url: "http://example.com".into(),
            ..Default::default()
        };
        assert!(check(&config, false, &[]).is_err());
        config.mailflare_url = "https://mail.example.com".into();
        assert!(check(&config, true, &[]).is_ok());
        config.send_provider = "cloudflare".into();
        assert!(check(&config, false, &[]).is_err());
        config.cloudflare_account_id = "0123456789abcdef0123456789ABCDEF".into();
        assert!(check(&config, false, &[]).is_ok());
        config.cloudflare_account_id = "0123456789abcdef0123456789abcdeg".into();
        assert!(check(&config, false, &[]).is_err());

        let short = [("inbound", "x".repeat(INBOUND_MIN - 1))];
        let long = [("inbound", "x".repeat(INBOUND_MIN))];
        let config = Config::default();
        assert!(check_setup(&config, false, &[], &short).is_err());
        assert!(check_setup(&config, false, &[], &long).is_ok());
    }

    #[test]
    fn calendar_files_are_checked_before_the_core_sees_them() {
        assert!(check_ics("BEGIN:VCALENDAR\nEND:VCALENDAR").is_ok());
        assert!(check_ics("begin:vcalendar\nend:vcalendar").is_ok());
        assert!(check_ics("hello").is_err());
        assert!(check_ics(&"x".repeat(ICS_LIMIT + 1)).is_err());
    }
}
