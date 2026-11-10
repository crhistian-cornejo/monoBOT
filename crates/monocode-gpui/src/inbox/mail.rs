//! The Inbox's Email tab: the user's mail beside GitHub and the trackers, as the desktop
//! app's MailInbox shows it. Read it, sort it, and answer with a reply AI drafts for the
//! user to review. Accounts, secrets and the last reading are the desktop app's own
//! (`monocode_core::inbox::mail`), so a mailbox connected there shows here.
mod accounts;
mod art;
mod compose;
pub(crate) mod connect;
mod organize;
mod reader;
mod reply;
mod rules_view;

use crate::{
    input::{self, Input},
    monos::html::Html,
    popover::{Anchor, AnchorSlot, popover},
    resize_limit::{Axis, LimitGlow, limit_glow},
    theme::*,
};
use gpui::{
    AnyElement, App, AppContext, Context, CursorStyle, Div, Entity, EventEmitter, FocusHandle,
    Focusable, FontWeight, Image, ImageFormat, KeyDownEvent, MouseButton, Role, ScrollHandle,
    SharedString, Stateful, Subscription, Task, Window, canvas, div, linear_color_stop,
    linear_gradient, point, prelude::*, px, rgb,
};
use monocode_core::inbox::mail::{self as model, AccountView, Action, Message, Opened, Overview};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

pub(crate) use reply::MailReply;

/// The list's default width, as `w-[340px]`.
const LIST_WIDTH: f32 = 340.;
const MIN_LIST_WIDTH: f32 = 240.;
const MAX_LIST_WIDTH: f32 = 520.;
/// How far into each edge of the folder strip the fade reaches (`ScrollStrip.tsx`).
const VIEWS_FADE: f32 = 28.;
/// Messages opened this session: going back to one shows it at once.
const OPENED_KEPT: usize = 40;
/// The most pictures on the web one message loads, and how many are fetched at once.
const MAX_IMAGES: usize = 40;
const IMAGES_AT_ONCE: usize = 6;

pub(crate) enum MailEvent {
    /// The user turned automatic pictures on or off.
    AutoImages(bool),
    /// The user resized the mail list sidebar.
    ListWidth(f32),
}

/// The open message: what the reader shows of it.
struct Reader {
    id: String,
    opened: Option<Opened>,
    error: Option<String>,
    /// Pictures on the web that landed, by address.
    images: HashMap<String, String>,
    loading_images: bool,
    /// Bumped when the message changes, so a load still running for the last one stops.
    run: u64,
    html: Option<Entity<Html>>,
    plain_text: bool,
    /// The attachments' pictures, decoded once: (index, thumbnail).
    pictures: Vec<(usize, Arc<Image>)>,
    zoomed: Option<usize>,
    notice: Option<String>,
}

impl Reader {
    fn new(id: String) -> Self {
        Self {
            id,
            opened: None,
            error: None,
            images: HashMap::new(),
            loading_images: false,
            run: 0,
            html: None,
            plain_text: false,
            pictures: Vec::new(),
            zoomed: None,
            notice: None,
        }
    }
}

pub(crate) struct Mailbox {
    service: model::Service,
    started: bool,
    accounts: Option<Vec<AccountView>>,
    overview: Option<Overview>,
    view: model::View,
    views_scroll: ScrollHandle,
    views_overflow: (bool, bool),
    list_width: f32,
    resize_start: Option<(f32, f32)>,
    limit_glow: Option<LimitGlow>,
    account_id: Option<String>,
    account_menu: bool,
    account_anchor: AnchorSlot,
    search: Entity<Input>,
    selected: Option<String>,
    loading: bool,
    error: Option<String>,
    /// The newest reading wins over one that came back late.
    ticket: u64,
    settings_open: bool,
    auto_images: bool,
    reader: Option<Reader>,
    opened: VecDeque<Opened>,
    reply: Option<Entity<MailReply>>,
    /// A new message or a forward being written, in place of the reader.
    compose: Option<Entity<compose::MailCompose>>,
    /// Messages snoozed on this computer, and when each comes back.
    snoozed: model::Snoozed,
    /// The reader's row of snooze choices is open.
    snooze_menu: bool,
    /// The user's mail rules, the form for a new one, and the last problem saving one.
    rules: Vec<model::Rule>,
    rule_form: Option<rules_view::RuleForm>,
    rule_error: Option<String>,
    /// What the agents did with the mail lately.
    activity: Vec<model::activity::Entry>,
    /// How mail leaves: `smtp` (each account's own server), `resend` or `cloudflare`.
    send_via: String,
    form: Entity<connect::ConnectForm>,
    /// Which agents read each account, and the bots it can be kept for.
    access: model::Access,
    agents: Vec<connect::Agent>,
    /// The account whose «Who can read it» row is open.
    readers_open: Option<String>,
    /// Where the bots are listed, for «Who can read it».
    store: Option<crate::runtime::Database>,
    /// The photos the user chose in the desktop app for their accounts.
    photos: HashMap<String, Arc<Image>>,
    hover: Option<Task<()>>,
    prefetched: Vec<String>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<MailEvent> for Mailbox {}
impl Focusable for Mailbox {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn caption(text: impl Into<SharedString>) -> Div {
    div()
        .p(px(16.))
        .text_size(px(12.))
        .text_color(content(0.5))
        .child(text.into())
}

fn alert(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(12.))
        .text_color(rgb(0xf87171))
        .child(text.into())
}

/// The square icon buttons of the mail's bars (`size-7`).
fn tool(
    id: impl Into<SharedString>,
    label: &str,
    name: &'static str,
    ink: gpui::Hsla,
) -> Stateful<Div> {
    tool_enabled(id, label, name, ink, true)
}

fn tool_enabled(
    id: impl Into<SharedString>,
    label: &str,
    name: &'static str,
    ink: gpui::Hsla,
    enabled: bool,
) -> Stateful<Div> {
    let id: SharedString = id.into();
    let tip = label.to_owned();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(tip.clone())
        .size(px(28.))
        .rounded(px(6.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .when(enabled, |d| {
            d.cursor_pointer()
                .tooltip(chrome_tip(tip, None))
                .hover(|s| s.bg(content(0.08)))
        })
        .child(icon(name, 14.).text_color(ink))
}

/// The flat text buttons of the mail (`rounded-md px-2.5 py-1 text-[12px] hover:bg-content/8`).
fn text_button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    ink: f32,
) -> Stateful<Div> {
    let id: SharedString = id.into();
    let label: SharedString = label.into();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .flex_shrink_0()
        .px(px(10.))
        .py(px(4.))
        .text_size(px(12.))
        .text_color(content(ink))
        .cursor_pointer()
        .hover(|s| s.bg(content(0.08)))
        .child(label)
}

/// The app's primary action (`primary-action`): the accent when the user picked one.
fn primary(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    enabled: bool,
) -> Stateful<Div> {
    let id: SharedString = id.into();
    let label: SharedString = label.into();
    let (fill, ink) = primary_action(enabled).unwrap_or_else(|| {
        let mut ink = floating_fill();
        ink.a = 1.;
        (content(if enabled { 0.92 } else { 0.3 }), ink)
    });
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(28.))
        .px(px(10.))
        .bg(fill)
        .text_color(ink)
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .when(enabled, |d| d.cursor_pointer())
        .child(label)
}

/// A picture as GPUI paints it.
fn png_image(bytes: Vec<u8>) -> Arc<Image> {
    Arc::new(Image::from_bytes(ImageFormat::Png, bytes))
}

impl Mailbox {
    pub(crate) fn new(data_dir: PathBuf, auto_images: bool, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            let mut input = Input::field("Search mail", "Search mail", content(0.4), cx);
            input.set_font_size(12.);
            input
        });
        let form =
            cx.new(|cx| connect::ConnectForm::new(model::Service::new(data_dir.clone()), cx));
        let subscriptions = vec![
            cx.subscribe(&search, |this, _, _: &input::Changed, cx| {
                this.prefetch_ahead(cx);
                cx.notify();
            }),
            cx.subscribe(&form, |this, _, event: &connect::ConnectEvent, cx| {
                if let connect::ConnectEvent::Connected(_) = event {
                    this.load_accounts(cx);
                }
            }),
        ];
        // New mail, or a change made elsewhere: read again.
        let changes = model::subscribe();
        cx.spawn(async move |this, cx| {
            while changes.recv().await.is_ok() {
                if this
                    .update(cx, |this, cx| {
                        if this.accounts.as_ref().is_some_and(|a| !a.is_empty()) {
                            this.read(true, cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        Self {
            service: model::Service::new(data_dir),
            started: false,
            accounts: None,
            overview: None,
            view: model::View::Inbox,
            views_scroll: ScrollHandle::new(),
            views_overflow: (false, true),
            list_width: LIST_WIDTH,
            resize_start: None,
            limit_glow: None,
            account_id: None,
            account_menu: false,
            account_anchor: AnchorSlot::default(),
            search,
            selected: None,
            loading: false,
            error: None,
            ticket: 0,
            settings_open: false,
            auto_images,
            reader: None,
            opened: VecDeque::new(),
            reply: None,
            compose: None,
            snoozed: model::Snoozed::new(),
            snooze_menu: false,
            rules: Vec::new(),
            rule_form: None,
            rule_error: None,
            activity: Vec::new(),
            send_via: String::new(),
            form,
            access: model::Access::new(),
            agents: Vec::new(),
            readers_open: None,
            store: None,
            photos: HashMap::new(),
            hover: None,
            prefetched: Vec::new(),
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    pub(crate) fn set_list_width(&mut self, width: f32) {
        if width.is_finite() && width >= MIN_LIST_WIDTH {
            self.list_width = width.clamp(MIN_LIST_WIDTH, MAX_LIST_WIDTH);
        }
    }

    /// The tab came into view: the first time, read the accounts and their mail.
    pub(crate) fn activate(&mut self, cx: &mut Context<Self>) {
        if !self.started {
            self.started = true;
            art::warm(cx);
            self.load_accounts(cx);
        }
    }

    /// Native web views draw over the window: hide the message's until it paints again.
    pub(crate) fn hide_html(&self, cx: &App) {
        if let Some(html) = self.reader.as_ref().and_then(|r| r.html.as_ref()) {
            html.read(cx).hide();
        }
    }

    /// Whether Escape has something of the mail's to close first.
    pub(crate) fn escape(&mut self, cx: &mut Context<Self>) -> bool {
        if let Some(reader) = self.reader.as_mut()
            && reader.zoomed.take().is_some()
        {
            cx.notify();
            return true;
        }
        if self.account_menu {
            self.account_menu = false;
            cx.notify();
            return true;
        }
        if self.settings_open {
            if !self.form.read(cx).busy() {
                self.settings_open = false;
                cx.notify();
            }
            return true;
        }
        if self
            .compose
            .as_ref()
            .is_some_and(|compose| !compose.read(cx).busy())
        {
            self.compose = None;
            cx.notify();
            return true;
        }
        if self.reply.take().is_some() {
            cx.notify();
            return true;
        }
        false
    }

    /// Where the bots are listed, for choosing which of them read each account.
    pub(crate) fn set_store(&mut self, store: crate::runtime::Database) {
        self.store = Some(store);
    }

    fn load_accounts(&mut self, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let store = self.store.clone();
        let task = cx.background_executor().spawn(async move {
            let accounts = service.accounts();
            let last = if accounts.is_empty() {
                None
            } else {
                service.last()
            };
            let desktop =
                monocode_core::desktop_preferences::DesktopPreferences::load(&service.data_dir);
            let photos = accounts
                .iter()
                .filter_map(|account| {
                    let data = desktop.get(&model::photo_key(&account.address))?;
                    let png = model::picture_png(data, 128)?;
                    Some((account.address.to_lowercase(), png))
                })
                .collect::<Vec<_>>();
            let access = service.access();
            let snoozed = service.snoozed();
            let rules = service.rules();
            let activity = service.activity(40);
            let send_via = monocode_core::inbox::workspace::Service::new(&service.data_dir)
                .config()
                .map(|config| config.send_provider)
                .unwrap_or_default();
            let agents = store
                .and_then(|store| {
                    let db = crate::runtime::locked(&store).ok()?;
                    monocode_core::mono::Roster::load(&db).ok()
                })
                .map(|roster| connect::agents_of(&roster))
                .unwrap_or_default();
            (
                accounts,
                last,
                photos,
                access,
                agents,
                (snoozed, rules, activity, send_via),
            )
        });
        cx.spawn(async move |this, cx| {
            let (accounts, last, photos, access, agents, (snoozed, rules, activity, send_via)) =
                task.await;
            let _ = this.update(cx, |this, cx| {
                let empty = accounts.is_empty();
                this.access = access;
                this.snoozed = snoozed;
                this.rules = rules;
                this.activity = activity;
                this.send_via = send_via;
                this.form
                    .update(cx, |form, cx| form.set_agents(agents.clone(), cx));
                this.agents = agents;
                this.photos = photos
                    .into_iter()
                    .map(|(address, png)| (address, png_image(png)))
                    .collect();
                if this
                    .account_id
                    .as_ref()
                    .is_some_and(|id| !accounts.iter().any(|a| &a.id == id))
                {
                    this.account_id = None;
                }
                this.accounts = Some(accounts);
                if empty {
                    this.overview = None;
                    this.close_message(cx);
                } else {
                    // The last reading paints at once; the fresh one replaces it.
                    if this.overview.is_none() {
                        this.overview = last;
                    }
                    this.read(false, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn read(&mut self, force: bool, cx: &mut Context<Self>) {
        self.ticket += 1;
        let ticket = self.ticket;
        self.loading = true;
        self.error = None;
        let service = self.service.clone();
        let task = cx
            .background_executor()
            .spawn(async move { service.overview(force) });
        cx.spawn(async move |this, cx| {
            let next = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.ticket != ticket {
                    return;
                }
                this.loading = false;
                this.overview = Some(next);
                this.prefetch_ahead(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn archive_more(&mut self, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let task = cx
            .background_executor()
            .spawn(async move { service.archive_more() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(next) => this.overview = Some(next),
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn messages(&self) -> &[Message] {
        self.overview
            .as_ref()
            .map(|o| o.messages.as_slice())
            .unwrap_or_default()
    }

    fn shown(&self, cx: &App) -> Vec<&Message> {
        model::visible_now(
            self.messages(),
            self.view,
            self.account_id.as_deref(),
            &self.search.read(cx).content,
            &self.snoozed,
            model::now_ms(),
        )
    }

    fn selected_message(&self) -> Option<&Message> {
        let id = self.selected.as_ref()?;
        self.messages().iter().find(|m| &m.id == id)
    }

    /// Reads ahead what is likely opened next: the top of the list, and the messages beside
    /// the open one. Opening them then comes from memory.
    fn prefetch_ahead(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<String> = self.shown(cx).iter().map(|m| m.id.clone()).collect();
        let at = self
            .selected
            .as_ref()
            .and_then(|id| ids.iter().position(|shown| shown == id));
        let wanted: Vec<String> = match at {
            Some(at) => [Some(at + 1), at.checked_sub(1)]
                .into_iter()
                .flatten()
                .filter_map(|ix| ids.get(ix).cloned())
                .collect(),
            None => ids.into_iter().take(3).collect(),
        };
        if wanted.is_empty() || wanted == self.prefetched {
            return;
        }
        self.prefetched = wanted.clone();
        self.prefetch(wanted, cx);
    }

    fn prefetch(&self, ids: Vec<String>, cx: &mut Context<Self>) {
        let service = self.service.clone();
        cx.background_executor()
            .spawn(async move { service.prefetch(ids) })
            .detach();
    }

    fn prefetch_soon(&mut self, id: String, cx: &mut Context<Self>) {
        self.hover = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(120))
                .await;
            let _ = this.update(cx, |this, cx| this.prefetch(vec![id], cx));
        }));
    }

    fn act(&mut self, id: &str, action: Action, cx: &mut Context<Self>) {
        if let Some(overview) = self.overview.as_mut() {
            model::apply_action(&mut overview.messages, id, action);
        }
        let service = self.service.clone();
        let id = id.to_string();
        let task = cx
            .background_executor()
            .spawn(async move { service.act(&id, action) });
        cx.spawn(async move |this, cx| {
            if let Err(error) = task.await {
                let _ = this.update(cx, |this, cx| {
                    this.error = Some(error);
                    this.read(true, cx);
                });
            }
        })
        .detach();
        cx.notify();
    }

    fn mark_interesting(&mut self, address: String, on: bool, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let task = cx
            .background_executor()
            .spawn(async move { service.interesting(&address, on) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(()) => this.read(true, cx),
                Err(error) => {
                    this.error = Some(error);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn change_view(&mut self, view: model::View, cx: &mut Context<Self>) {
        self.view = view;
        if let Some(ix) = model::VIEWS.iter().position(|(v, _)| *v == view) {
            self.views_scroll.scroll_to_item(ix);
        }
        self.views_overflow = self.measured_views_overflow();
        self.close_message(cx);
        self.prefetch_ahead(cx);
        cx.notify();
    }

    /// Matches `stripOverflow(scrollLeft, clientWidth, scrollWidth)` in `ScrollStrip.tsx`.
    fn measured_views_overflow(&self) -> (bool, bool) {
        let width = f32::from(self.views_scroll.bounds().size.width);
        let max = f32::from(self.views_scroll.max_offset().x);
        if width <= 0. {
            return (false, self.list_width < 390.);
        }
        if max <= 1. {
            return (false, false);
        }
        let scroll_left = (-f32::from(self.views_scroll.offset().x)).clamp(0., max);
        (scroll_left > 1., scroll_left < max - 1.)
    }

    fn scroll_views(&mut self, direction: f32, cx: &mut Context<Self>) {
        let width = f32::from(self.views_scroll.bounds().size.width).max(self.list_width);
        let step = (width * 0.6).max(96.);
        self.scroll_views_by(-direction * step, cx);
    }

    fn scroll_views_by(&mut self, delta_x: f32, cx: &mut Context<Self>) {
        let max = f32::from(self.views_scroll.max_offset().x).max(0.);
        let current = f32::from(self.views_scroll.offset().x);
        let next = (current + delta_x).clamp(-max, 0.);
        self.views_scroll.set_offset(point(px(next), px(0.)));
        self.views_overflow = self.measured_views_overflow();
        cx.notify();
    }

    fn close_message(&mut self, cx: &mut Context<Self>) {
        self.selected = None;
        self.reader = None;
        self.reply = None;
        cx.notify();
    }

    fn open(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        if self.selected.as_ref() == Some(&id) {
            return;
        }
        if self.messages().iter().any(|m| m.id == id && m.unread) {
            self.act(&id, Action::Read, cx);
        }
        self.selected = Some(id.clone());
        self.reply = None;
        self.reader = Some(Reader::new(id.clone()));
        if let Some(cached) = self.opened.iter().find(|o| o.id == id).cloned() {
            self.show_opened(cached, cx);
        } else {
            let service = self.service.clone();
            let task = cx.background_executor().spawn({
                let id = id.clone();
                async move { service.open(&id) }
            });
            cx.spawn(async move |this, cx| {
                let result = task.await;
                let _ = this.update(cx, |this, cx| {
                    if this.reader.as_ref().is_none_or(|r| r.id != id) {
                        if let Ok(opened) = result {
                            this.remember(opened);
                        }
                        return;
                    }
                    match result {
                        Ok(opened) => this.show_opened(opened, cx),
                        Err(error) => {
                            if let Some(reader) = this.reader.as_mut() {
                                reader.error = Some(error);
                            }
                            cx.notify();
                        }
                    }
                });
            })
            .detach();
        }
        self.prefetch_ahead(cx);
        cx.notify();
    }

    fn remember(&mut self, opened: Opened) {
        self.opened.retain(|o| o.id != opened.id);
        self.opened.push_back(opened);
        while self.opened.len() > OPENED_KEPT {
            self.opened.pop_front();
        }
    }

    fn show_opened(&mut self, opened: Opened, cx: &mut Context<Self>) {
        self.remember(opened.clone());
        let held = self
            .selected_message()
            .is_some_and(|m| m.triage == model::Triage::Held);
        let Some(reader) = self.reader.as_mut().filter(|r| r.id == opened.id) else {
            return;
        };
        if cfg!(any(target_os = "macos", target_os = "windows")) && !opened.html.is_empty() {
            reader.html = Some(cx.new(|_| Html::mail(opened.html.clone())));
        }
        let previews: Vec<(usize, String)> = opened
            .attachments
            .iter()
            .filter_map(|a| a.preview.clone().map(|p| (a.index, p)))
            .collect();
        let has_remote = !opened.remote.is_empty();
        if let Some(reply) = self.reply.as_ref() {
            reply.update(cx, |reply, cx| reply.set_opened(opened.clone(), cx));
        }
        reader.opened = Some(opened);
        if !previews.is_empty() {
            let id = reader.id.clone();
            let task = cx.background_executor().spawn(async move {
                previews
                    .into_iter()
                    .filter_map(|(index, data)| Some((index, model::picture_png(&data, 1600)?)))
                    .collect::<Vec<_>>()
            });
            cx.spawn(async move |this, cx| {
                let pictures = task.await;
                let _ = this.update(cx, |this, cx| {
                    if let Some(reader) = this.reader.as_mut().filter(|r| r.id == id) {
                        reader.pictures = pictures
                            .into_iter()
                            .map(|(index, png)| (index, png_image(png)))
                            .collect();
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        // A message's pictures load as it opens, unless the user turned that off or the
        // message is held back as a possible scam: those wait for a press.
        if has_remote && !held && self.auto_images {
            self.load_images(cx);
        }
        cx.notify();
    }

    /// Fetches the message's pictures on the web, a few at a time, painting them as they land.
    fn load_images(&mut self, cx: &mut Context<Self>) {
        let Some(reader) = self.reader.as_mut() else {
            return;
        };
        let Some(opened) = reader.opened.as_ref() else {
            return;
        };
        reader.run += 1;
        let run = reader.run;
        reader.loading_images = true;
        let id = reader.id.clone();
        let urls: Vec<String> = opened.remote.iter().take(MAX_IMAGES).cloned().collect();
        let service = self.service.clone();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            for batch in urls.chunks(IMAGES_AT_ONCE) {
                let batch = batch.to_vec();
                let service = service.clone();
                let message = id.clone();
                let landed = executor
                    .spawn(async move {
                        std::thread::scope(|scope| {
                            let jobs: Vec<_> = batch
                                .iter()
                                .map(|url| {
                                    let service = &service;
                                    let message = &message;
                                    scope.spawn(move || (url.clone(), service.image(message, url)))
                                })
                                .collect();
                            jobs.into_iter()
                                .filter_map(|job| job.join().ok())
                                // One picture that fails leaves the rest.
                                .filter_map(|(url, data)| data.ok().map(|data| (url, data)))
                                .collect::<Vec<_>>()
                        })
                    })
                    .await;
                let going = this
                    .update(cx, |this, cx| {
                        let Some(reader) =
                            this.reader.as_mut().filter(|r| r.id == id && r.run == run)
                        else {
                            return false;
                        };
                        if !landed.is_empty() {
                            reader.images.extend(landed);
                            this.paint_images(cx);
                        }
                        true
                    })
                    .unwrap_or(false);
                if !going {
                    return;
                }
            }
            let _ = this.update(cx, |this, cx| {
                if let Some(reader) = this.reader.as_mut().filter(|r| r.id == id && r.run == run) {
                    reader.loading_images = false;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn paint_images(&mut self, cx: &mut Context<Self>) {
        let Some(reader) = self.reader.as_ref() else {
            return;
        };
        let (Some(opened), Some(html)) = (reader.opened.as_ref(), reader.html.as_ref()) else {
            return;
        };
        let body = model::swap_images(&opened.html, &reader.images);
        html.update(cx, |html, cx| html.set_body(body, cx));
        cx.notify();
    }

    fn open_attachment(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(id) = self.reader.as_ref().map(|r| r.id.clone()) else {
            return;
        };
        let service = self.service.clone();
        let task = cx.background_executor().spawn({
            let id = id.clone();
            async move {
                let path = service.attachment(&id, index)?;
                monocode_core::files::open_path_with_default_app(&path)
            }
        });
        cx.spawn(async move |this, cx| {
            if let Err(error) = task.await {
                let _ = this.update(cx, |this, cx| {
                    if let Some(reader) = this.reader.as_mut().filter(|r| r.id == id) {
                        reader.notice = Some(error);
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    fn set_auto_images(&mut self, on: bool, cx: &mut Context<Self>) {
        self.auto_images = on;
        cx.emit(MailEvent::AutoImages(on));
        cx.notify();
    }

    fn start_reply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(message) = self.selected_message().cloned() else {
            return;
        };
        if message.triage == model::Triage::Held {
            return;
        }
        if self.reply.is_some() {
            return;
        }
        let account = self
            .accounts
            .as_ref()
            .and_then(|accounts| accounts.iter().find(|a| a.id == message.account_id))
            .cloned();
        let opened = self.reader.as_ref().and_then(|r| r.opened.clone());
        let photo = account
            .as_ref()
            .and_then(|a| self.photos.get(&a.address.to_lowercase()).cloned());
        let service = self.service.clone();
        let reply =
            cx.new(|cx| MailReply::new(service, message, account, photo, opened, window, cx));
        self._subscriptions.push(cx.subscribe(
            &reply,
            |this, reply, event: &reply::ReplyEvent, cx| {
                if this.reply.as_ref() != Some(&reply) {
                    return;
                }
                match event {
                    reply::ReplyEvent::Close => this.reply = None,
                    reply::ReplyEvent::Sent => {
                        this.reply = None;
                        if let Some(reader) = this.reader.as_mut() {
                            reader.notice = None;
                        }
                    }
                }
                cx.notify();
            },
        ));
        self.reply = Some(reply);
        cx.notify();
    }

    /// A new message (`forward`: the forward of the open one) in place of the reader.
    fn start_compose(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.compose.is_some() {
            return;
        }
        let accounts: Vec<AccountView> = self
            .accounts
            .clone()
            .unwrap_or_default()
            .into_iter()
            .filter(|a| a.provider != "cloudflare")
            .collect();
        if accounts.is_empty() {
            return;
        }
        let selected = self.selected_message().cloned();
        let (from, forwarded) = match (forward, selected) {
            (true, Some(message)) => {
                if message.triage == model::Triage::Held {
                    return;
                }
                let opened = self.reader.as_ref().and_then(|r| r.opened.as_ref());
                (
                    Some(message.account_id.clone()),
                    Some(model::forward_of(&message, opened)),
                )
            }
            (true, None) => return,
            (false, _) => (self.account_id.clone(), None),
        };
        let service = self.service.clone();
        let compose = cx.new(|cx| {
            compose::MailCompose::new(service, accounts, from.as_deref(), forwarded, window, cx)
        });
        self._subscriptions.push(cx.subscribe(
            &compose,
            |this, compose, event: &compose::ComposeEvent, cx| {
                if this.compose.as_ref() != Some(&compose) {
                    return;
                }
                this.compose = None;
                if let compose::ComposeEvent::Sent(note) = event
                    && let Some(reader) = this.reader.as_mut()
                {
                    reader.notice = Some(note.clone());
                }
                cx.notify();
            },
        ));
        self.compose = Some(compose);
        cx.notify();
    }

    /// J/K move, E archives, S stars, R replies: outside text fields only.
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus.is_focused(window) {
            return;
        }
        let keys = &event.keystroke;
        if keys.modifiers.platform || keys.modifiers.control || keys.modifiers.alt {
            return;
        }
        let shown: Vec<String> = self.shown(cx).iter().map(|m| m.id.clone()).collect();
        let index = self
            .selected
            .as_ref()
            .and_then(|id| shown.iter().position(|s| s == id));
        let selected = self.selected_message().cloned();
        match keys.key.as_str() {
            "j" | "k" => {
                let next = match (index, keys.key.as_str()) {
                    (Some(at), "j") => (at + 1).min(shown.len().saturating_sub(1)),
                    (Some(at), _) => at.saturating_sub(1),
                    (None, _) => 0,
                };
                if let Some(id) = shown.get(next).cloned() {
                    self.open(id, window, cx);
                }
            }
            "e" => match selected {
                Some(message) if message.folder != model::Folder::Archive => {
                    self.act(&message.id, Action::Archive, cx)
                }
                _ => return,
            },
            "s" => match selected {
                Some(message) => self.act(
                    &message.id,
                    if message.starred {
                        Action::Unstar
                    } else {
                        Action::Star
                    },
                    cx,
                ),
                None => return,
            },
            "r" if selected.is_some() => self.start_reply(window, cx),
            "c" => self.start_compose(false, window, cx),
            "f" if selected.is_some() => self.start_compose(true, window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn views_strip(&self, cx: &mut Context<Self>) -> AnyElement {
        let messages = self.messages();
        let (overflow_left, overflow_right) = self.views_overflow;
        let base = background_base();
        let mut clear = base;
        clear.a = 0.;
        let entity = cx.entity();
        let drawn_overflow = self.views_overflow;
        let arrow = |id: &'static str,
                     label: &'static str,
                     glyph: &'static str,
                     direction: f32,
                     cx: &mut Context<Self>| {
            div()
                .id(id)
                .debug_selector(move || id.into())
                .role(Role::Button)
                .aria_label(label)
                .tooltip(chrome_tip(label, None))
                .size(px(24.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .bg(base.blend(content(0.1)))
                .hover(move |s| s.bg(base.blend(content(0.16))))
                .child(icon(glyph, 14.).text_color(content(0.75)))
                .on_click(cx.listener(move |this, _, _, cx| this.scroll_views(direction, cx)))
        };
        div()
            .relative()
            .min_w_0()
            .flex_shrink_0()
            .border_b_1()
            .border_color(content(0.07))
            .child(
                div()
                    .id("mail-views")
                    .role(Role::TabList)
                    .aria_label("folders")
                    .track_scroll(&self.views_scroll)
                    .on_scroll_wheel(cx.listener(|this, event: &gpui::ScrollWheelEvent, _, cx| {
                        let delta = event.delta.pixel_delta(px(18.));
                        let step = if delta.x.abs() > delta.y.abs() {
                            f32::from(delta.x)
                        } else {
                            f32::from(delta.y)
                        };
                        this.scroll_views_by(step, cx);
                        cx.stop_propagation();
                    }))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(8.))
                    .py(px(6.))
                    .overflow_x_scroll()
                    .children(model::VIEWS.into_iter().map(|(view, label)| {
                        let count = if view == model::View::Snoozed {
                            let now = model::now_ms();
                            messages
                                .iter()
                                .filter(|m| {
                                    self.snoozed.get(&m.id).is_some_and(|until| *until > now)
                                })
                                .count()
                        } else {
                            model::unread_count(messages, view)
                        };
                        let active = self.view == view;
                        div()
                            .id(SharedString::from(format!("mail-view-{}", view.id())))
                            .role(Role::Tab)
                            .aria_label(label)
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .px(px(8.))
                            .py(px(4.))
                            .rounded(px(6.))
                            .text_size(px(12.))
                            .cursor_pointer()
                            .text_color(content(if active { 1. } else { 0.55 }))
                            .when(active, |d| d.bg(content(0.1)))
                            .when(!active, |d| {
                                d.hover(|s| s.bg(content(0.05)).text_color(content(1.)))
                            })
                            .child(label)
                            .when(count > 0 && view != model::View::Held, |d| {
                                d.child(
                                    div()
                                        .text_size(px(10.))
                                        .text_color(accent_color())
                                        .child(count.to_string()),
                                )
                            })
                            .on_click(cx.listener(move |this, _, _, cx| this.change_view(view, cx)))
                    })),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, cx| {
                        let current = entity.read(cx).measured_views_overflow();
                        if current != drawn_overflow {
                            let entity = entity.clone();
                            window.defer(cx, move |_, cx| {
                                entity.update(cx, |this, cx| {
                                    let next = this.measured_views_overflow();
                                    if this.views_overflow != next {
                                        this.views_overflow = next;
                                        cx.notify();
                                    }
                                });
                            });
                        }
                    },
                )
                .size_0(),
            )
            .when(overflow_left, |d| {
                d.child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(VIEWS_FADE))
                        .bg(linear_gradient(
                            90.,
                            linear_color_stop(base, 0.),
                            linear_color_stop(clear, 1.),
                        ))
                        .flex()
                        .items_center()
                        .pl(px(4.))
                        .child(arrow(
                            "mail-views-left",
                            "Scroll folders left",
                            "chevron-left",
                            -1.,
                            cx,
                        )),
                )
            })
            .when(overflow_right, |d| {
                d.child(
                    div()
                        .absolute()
                        .right_0()
                        .top_0()
                        .bottom_0()
                        .w(px(VIEWS_FADE))
                        .bg(linear_gradient(
                            90.,
                            linear_color_stop(clear, 0.),
                            linear_color_stop(base, 1.),
                        ))
                        .flex()
                        .items_center()
                        .justify_end()
                        .pr(px(4.))
                        .child(arrow(
                            "mail-views-right",
                            "Scroll folders right",
                            "chevron-right",
                            1.,
                            cx,
                        )),
                )
            })
            .into_any_element()
    }

    fn tools_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let accounts = self.accounts.clone().unwrap_or_default();
        let account_label = self
            .account_id
            .as_ref()
            .and_then(|id| accounts.iter().find(|a| &a.id == id))
            .map(|a| a.name.clone())
            .unwrap_or_else(|| "All accounts".into());
        div()
            .h(px(36.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(8.))
            .border_b_1()
            .border_color(content(0.07))
            .child(
                div()
                    .h(px(28.))
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(8.))
                    .child(icon("search", 12.))
                    .child(div().min_w_0().flex_1().child(self.search.clone())),
            )
            .when(accounts.len() > 1, |d| {
                d.child(
                    div()
                        .id("mail-account")
                        .role(Role::ComboBox)
                        .aria_label("Account")
                        .relative()
                        .h(px(28.))
                        .max_w(px(112.))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .px(px(6.))
                        .text_size(px(12.))
                        .text_color(content(0.7))
                        .cursor_pointer()
                        .hover(|s| s.bg(content(0.05)))
                        .child(self.account_anchor.probe())
                        .child(div().min_w_0().truncate().child(account_label))
                        .child(icon("chevron-down", 12.))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.account_menu = !this.account_menu;
                            cx.notify();
                        })),
                )
            })
            .child(
                tool("mail-new", "New message (C)", "pencil", content(0.55)).on_click(
                    cx.listener(|this, _, window, cx| this.start_compose(false, window, cx)),
                ),
            )
            .child(
                tool(
                    "mail-refresh",
                    "Refresh",
                    if self.loading { "loader" } else { "refresh-cw" },
                    content(0.55),
                )
                .on_click(cx.listener(|this, _, _, cx| this.read(true, cx))),
            )
            .child(
                tool("mail-settings", "Mail accounts", "settings", content(0.55)).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.settings_open = true;
                        cx.notify();
                    }),
                ),
            )
            .into_any_element()
    }

    fn account_menu_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let accounts = self.accounts.clone().unwrap_or_default();
        let row = |id: SharedString, label: String, checked: bool| {
            div()
                .id(id)
                .role(Role::MenuItemRadio)
                .aria_label(label.clone())
                .h(px(28.))
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(12.))
                .text_color(content(0.8))
                .cursor_pointer()
                .hover(|s| s.bg(content(0.06)))
                .child(
                    div()
                        .size(px(14.))
                        .when(checked, |d| d.child(icon("check", 14.))),
                )
                .child(div().min_w_0().flex_1().truncate().child(label))
        };
        popover(
            "mail-account-menu",
            Anchor::Slot(self.account_anchor.clone()),
        )
        .width(200.)
        .padding(4.)
        .on_dismiss(move |_, cx| {
            let _ = weak.update(cx, |this, cx| {
                this.account_menu = false;
                cx.notify();
            });
        })
        .child(
            row(
                "mail-account-all".into(),
                "All accounts".into(),
                self.account_id.is_none(),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.account_id = None;
                this.account_menu = false;
                this.prefetch_ahead(cx);
                cx.notify();
            })),
        )
        .children(accounts.into_iter().map(|account| {
            let id = account.id.clone();
            row(
                SharedString::from(format!("mail-account-{}", account.id)),
                account.name.clone(),
                self.account_id.as_ref() == Some(&account.id),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.account_id = Some(id.clone());
                this.account_menu = false;
                this.prefetch_ahead(cx);
                cx.notify();
            }))
        }))
        .into_any_element()
    }

    fn list(&self, cx: &mut Context<Self>) -> AnyElement {
        let now = model::now_ms();
        let shown = self.shown(cx);
        let searching = !self.search.read(cx).content.is_empty();
        let mut problems: Vec<String> = self
            .overview
            .as_ref()
            .map(|o| o.problems.clone())
            .unwrap_or_default();
        problems.extend(self.error.clone());
        let body: Vec<AnyElement> = if self.overview.is_none() && self.loading {
            vec![caption("Reading your mail…").into_any_element()]
        } else if shown.is_empty() {
            vec![
                caption(if searching {
                    "Nothing matches."
                } else {
                    "Nothing here."
                })
                .into_any_element(),
            ]
        } else {
            let threads = model::by_thread(&shown);
            let heads: Vec<&Message> = threads.iter().map(|(m, _)| *m).collect();
            let counts: HashMap<String, usize> =
                threads.iter().map(|(m, n)| (m.id.clone(), *n)).collect();
            model::by_day(&heads, now)
                .into_iter()
                .map(|(day, messages)| {
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .px(px(12.))
                                .pb(px(4.))
                                .pt(px(8.))
                                .text_size(px(11.))
                                .text_color(content(0.45))
                                .child(day),
                        )
                        .children(messages.into_iter().map(|m| {
                            let count = counts.get(&m.id).copied().unwrap_or(1);
                            self.row(m, count, now, cx)
                        }))
                        .into_any_element()
                })
                .collect()
        };
        let archive = self.view == model::View::Archive && !shown.is_empty();
        div()
            .relative()
            .w(px(self.list_width.clamp(MIN_LIST_WIDTH, MAX_LIST_WIDTH)))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(content(0.07))
            .child(self.views_strip(cx))
            .child(self.tools_bar(cx))
            .children(problems.into_iter().map(|problem| {
                div()
                    .flex_shrink_0()
                    .px(px(12.))
                    .py(px(8.))
                    .border_b_1()
                    .border_color(content(0.07))
                    .child(alert(problem))
            }))
            .child(
                div()
                    .id("mail-list")
                    .role(Role::List)
                    .aria_label("Messages")
                    .min_h_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .children(body)
                    .when(archive, |d| {
                        d.child(
                            div().p(px(8.)).child(
                                text_button("mail-load-more", "Load more", 0.6)
                                    .on_click(cx.listener(|this, _, _, cx| this.archive_more(cx))),
                            ),
                        )
                    }),
            )
            .child(
                div()
                    .id("mail-list-resize")
                    .debug_selector(|| "mail-list-resize".into())
                    .role(Role::Splitter)
                    .aria_label("Resize mail list")
                    .absolute()
                    .right(px(-3.))
                    .top_0()
                    .bottom_0()
                    .w(px(6.))
                    .cursor(CursorStyle::ResizeLeftRight)
                    .hover(|s| s.bg(content(0.1)))
                    .when(self.resize_start.is_some(), |d| d.bg(content(0.15)))
                    .when_some(self.limit_glow.clone(), |d, glow| {
                        d.child(limit_glow(Axis::X, glow, cx.reduce_motion()))
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                            if event.click_count == 2 {
                                this.list_width = LIST_WIDTH;
                                this.resize_start = None;
                                if let Some(glow) = &mut this.limit_glow {
                                    glow.release();
                                }
                                cx.emit(MailEvent::ListWidth(this.list_width));
                            } else {
                                this.resize_start =
                                    Some((f32::from(event.position.x), this.list_width));
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    ),
            )
            .into_any_element()
    }

    fn row(&self, message: &Message, count: usize, now: i64, cx: &mut Context<Self>) -> AnyElement {
        let id = message.id.clone();
        let hover_id = message.id.clone();
        let active = self.selected.as_ref() == Some(&message.id);
        let unread = message.unread;
        div()
            .id(SharedString::from(format!("mail-row-{}", message.id)))
            .role(Role::Button)
            .aria_label(if message.subject.is_empty() {
                "(no subject)".to_string()
            } else {
                message.subject.clone()
            })
            .w_full()
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(12.))
            .py(px(8.))
            .border_b_1()
            .border_color(content(0.05))
            .cursor_pointer()
            .when(active, |d| d.bg(content(0.1)))
            .when(!active, |d| d.hover(|s| s.bg(content(0.04))))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(12.))
                    .when(unread, |d| {
                        d.child(
                            div()
                                .size(px(6.))
                                .flex_shrink_0()
                                .rounded_full()
                                .bg(accent_color()),
                        )
                    })
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .when(unread, |d| {
                                d.font_weight(FontWeight::MEDIUM).text_color(content(1.))
                            })
                            .when(!unread, |d| d.text_color(content(0.75)))
                            .child(message.from.clone()),
                    )
                    .when(count > 1, |d| {
                        d.child(
                            div()
                                .flex_shrink_0()
                                .text_size(px(10.))
                                .text_color(content(0.5))
                                .child(count.to_string()),
                        )
                    })
                    .when(message.starred, |d| {
                        d.child(icon("star-filled", 12.).text_color(accent_color()))
                    })
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(11.))
                            .text_color(content(0.4))
                            .child(match self.snoozed.get(&message.id) {
                                Some(until) if *until > now => {
                                    format!("until {}", model::row_time(*until, now))
                                }
                                _ => model::row_time(message.at, now),
                            }),
                    ),
            )
            .child(
                div()
                    .truncate()
                    .text_size(px(12.))
                    .text_color(content(if unread { 1. } else { 0.7 }))
                    .child(if message.subject.is_empty() {
                        "(no subject)".to_string()
                    } else {
                        message.subject.clone()
                    }),
            )
            .child(
                div()
                    .truncate()
                    .text_size(px(11.))
                    .text_color(content(0.45))
                    .child(message.preview.clone()),
            )
            .when(!message.labels.is_empty(), |d| {
                d.child(organize::label_chips(&message.labels))
            })
            .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                if *hovering {
                    this.prefetch_soon(hover_id.clone(), cx);
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| this.open(id.clone(), window, cx)))
            .into_any_element()
    }

    /// Beside the list when no message is open: the art, and where the inbox stands.
    fn idle(&self) -> AnyElement {
        let unread = model::unread_count(self.messages(), model::View::Inbox);
        art::art(Some(
            div()
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(content(1.))
                        .child(if unread > 0 {
                            format!("{unread} unread in your inbox")
                        } else {
                            "You're up to date".into()
                        }),
                )
                .child(
                    div()
                        .mt(px(4.))
                        .text_size(px(12.))
                        .text_color(content(0.6))
                        .child("Pick a message · J/K to move, E archive, S star, R reply"),
                )
                .into_any_element(),
        ))
    }

    /// No account yet: what the tab is for, and the form to connect one.
    fn welcome(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        div()
            .size_full()
            .flex()
            .child(
                div()
                    .id("mail-welcome")
                    .min_w_0()
                    .min_h_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .items_center()
                    .p(px(20.))
                    .child(
                        div()
                            .w_full()
                            .max_w(px(448.))
                            .my_auto()
                            .py(px(16.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(content(1.))
                                    .child(icon("mail", 16.).text_color(accent_color()))
                                    .child("Your email in the Inbox"),
                            )
                            .child(
                                div()
                                    .mt(px(4.))
                                    .mb(px(20.))
                                    .text_size(px(13.))
                                    .line_height(px(19.))
                                    .text_color(content(0.6))
                                    .child("Read your mail beside GitHub, sort it, and answer with a reply AI drafts for you to review. Connect an account with an app password."),
                            )
                            .child(self.accounts_view(window, cx)),
                    ),
            )
            .child(
                div()
                    .w(gpui::relative(0.42))
                    .h_full()
                    .flex_shrink_0()
                    .border_l_1()
                    .border_color(content(0.07))
                    .child(art::art(None)),
            )
            .into_any_element()
    }

    fn settings_dialog(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("mail-accounts-dismiss")
                    .absolute()
                    .inset_0()
                    .child(crate::modal::scrim("mail-accounts-scrim"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if !this.form.read(cx).busy() {
                            this.settings_open = false;
                            cx.notify();
                        }
                    })),
            )
            .child(crate::modal::enter(
                "mail-accounts-card",
                div()
                    .id("mail-accounts-dialog")
                    .role(Role::Dialog)
                    .aria_label("Mail accounts")
                    .occlude()
                    .relative()
                    .w(px(480.))
                    .max_w_full()
                    .max_h(gpui::relative(0.85))
                    .border_1()
                    .border_color(content(crate::modal::BORDER))
                    .child(crate::modal::surface(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .relative()
                            .flex()
                            .items_center()
                            .px(px(20.))
                            .pt(px(16.))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(content(1.))
                                    .child("Mail accounts"),
                            )
                            .child(
                                tool("mail-accounts-close", "Close", "close", content(0.5))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if !this.form.read(cx).busy() {
                                            this.settings_open = false;
                                            cx.notify();
                                        }
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .id("mail-accounts-body")
                            .relative()
                            .min_h_0()
                            .overflow_y_scroll()
                            .px(px(20.))
                            .py(px(16.))
                            .child(self.accounts_view(window, cx)),
                    ),
            ))
            .into_any_element()
    }
}

impl gpui::Render for Mailbox {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Shown again by its own paint when the reader is still on screen.
        self.hide_html(cx);
        let root = div()
            .id("mail-inbox")
            .track_focus(&self.focus)
            .key_context("MailInbox")
            .relative()
            .size_full()
            .min_h_0()
            .min_w_0()
            .flex()
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_move(
                cx.listener(|this, event: &gpui::MouseMoveEvent, window, cx| {
                    if let Some((start, width)) = this.resize_start {
                        let max = (f32::from(window.viewport_size().width) * 0.5)
                            .clamp(MIN_LIST_WIDTH, MAX_LIST_WIDTH);
                        let wanted = width + f32::from(event.position.x) - start;
                        let applied = wanted.clamp(MIN_LIST_WIDTH, max);
                        this.list_width = applied;
                        // Past the limit the handle glows near the pointer.
                        let along = f32::from(event.position.y);
                        match &mut this.limit_glow {
                            Some(glow) => glow.paint(wanted - applied, along),
                            None => this.limit_glow = Some(LimitGlow::new(wanted - applied, along)),
                        }
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.resize_start.take().is_some() {
                        if let Some(glow) = &mut this.limit_glow {
                            glow.release();
                        }
                        cx.emit(MailEvent::ListWidth(this.list_width));
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.resize_start.take().is_some() {
                        if let Some(glow) = &mut this.limit_glow {
                            glow.release();
                        }
                        cx.emit(MailEvent::ListWidth(this.list_width));
                        cx.notify();
                    }
                }),
            );
        let Some(accounts) = self.accounts.as_ref() else {
            return root
                .child(
                    div()
                        .p(px(20.))
                        .text_size(px(13.))
                        .text_color(content(0.5))
                        .child("Loading mail…"),
                )
                .into_any_element();
        };
        if accounts.is_empty() {
            return root.child(self.welcome(window, cx)).into_any_element();
        }
        let detail = if let Some(compose) = self.compose.clone() {
            compose.into_any_element()
        } else if self.selected_message().is_some() {
            self.reader_view(window, cx)
        } else {
            self.idle()
        };
        let dialog = self.settings_open.then(|| self.settings_dialog(window, cx));
        root.child(self.list(cx))
            .child(div().min_w_0().min_h_0().flex_1().child(detail))
            .when(self.account_menu, |d| d.child(self.account_menu_view(cx)))
            .when_some(dialog, |d, dialog| {
                d.child(crate::modal::window_overlay(window, dialog))
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, TestAppContext, VisualTestContext};

    fn message(id: &str, ago: i64, unread: bool) -> Message {
        serde_json::from_value(serde_json::json!({
            "id": id, "accountId": "a", "folder": "inbox", "from": "Ana", "address": "ana@mina.pe",
            "subject": "Pozas", "preview": "Te envío los planos", "body": [],
            "at": model::now_ms() - ago, "unread": unread, "starred": false,
            "triage": "routine", "labels": []
        }))
        .unwrap()
    }

    fn mailbox<'a>(
        cx: &'a mut TestAppContext,
        name: &str,
    ) -> (Entity<Mailbox>, &'a mut VisualTestContext) {
        let dir = std::env::temp_dir().join(format!("monobot-mail-{name}-{}", std::process::id()));
        cx.add_window_view(move |_, cx| Mailbox::new(dir, true, cx))
    }

    #[gpui::test]
    fn without_accounts_the_tab_offers_to_connect_one(cx: &mut TestAppContext) {
        let (mailbox, cx) = mailbox(cx, "empty");
        mailbox.update(cx, |mailbox, cx| mailbox.activate(cx));
        cx.run_until_parked();
        mailbox.read_with(cx, |mailbox, _| {
            assert!(mailbox.accounts.as_ref().is_some_and(Vec::is_empty));
            assert!(mailbox.overview.is_none());
        });
    }

    #[gpui::test]
    fn a_view_lists_newest_first_and_an_action_shows_at_once(cx: &mut TestAppContext) {
        let (mailbox, cx) = mailbox(cx, "list");
        mailbox.update(cx, |mailbox, cx| {
            mailbox.started = true;
            mailbox.accounts = Some(vec![AccountView {
                id: "a".into(),
                name: "Ana".into(),
                address: "ana@gmail.com".into(),
                provider: "gmail".into(),
            }]);
            mailbox.overview = Some(Overview {
                messages: vec![
                    message("a.inbox.2", 120_000, false),
                    message("a.inbox.1", 60_000, true),
                ],
                ..Default::default()
            });
            let ids = |mailbox: &Mailbox, cx: &App| {
                mailbox
                    .shown(cx)
                    .iter()
                    .map(|m| m.id.clone())
                    .collect::<Vec<_>>()
            };
            assert_eq!(ids(mailbox, cx), ["a.inbox.1", "a.inbox.2"]);
            assert_eq!(
                model::unread_count(mailbox.messages(), model::View::Inbox),
                1
            );
            mailbox.act("a.inbox.1", Action::Star, cx);
            mailbox.act("a.inbox.1", Action::Archive, cx);
            assert_eq!(ids(mailbox, cx), ["a.inbox.2"]);
            mailbox.change_view(model::View::Starred, cx);
            assert_eq!(ids(mailbox, cx), ["a.inbox.1"]);
            mailbox.selected = Some("a.inbox.1".into());
            mailbox.reader = Some(Reader::new("a.inbox.1".into()));
            assert!(!mailbox.escape(cx));
            mailbox.settings_open = true;
            assert!(mailbox.escape(cx) && !mailbox.settings_open);
            cx.notify();
        });
        cx.run_until_parked();
    }

    #[gpui::test]
    fn a_rule_is_made_with_clicks_and_hands_bank_mail_to_an_agent(cx: &mut TestAppContext) {
        let (mailbox, cx) = mailbox(cx, "rules");
        let dir = mailbox.read_with(cx, |m, _| m.service.data_dir.clone());
        let _ = std::fs::remove_dir_all(dir.join("mail"));
        mailbox.update(cx, |mailbox, cx| {
            mailbox.started = true;
            mailbox.accounts = Some(vec![AccountView {
                id: "a".into(),
                name: "Ana".into(),
                address: "ana@gmail.com".into(),
                provider: "gmail".into(),
            }]);
            mailbox.overview = Some(Overview::default());
            mailbox.agents = vec![connect::Agent {
                id: "fin".into(),
                label: "Finance".into(),
            }];
            mailbox.settings_open = true;
            cx.notify();
        });
        cx.run_until_parked();
        cx.refresh().unwrap();
        let add = cx.debug_bounds("mail-rule-add").expect("Add rule");
        cx.simulate_click(add.center(), Modifiers::default());
        cx.run_until_parked();
        mailbox.update(cx, |mailbox, cx| {
            let form = mailbox.rule_form.as_ref().unwrap();
            let (from, label) = (form.from.clone(), form.label.clone());
            from.update(cx, |i, cx| i.set_text("bcp, yape".into(), cx));
            label.update(cx, |i, cx| i.set_text("Finanzas".into(), cx));
        });
        cx.refresh().unwrap();
        let finance = cx
            .debug_bounds("mail-rule-agent-fin")
            .expect("the Finance choice");
        cx.simulate_click(finance.center(), Modifiers::default());
        cx.run_until_parked();
        cx.refresh().unwrap();
        let save = cx.debug_bounds("mail-rule-save").expect("Save rule");
        cx.simulate_click(save.center(), Modifiers::default());
        cx.run_until_parked();
        let rules = model::Service::new(&dir).rules();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].from, "bcp, yape");
        assert_eq!(rules[0].label, "Finanzas");
        assert_eq!(rules[0].agent, "fin");
        mailbox.read_with(cx, |mailbox, _| {
            assert!(mailbox.rule_form.is_none());
            assert_eq!(mailbox.rules.len(), 1);
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui::test]
    fn a_snoozed_message_leaves_the_inbox_for_the_snoozed_view(cx: &mut TestAppContext) {
        let (mailbox, cx) = mailbox(cx, "snooze");
        let dir = mailbox.read_with(cx, |m, _| m.service.data_dir.clone());
        let _ = std::fs::remove_dir_all(dir.join("mail"));
        mailbox.update(cx, |mailbox, cx| {
            mailbox.started = true;
            mailbox.accounts = Some(vec![AccountView {
                id: "a".into(),
                name: "Ana".into(),
                address: "ana@gmail.com".into(),
                provider: "gmail".into(),
            }]);
            mailbox.overview = Some(Overview {
                messages: vec![message("a.inbox.1", 60_000, false)],
                ..Default::default()
            });
            mailbox.selected = Some("a.inbox.1".into());
            mailbox.reader = Some(Reader::new("a.inbox.1".into()));
            cx.notify();
        });
        cx.run_until_parked();
        cx.refresh().unwrap();
        let snooze = cx.debug_bounds("mail-snooze").expect("Snooze");
        cx.simulate_click(snooze.center(), Modifiers::default());
        cx.run_until_parked();
        cx.refresh().unwrap();
        let later = cx.debug_bounds("mail-snooze-later").expect("In 3 hours");
        cx.simulate_click(later.center(), Modifiers::default());
        cx.run_until_parked();
        mailbox.update(cx, |mailbox, cx| {
            assert!(mailbox.snoozed.contains_key("a.inbox.1"));
            assert!(mailbox.selected.is_none());
            assert!(mailbox.shown(cx).is_empty());
            mailbox.change_view(model::View::Snoozed, cx);
            assert_eq!(mailbox.shown(cx).len(), 1);
        });
        assert!(
            model::Service::new(&dir)
                .snoozed()
                .contains_key("a.inbox.1")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui::test]
    fn the_user_chooses_which_agents_read_a_connected_account(cx: &mut TestAppContext) {
        let (mailbox, cx) = mailbox(cx, "readers");
        let dir = mailbox.read_with(cx, |m, _| m.service.data_dir.clone());
        std::fs::create_dir_all(dir.join("mail")).unwrap();
        std::fs::write(
            dir.join("mail/accounts.json"),
            r#"{"accounts":[{"id":"a","name":"Ana","address":"ana@gmail.com","provider":"gmail","host":"imap.gmail.com","port":993}],"interesting":[]}"#,
        )
        .unwrap();
        mailbox.update(cx, |mailbox, cx| {
            mailbox.started = true;
            mailbox.accounts = Some(vec![AccountView {
                id: "a".into(),
                name: "Ana".into(),
                address: "ana@gmail.com".into(),
                provider: "gmail".into(),
            }]);
            mailbox.overview = Some(Overview::default());
            mailbox.agents = vec![connect::Agent {
                id: "fin".into(),
                label: "Finance".into(),
            }];
            mailbox.settings_open = true;
            cx.notify();
        });
        cx.run_until_parked();
        cx.refresh().unwrap();
        let toggle = cx.debug_bounds("mail-readers-a").expect("who reads it");
        cx.simulate_click(toggle.center(), Modifiers::default());
        cx.run_until_parked();
        cx.refresh().unwrap();
        let finance = cx
            .debug_bounds("mail-readers-pick-a-fin")
            .expect("the Finance pill");
        cx.simulate_click(finance.center(), Modifiers::default());
        cx.run_until_parked();
        mailbox.read_with(cx, |mailbox, _| {
            assert_eq!(
                model::readers_of(&mailbox.access, "ana@gmail.com").unwrap(),
                &["fin"]
            );
        });
        assert_eq!(
            model::readers_of(&model::Service::new(&dir).access(), "ana@gmail.com").unwrap(),
            &["fin"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui::test]
    fn folders_strip_scrolls_and_sidebar_resizes(cx: &mut TestAppContext) {
        let (mailbox, cx) = mailbox(cx, "scroll-resize");
        mailbox.update(cx, |mailbox, cx| {
            mailbox.started = true;
            mailbox.accounts = Some(vec![AccountView {
                id: "a".into(),
                name: "Ana".into(),
                address: "ana@gmail.com".into(),
                provider: "gmail".into(),
            }]);
            mailbox.overview = Some(Overview::default());
            cx.notify();
        });
        cx.run_until_parked();
        cx.refresh().unwrap();
        let right = cx
            .debug_bounds("mail-views-right")
            .expect("the right scroll arrow is visible when folders overflow");
        assert!(cx.debug_bounds("mail-views-left").is_none());
        cx.simulate_click(right.center(), Modifiers::default());
        cx.run_until_parked();
        cx.refresh().unwrap();
        assert!(
            cx.debug_bounds("mail-views-left").is_some(),
            "scrolling right reveals the left scroll arrow"
        );
        let splitter = cx
            .debug_bounds("mail-list-resize")
            .expect("the mail list resize splitter is visible");
        cx.simulate_mouse_down(splitter.center(), MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(
            point(splitter.center().x + px(80.), splitter.center().y),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_up(
            point(splitter.center().x + px(80.), splitter.center().y),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.run_until_parked();
        mailbox.read_with(cx, |mailbox, _| {
            assert_eq!(mailbox.list_width, 420.);
        });
    }
}
