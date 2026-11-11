//! Mono workspaces: one runtime per identity, shared by the rail, title tabs,
//! overview, settings and floating windows.
mod agent_calls;
pub(crate) mod agents;
pub(crate) mod avatar;
pub(crate) mod bot_import;
mod bot_settings;
pub(crate) mod bot_transfer;
pub(crate) mod cards;
mod changes;
mod color_picker;
mod dashboard;
mod files;
mod filters;
mod floating;
pub(crate) use floating::is_floating_window;
mod gallery;
pub(crate) mod group_call;
mod group_dialog;
mod groups;
mod host;
mod hover_cards;
pub(crate) mod html;
#[cfg(target_os = "windows")]
mod html_windows;
mod images;
mod mail_card;
mod mail_connect;
mod mail_workspace;
mod memory_ui;
mod mobile;
mod news;
mod news_view;
mod notifications;
mod overview;
mod panel_stack;
mod panels;
mod papers;
mod research;
mod scheduler;
mod search;
#[cfg(all(target_os = "macos", not(test)))]
mod status;
mod sync;
pub(crate) use sync::{SyncUpdated, SyncView};
#[cfg(test)]
mod tests;
mod work;
mod workstreams;
use crate::{
    conversation::{Conversation, MonoSubmitted, SessionChanged},
    input::{self, Input},
    popover::{Anchor, AnchorSlot, Menu, MenuEvent, menu_popover, popover},
    runtime::{Database, locked},
    shell::Shell,
    theme::*,
};
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, Focusable, MouseButton, Role, SharedString,
    Subscription, WeakEntity, Window, div, prelude::*, px, rgb,
};
use monocode_core::{
    agent::{AccessMode, Settings},
    mono::{
        self, Message, Mono, Roster,
        files::AgentFiles,
        habits::{self, Habit},
    },
    provider::Kind,
    store::Session,
};
pub(crate) use panels::SettingsSection;
use panels::{OpenPicker, OverviewTab, Route};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::Duration,
};

/// A Mono's document folders, cached by `(mono id, transcript rows, last row length)`.
type DocumentFolders = (
    (String, usize, usize),
    Vec<monocode_core::mono::folders::Folder>,
);

fn can_dispatch(message: &Message, busy: bool) -> bool {
    message.error.is_none()
        && (!busy
            || (!message.internal
                && !monocode_core::inbox::workspace::is_cloud(&message.id)
                && monocode_core::mono::agents::message_sender(&message.text).is_none()))
}

pub(crate) struct Changed;
pub(crate) struct HoverChanged;
pub(crate) struct TabOpened(pub String);
pub(crate) struct ShowDetails(pub String);
pub(crate) use notifications::{NotificationChanged, NotificationFinished, NotificationPosted};
#[derive(Clone, PartialEq)]
pub(crate) struct OverviewChrome {
    active: Option<String>,
    tab: OverviewTab,
    routed: bool,
    counts: (usize, usize, usize, usize),
}
struct RunningHabit {
    mono: String,
    habit: String,
    view: Entity<Conversation>,
    started: u64,
    launched: u64,
    failure: Option<String>,
    waiting_since: Option<u64>,
    cards: Vec<serde_json::Value>,
    mailbox: Vec<String>,
}
pub(crate) struct Monos {
    #[cfg(all(target_os = "macos", not(test)))]
    status: Option<status::Status>,
    /// The live items beside the menu bar icon (the tray on Windows).
    #[cfg(all(any(target_os = "macos", target_os = "windows"), not(test)))]
    live: Option<Entity<crate::menu_bar::LiveBar>>,
    /// Draws the live item into the icon as it changes.
    #[cfg(all(target_os = "macos", not(test)))]
    live_watch: Option<Subscription>,
    /// Shows monoBOT's menu when the tray icon is clicked.
    #[cfg(all(target_os = "windows", not(test)))]
    tray_menu: Option<Subscription>,
    parent_window: Option<gpui::AnyWindowHandle>,
    owner: WeakEntity<Shell>,
    store: Database,
    root: PathBuf,
    prefs: crate::settings::Preferences,
    pub(crate) roster: Roster,
    pub(crate) active: Option<String>,
    chats: HashMap<String, Entity<Conversation>>,
    bridge: Option<mono::bridge::Server>,
    observations: Vec<Subscription>,
    _bridge_task: Option<gpui::Task<()>>,
    _clock: Option<gpui::Task<()>>,
    writes: Option<gpui::Task<()>>,
    loading_chats: HashSet<String>,
    work: work::Cache,
    pub(crate) error: Option<String>,
    notice_region: AnchorSlot,
    pub(crate) details: Entity<overview::Overview>,
    original_chrome: bool,
    overview_tab: OverviewTab,
    /// A group's chat drawn as a board, made the first time it shows.
    group_board: Option<groups::GroupBoard>,
    overview_previous: OverviewTab,
    /// When the Overview tab last appeared for a Mono, so its graph draws in.
    dashboard_shown: Option<(String, std::time::Instant)>,
    /// Each Mono's contributions as last loaded, and those loading now.
    contributions: HashMap<String, dashboard::Loaded>,
    contributions_loading: HashSet<String>,
    overview_reveal: Option<crate::project_tabs::TabReveal<OverviewTab>>,
    route: Option<Route>,
    settings: Option<SettingsSection>,
    settings_focus: gpui::FocusHandle,
    fields: HashMap<String, Entity<Input>>,
    memory_edit: Option<String>,
    memory_adding: bool,
    /// The group's Members tab is listing who can still join.
    members_adding: bool,
    memory_pending: HashSet<String>,
    field_blurs: HashSet<gpui::EntityId>,
    file_versions: HashMap<String, (String, String)>,
    saving_files: HashSet<String>,
    file_load_revision: u64,
    files: Option<AgentFiles>,
    habits: Vec<Habit>,
    habit_ui: scheduler::HabitUi,
    picker: Option<OpenPicker>,
    settings_anchor: AnchorSlot,
    model_anchor: AnchorSlot,
    subsession_model_anchor: AnchorSlot,
    subsession_effort_anchor: AnchorSlot,
    projects_anchor: AnchorSlot,
    access_anchor: AnchorSlot,
    menu: Option<(String, Entity<Menu>, Vec<Subscription>)>,
    menu_anchor: Anchor,
    documents: HashMap<String, Entity<crate::transcript::TranscriptView>>,
    html_documents: HashMap<String, Entity<html::Html>>,
    /// Native visuals the Mono published in its chat, opened as documents.
    visual_documents: HashMap<String, Entity<crate::visuals::VisualView>>,
    papers: HashMap<String, papers::PaperPage>,
    /// Cited papers known only by their record, by the citation's link.
    paper_records: HashMap<String, papers::PaperRecord>,
    loading_papers: HashSet<String>,
    /// The Documents tab's folders, read from the chat; kept while the chat's
    /// rows stay as they were (mono id, row count, last row's length).
    document_folders: std::cell::RefCell<Option<DocumentFolders>>,
    /// Folders the user closed in the Documents tab.
    closed_folders: HashSet<String>,
    project_changes: changes::ProjectChanges,
    review: Option<Entity<crate::review::ReviewPane>>,
    activity: Option<(
        String,
        Entity<crate::transcript::TranscriptView>,
        monocode_core::research::TurnResearch,
    )>,
    activity_turn: Option<usize>,
    activity_sources_all: bool,
    rail_ids: Vec<String>,
    /// Messages between agents, waits and delegations.
    agent_calls: agent_calls::AgentCalls,
    /// The bots' News feed and the cards waiting to be shown.
    news: news::NewsState,
    /// What a bot's settings loaded or last failed at.
    bot_settings: bot_settings::BotSettingsState,
    /// Bots kept the same on the user's other computers.
    sync: sync::SyncState,
    /// Groups' seats, rounds and members' posts.
    groups: groups::Groups,
    /// The open group's local voice call, if its Call tab was opened.
    call: Option<group_call::CallState>,
    /// Where the new-bot menu opened, while it is open.
    roles_menu: Option<gpui::Point<gpui::Pixels>>,
    /// The open bot's subagent runs, read from its chat until it changes.
    subagent_runs: Option<(agents::RunsKey, Vec<agents::SubagentRun>)>,
    /// The image bot's gallery, read from its chat, by the chat's size.
    gallery_items: Option<(agents::RunsKey, Vec<monocode_core::images::gallery::Item>)>,
    gallery_filter: monocode_core::images::gallery::Filter,
    running: HashMap<i64, RunningHabit>,
    active_run: Option<i64>,
    completion_work: HashSet<(String, u64)>,
    /// The open background dialog, centred in the window it opened in.
    background: Option<(
        Entity<crate::project_settings::BackgroundDialog>,
        Subscription,
        gpui::Point<gpui::Pixels>,
    )>,
    /// The open group editor, and the window it was opened in.
    group_dialog: Option<(
        Entity<group_dialog::GroupDialog>,
        Subscription,
        gpui::Size<gpui::Pixels>,
    )>,
    /// Mail cards whose Send or Save as draft is on its way.
    mail_sending: HashSet<String>,
    /// The dialog a bot opened with `mail.connect`: the bot, the form, and the window size.
    mail_connect: Option<(
        String,
        Entity<crate::inbox::mail::connect::ConnectForm>,
        Subscription,
        gpui::Size<gpui::Pixels>,
    )>,
    /// The Workstreams tour and dialog, and the PRs and Files tabs' lists.
    workstreams: workstreams::State,
    backgrounds: HashMap<String, Entity<crate::project_background::ProjectBackground>>,
    drag: Option<(String, gpui::Point<gpui::Pixels>, Option<String>)>,
    rail_bounds: HashMap<String, AnchorSlot>,
    /// The agent card beside the rail row under the pointer.
    mono_preview: hover_cards::Hover,
    skip_click: bool,
    floating: Vec<gpui::AnyWindowHandle>,
    scheduling: bool,
    persisted_messages: HashSet<String>,
    failed_reports: HashSet<String>,
    inflight: HashMap<String, Message>,
    card_refresh: HashMap<String, u64>,
    session_filters: filters::Filters,
    session_filter_anchor: AnchorSlot,
    app_results: HashMap<(i64, String), (String, Result<serde_json::Value, String>)>,
    app_pending: HashSet<(i64, String)>,
    mail_polling: bool,
    mail_last_poll: u64,
    last_tick: Option<u64>,
}
impl EventEmitter<Changed> for Monos {}
impl EventEmitter<HoverChanged> for Monos {}
impl EventEmitter<TabOpened> for Monos {}
impl EventEmitter<ShowDetails> for Monos {}
impl EventEmitter<SyncUpdated> for Monos {}
impl Monos {
    /// What its chats and habit runs are doing, for keeping the computer awake.
    pub(crate) fn keep_awake_work(&self, cx: &App) -> Vec<monocode_core::keep_awake::Work> {
        self.chats
            .values()
            .chain(self.running.values().map(|r| &r.view))
            .map(|view| {
                let view = view.read(cx);
                monocode_core::keep_awake::Work {
                    cwd: view.summary.cwd.clone(),
                    busy: view.busy,
                    retry_scheduled: view.retry_scheduled(),
                    resume_at_reset: view
                        .settings
                        .usage_limit
                        .as_ref()
                        .is_some_and(|limit| limit.resume_at_reset),
                    scheduled_message: view.has_scheduled_message(),
                }
            })
            .collect()
    }
    pub(crate) fn resource_sessions(
        &self,
        cx: &App,
    ) -> Vec<(i64, crate::resource_pressure::SessionInfo)> {
        self.chats
            .values()
            .chain(self.running.values().map(|r| &r.view))
            .map(|v| {
                let s = &v.read(cx).summary;
                (
                    s.id,
                    crate::resource_pressure::SessionInfo {
                        title: s.title.clone().into(),
                        provider: s.provider.clone(),
                    },
                )
            })
            .collect()
    }
    pub(crate) fn stop_session(&mut self, id: i64, cx: &mut Context<Self>) {
        if let Some(view) = self
            .chats
            .values()
            .chain(self.running.values().map(|r| &r.view))
            .find(|v| v.read(cx).summary.id == id)
        {
            view.update(cx, |v, cx| v.stop_if_busy(cx));
        }
    }
    pub(crate) fn bind_window(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.parent_window = Some(window.window_handle());
        self.sync_status(cx);
    }
    fn sync_status(&mut self, _cx: &mut Context<Self>) {
        #[cfg(all(target_os = "macos", not(test)))]
        {
            if !self.prefs.mono_menu_bar_icon {
                self.live_watch = None;
                self.live = None;
                self.status = None;
                return;
            }
            if self.status.is_none() {
                self.status = status::Status::new();
            }
            let entity = _cx.weak_entity();
            let app = std::cell::RefCell::new(_cx.to_async());
            let run = std::rc::Rc::new(move |id: &str| {
                let _ = entity.update(&mut *app.borrow_mut(), |this, cx| this.run_status(id, cx));
            });
            // The menu opens the floating chat, which moves between the
            // personal agents only.
            let monos = self.menu_monos();
            if let Some(status) = &mut self.status {
                status.update(&monos, &self.prefs.menu_bar_live, run);
            }
        }
        // What sits beside the icon (macOS) or in the tray (Windows).
        #[cfg(all(any(target_os = "macos", target_os = "windows"), not(test)))]
        {
            let prefs = self.prefs.clone();
            match &self.live {
                Some(live) => live.update(_cx, |live, cx| live.set_preferences(&prefs, cx)),
                None => {
                    let live = crate::menu_bar::open(&prefs, _cx);
                    #[cfg(target_os = "macos")]
                    {
                        self.live_watch =
                            Some(_cx.observe(&live, |this, _, cx| this.show_live(cx)));
                    }
                    #[cfg(target_os = "windows")]
                    {
                        self.tray_menu =
                            Some(_cx.subscribe(
                                &live,
                                |this, _, _: &crate::menu_bar::TrayClicked, cx| {
                                    this.show_tray_menu(cx)
                                },
                            ));
                    }
                    self.live = Some(live);
                }
            }
            #[cfg(target_os = "macos")]
            self.show_live(_cx);
        }
    }
    /// The Monos the icon's menu lists: the floating chat moves between the
    /// personal agents only.
    #[cfg(all(any(target_os = "macos", target_os = "windows"), not(test)))]
    fn menu_monos(&self) -> Vec<(String, String)> {
        floating::personal_agents(&self.roster.monos)
            .iter()
            .map(|m| (m.id.clone(), m.label()))
            .collect()
    }
    /// What the icon's menu does with the line chosen, on macOS and Windows.
    #[cfg(all(any(target_os = "macos", target_os = "windows"), not(test)))]
    fn run_status(&mut self, id: &str, cx: &mut Context<Self>) {
        if id == "quit" {
            // The regular quit: it saves, stops the agents and lets a
            // downloaded update install.
            crate::quit(cx);
        } else if id == "notch" {
            #[cfg(target_os = "windows")]
            if let Some(live) = self.live.clone()
                && live.update(cx, |live, cx| live.open_notch(cx))
            {
                return;
            }
            self.run_status("open", cx);
        } else if id == "open" {
            cx.activate(true);
            if let Some(handle) = self.parent_window {
                let _ = handle.update(cx, |_, window, _| window.activate_window());
            }
        } else if id == "paste-last" {
            // The newest dictation, pasted where the caret is (or copied
            // when the menu left nothing in front to take it).
            if let Some(system) = crate::dictation::system::get(cx) {
                system.update(cx, |system, cx| system.paste_last(cx));
            }
        } else if id == "composer" {
            // The composer is its own panel: the workspace window stays
            // where it is, as in the Tauri menu.
            if let Some(handle) = self.parent_window {
                let owner = self.owner.clone();
                let app: &mut App = cx;
                app.defer(move |cx| {
                    let _ = handle.update(cx, |_, window, cx| {
                        let _ = owner.update(cx, |o, cx| o.mono_quick_compose(window, cx));
                    });
                });
            }
        } else if let Some(choice) = id.strip_prefix("live:") {
            // Saved like any preference, after this update ends: the shell
            // hands the preferences back to the monos.
            let owner = self.owner.clone();
            let choice = choice.to_owned();
            let app: &mut App = cx;
            app.defer(move |cx| {
                let _ = owner.update(cx, |o, cx| o.set_menu_bar_live(choice, cx));
            });
        } else if self.roster.get(id).is_some() {
            // Only the floating chat opens; the workspace keeps whichever
            // Mono or session it was showing.
            self.open_floating(id.into(), cx);
        }
    }
    /// The tray's menu. Windows runs its own loop while the menu is open,
    /// so it is shown from a task, outside any entity update.
    #[cfg(all(target_os = "windows", not(test)))]
    fn show_tray_menu(&mut self, cx: &mut Context<Self>) {
        let entries =
            crate::menu_bar::status_menu(&self.menu_monos(), &self.prefs.menu_bar_live, true);
        cx.spawn(async move |this, cx| {
            if let Some(id) = crate::menu_bar::popup(&entries) {
                let _ = this.update(cx, |this, cx| this.run_status(&id, cx));
            }
        })
        .detach();
    }
    /// Draws the live item into the menu bar icon.
    #[cfg(all(target_os = "macos", not(test)))]
    fn show_live(&mut self, cx: &mut Context<Self>) {
        let item = self
            .live
            .as_ref()
            .and_then(|live| live.read(cx).shown.clone());
        if let Some(status) = &mut self.status {
            status.set_live(item.as_ref());
        }
    }
    pub(crate) fn hide_html(&self, cx: &App) {
        for view in self.html_documents.values() {
            view.read(cx).hide();
        }
    }
    pub(crate) fn sidebar_preferences(
        &mut self,
        id: &str,
        show: bool,
        folders: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(m) = self.roster.get_mut(id) {
            m.show_started_sessions_in_sidebar = show;
            m.use_sidebar_folders = folders;
        }
        let sessions = self
            .roster
            .get(id)
            .map(|m| m.launches.iter().map(|l| l.session_id).collect::<Vec<_>>())
            .unwrap_or_default();
        for session in sessions {
            self.group_launch(id, session, cx);
        }
        self.configure_chat(id, cx);
        self.save(cx);
    }
    pub(crate) fn new(
        store: Database,
        owner: WeakEntity<Shell>,
        prefs: crate::settings::Preferences,
        active: Option<String>,
        notice_region: AnchorSlot,
        cx: &mut Context<Self>,
    ) -> Self {
        let (roster, root, roster_error) = match locked(&store) {
            Ok(db) => match Roster::load(&db) {
                Ok(roster) => (roster, mono::root(&db), None),
                Err(error) => (Roster::default(), mono::root(&db), Some(error)),
            },
            Err(error) => (
                Roster::default(),
                std::env::temp_dir().join(mono::new_id()),
                Some(error),
            ),
        };
        let (bridge, rx) = if cx.is_test() {
            (None, None)
        } else {
            match mono::bridge::Server::start() {
                Ok((server, rx)) => (Some(server), Some(rx)),
                Err(_) => (None, None),
            }
        };
        let persisted_messages = roster
            .monos
            .iter()
            .flat_map(|m| m.outbox.iter().map(|q| q.id.clone()))
            .collect();
        let session_filters = locked(&store)
            .ok()
            .and_then(|s| s.setting("mono-session-filters").ok().flatten())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let manager = cx.entity();
        let details = cx.new(|cx| overview::Overview::new(manager, cx));
        let mut this = Self {
            #[cfg(all(target_os = "macos", not(test)))]
            status: None,
            #[cfg(all(any(target_os = "macos", target_os = "windows"), not(test)))]
            live: None,
            #[cfg(all(target_os = "macos", not(test)))]
            live_watch: None,
            #[cfg(all(target_os = "windows", not(test)))]
            tray_menu: None,
            parent_window: None,
            owner,
            store,
            root,
            prefs,
            roster,
            active: None,
            chats: HashMap::new(),
            bridge,
            observations: Vec::new(),
            _bridge_task: None,
            _clock: None,
            writes: None,
            loading_chats: HashSet::new(),
            work: work::Cache::default(),
            error: roster_error,
            notice_region,
            details,
            original_chrome: false,
            overview_tab: OverviewTab::Overview,
            group_board: None,
            overview_previous: OverviewTab::Overview,
            dashboard_shown: None,
            contributions: HashMap::new(),
            contributions_loading: HashSet::new(),
            overview_reveal: None,
            route: None,
            settings: None,
            settings_focus: cx.focus_handle(),
            fields: HashMap::new(),
            memory_edit: None,
            memory_adding: false,
            members_adding: false,
            memory_pending: HashSet::new(),
            field_blurs: HashSet::new(),
            file_versions: HashMap::new(),
            saving_files: HashSet::new(),
            file_load_revision: 0,
            files: None,
            habits: Vec::new(),
            habit_ui: scheduler::HabitUi::default(),
            picker: None,
            settings_anchor: AnchorSlot::default(),
            model_anchor: AnchorSlot::default(),
            subsession_model_anchor: AnchorSlot::default(),
            subsession_effort_anchor: AnchorSlot::default(),
            projects_anchor: AnchorSlot::default(),
            access_anchor: AnchorSlot::default(),
            menu: None,
            menu_anchor: Anchor::Point(gpui::Point::default()),
            documents: HashMap::new(),
            html_documents: HashMap::new(),
            visual_documents: HashMap::new(),
            papers: HashMap::new(),
            paper_records: HashMap::new(),
            loading_papers: HashSet::new(),
            document_folders: Default::default(),
            closed_folders: HashSet::new(),
            project_changes: Default::default(),
            review: None,
            activity: None,
            activity_turn: None,
            activity_sources_all: false,
            rail_ids: Vec::new(),
            roles_menu: None,
            subagent_runs: None,
            gallery_items: None,
            gallery_filter: Default::default(),
            agent_calls: Default::default(),
            news: Default::default(),
            bot_settings: Default::default(),
            sync: Default::default(),
            groups: Default::default(),
            call: None,
            running: HashMap::new(),
            active_run: None,
            completion_work: HashSet::new(),
            background: None,
            group_dialog: None,
            mail_connect: None,
            mail_sending: HashSet::new(),
            workstreams: Default::default(),
            backgrounds: HashMap::new(),
            drag: None,
            rail_bounds: HashMap::new(),
            mono_preview: Default::default(),
            skip_click: false,
            floating: Vec::new(),
            scheduling: false,
            persisted_messages,
            failed_reports: HashSet::new(),
            inflight: HashMap::new(),
            card_refresh: HashMap::new(),
            session_filters,
            session_filter_anchor: AnchorSlot::default(),
            app_results: HashMap::new(),
            app_pending: HashSet::new(),
            mail_polling: false,
            mail_last_poll: 0,
            last_tick: None,
        };
        this.load_sync_view();
        if let Some(rx) = rx {
            this._bridge_task = Some(cx.spawn(async move |this, cx| {
                while let Ok(call) = rx.recv().await {
                    if this
                        .update_in(cx, |this, window, cx| this.app_call(call, window, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        }
        this._clock = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(10))
                    .await;
                if this
                    .update_in(cx, |this, window, cx| this.tick(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
        // The Monos switch hides the coding Monos; bots and groups stay open.
        if let Some(id) = active.filter(|id| {
            this.roster
                .get(id)
                .is_some_and(|m| this.prefs.monos || m.is_personal())
        }) {
            this.active = Some(id.clone());
            this.ensure_chat(&id, cx);
            this.reload_files(&id, cx);
        }
        this
    }
    pub(crate) fn is_mono_session(&self, id: i64) -> bool {
        self.roster.hidden_sessions.contains(&id)
            || self.roster.of_session(id).is_some()
            || self
                .roster
                .monos
                .iter()
                .any(|m| m.habit_sessions.contains(&id))
    }
    pub(crate) fn hidden_session(&self, id: i64) -> bool {
        self.is_mono_session(id)
            || self.roster.monos.iter().any(|m| {
                !m.show_started_sessions_in_sidebar && m.launches.iter().any(|l| l.session_id == id)
            })
    }
    pub(crate) fn set_preferences(
        &mut self,
        prefs: crate::settings::Preferences,
        cx: &mut Context<Self>,
    ) {
        let was = self.prefs.monos;
        self.prefs = prefs;
        if was && !self.prefs.monos {
            self.active = None;
        }
        self.sync_status(cx);
        cx.notify();
    }
    pub(crate) fn set_original_chrome(&mut self, original: bool, cx: &mut Context<Self>) {
        if self.original_chrome == original {
            return;
        }
        self.original_chrome = original;
        self.details
            .update(cx, |view, cx| view.set_original_chrome(original, cx));
        cx.notify();
    }
    /// The session the details panel shows as a page, if any.
    pub(crate) fn routed_session(&self) -> Option<i64> {
        match self.route {
            Some(Route::Session(id)) => Some(id),
            _ => None,
        }
    }
    pub(crate) fn overview_chrome(&self) -> OverviewChrome {
        let counts = self.current().map_or((0, 0, 0, 0), |mono| {
            (
                mono.launches.len(),
                mono.documents.len(),
                self.habits.len(),
                self.project_changes.count(&mono.id),
            )
        });
        OverviewChrome {
            active: self.active.clone(),
            tab: self.overview_tab,
            routed: self.route.is_some(),
            counts,
        }
    }
    fn show_overview(&self, cx: &mut Context<Self>) {
        if let Some(id) = self.active.clone() {
            cx.emit(ShowDetails(id));
        }
    }
    /// TitleBar.tsx's MonoTitle, used by the original full-width Mono view.
    pub(crate) fn original_title(&self, inset: f32, cx: &App) -> AnyElement {
        let Some(mono) = self.current() else {
            return div().flex_1().min_w_0().into_any_element();
        };
        let state = self
            .chats
            .get(&mono.id)
            .map(|chat| chat.read(cx).mono_status())
            .unwrap_or("idle");
        let status = match state {
            "working" => "Working",
            "needs-you" => "Needs you",
            _ => "Idle",
        };
        div()
            .id("original-mono-title")
            .debug_selector(|| "original-mono-title".into())
            .flex_1()
            .min_w_0()
            .h_full()
            .pl(px(inset))
            .pr_4()
            .flex()
            .items_center()
            .gap_2()
            .child(if mono.is_group() {
                let (shown, more) = self.group_stack_members(mono);
                avatar::group_avatar_stack(&shown, state, more, 18., false)
            } else {
                avatar::avatar(&mono.face(), mono.color, state, 16., state == "idle")
            })
            .child(
                div()
                    .debug_selector(|| "original-mono-name".into())
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(content(1.))
                    .child(mono.label()),
            )
            .child(
                div()
                    .debug_selector(|| "original-mono-status".into())
                    .flex_shrink_0()
                    .text_size(px(12.))
                    .font_weight(gpui::FontWeight::NORMAL)
                    .text_color(if state == "needs-you" {
                        accent_color()
                    } else {
                        content(0.45)
                    })
                    .child(status),
            )
            .into_any_element()
    }
    pub(crate) fn open_current_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_current_settings_section(SettingsSection::General, window, cx);
    }
    pub(crate) fn open_current_settings_section(
        &mut self,
        section: SettingsSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.active.clone() else {
            return;
        };
        self.settings = Some(section);
        window.focus(&self.settings_focus, cx);
        self.reload_files(&id, cx);
        cx.notify();
    }
    pub(crate) fn toggle_bot_intro(&mut self, cx: &mut Context<Self>) -> bool {
        self.roster.bot_intro_dismissed = !self.roster.bot_intro_dismissed;
        self.save(cx);
        cx.notify();
        !self.roster.bot_intro_dismissed
    }
    pub(crate) fn current(&self) -> Option<&Mono> {
        self.active.as_deref().and_then(|id| self.roster.get(id))
    }
    pub(crate) fn chat(&self) -> Option<Entity<Conversation>> {
        self.active
            .as_ref()
            .and_then(|id| self.chats.get(id))
            .cloned()
    }
    /// A Mono's chat when it is open, without opening it.
    pub(crate) fn open_chat(&self, id: &str) -> Option<Entity<Conversation>> {
        self.chats.get(id).cloned()
    }
    /// A Mono's chat for a paired phone, opening it in the background when
    /// it is not open yet.
    pub(crate) fn mobile_chat(
        &mut self,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Option<Entity<Conversation>> {
        self.pane_chat(id, cx)
    }
    /// A Mono's chat for a split pane, loading it when it is not open yet.
    pub(crate) fn pane_chat(
        &mut self,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Option<Entity<Conversation>> {
        self.ensure_chat(id, cx);
        self.chats.get(id).cloned()
    }
    /// A phone's message to a Mono, queued as one typed in its chat.
    pub(crate) fn mobile_send(
        &mut self,
        id: &str,
        text: String,
        attachments: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.roster.get(id).is_none() {
            return Err("Mono is no longer available".into());
        }
        self.ensure_chat(id, cx);
        self.enqueue(id, text, attachments, false, cx);
        Ok(())
    }
    pub(crate) fn account_changed(
        &mut self,
        kind: Kind,
        id: &str,
        removed: bool,
        cx: &mut Context<Self>,
    ) {
        for view in self.chats.values() {
            if view.read(cx).summary.provider == kind.id()
                && view.read(cx).settings.account.id == id
            {
                view.update(cx, |view, cx| {
                    if removed {
                        view.account_unavailable(cx)
                    } else {
                        view.account_authenticated(cx)
                    }
                });
            }
        }
    }
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.sync_status(cx);
        cx.emit(Changed);
        cx.notify();
    }
    /// Chief's own mono, made the first time a chat hands work to Chief,
    /// and the user's code monos it may give tasks to.
    pub(crate) fn chief_crew(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<(
        String,
        Vec<monocode_core::orchestration::control::CrewMember>,
    )> {
        let (chief, made) = mono::crew::ensure_chief(&mut self.roster, &self.root)?;
        if made {
            self.save(cx);
            self.changed(cx);
        }
        Some((chief, mono::crew::members(&self.roster)))
    }

    /// The code mono that does a task: the one Chief named, else a free one
    /// that fits, else, when `may_create`, a new one that stays with the
    /// user. Its id, and whether it was just made.
    pub(crate) fn crew_for_task(
        &mut self,
        asked: Option<&str>,
        handle: Option<&str>,
        title: &str,
        busy: &dyn Fn(&str) -> bool,
        may_create: bool,
        cx: &mut Context<Self>,
    ) -> Option<(String, bool)> {
        if let Some(id) = asked.filter(|id| self.roster.monos.iter().any(|m| m.id == *id)) {
            return Some((id.to_owned(), false));
        }
        if let Some(id) = mono::crew::pick(&self.roster, handle, title, busy) {
            return Some((id, false));
        }
        if !may_create {
            return None;
        }
        let id = mono::crew::create(&mut self.roster, &self.root, handle, title)?;
        self.save(cx);
        self.changed(cx);
        Some((id, true))
    }

    /// A crew mono and the folder its files live in, to read its identity
    /// off the UI thread.
    pub(crate) fn crew_member(&self, id: &str) -> Option<(PathBuf, Mono)> {
        let mono = self.roster.monos.iter().find(|m| m.id == id)?.clone();
        Some((self.root.clone(), mono))
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        self.sync_soon();
        let roster = self.roster.clone();
        let cloud_ids: Vec<String> = roster
            .monos
            .iter()
            .flat_map(|m| m.outbox.iter())
            .filter(|q| monocode_core::inbox::workspace::is_cloud(&q.id))
            .map(|q| q.id.clone())
            .collect();
        let store = self.store.clone();
        let queued = roster
            .monos
            .iter()
            .map(|m| {
                (
                    m.id.clone(),
                    m.outbox.iter().map(|q| q.id.clone()).collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        let previous = self.writes.take();
        self.writes = Some(cx.spawn(async move |this, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            let work = cx.background_executor().spawn(async move {
                let db = locked(&store)?;
                roster.save(&db)?;
                let cloud_error = if !cloud_ids.is_empty() {
                    let dir = db
                        .data_dir()
                        .map(PathBuf::from)
                        .unwrap_or_else(monocode_core::inbox::mail::desktop_data_dir);
                    monocode_core::inbox::workspace::Service::new(dir)
                        .ack_deliveries(&cloud_ids)
                        .err()
                } else {
                    None
                };
                Ok::<_, String>(cloud_error)
            });
            let result = work.await;
            let _ = this.update(cx, |this, cx| match result {
                Err(error) => {
                    this.error = Some(error);
                    cx.notify();
                }
                Ok(cloud_error) => {
                    if let Some(error) = &cloud_error {
                        this.error = Some(error.clone());
                        cx.notify();
                    }
                    for (id, messages) in queued {
                        this.persisted_messages
                            .extend(messages.into_iter().filter(|message| {
                                cloud_error.is_none()
                                    || !monocode_core::inbox::workspace::is_cloud(message)
                            }));
                        this.dispatch(&id, cx);
                    }
                }
            });
        }));
        self.changed(cx);
    }
    fn ensure_chat(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.chats.contains_key(id) || !self.loading_chats.insert(id.into()) {
            return;
        }
        let Some(mono) = self.roster.get(id).cloned() else {
            return;
        };
        let store = self.store.clone();
        let prefs = self.prefs.clone();
        let id = id.to_string();
        let work = cx.background_executor().spawn(async move {
            let db = locked(&store)?;
            if let Some(missing) = &mono.unavailable_session {
                return Err(format!("This Mono's saved conversation ({missing}) is unavailable. Its identity and history have been preserved."));
            }
            if let Some(session) = mono.session_id {
                let summary = db.mono_session(session)?
                    .ok_or("This Mono's saved conversation is unavailable. Reset its conversation explicitly to start a new one.")?;
                let delivered = mono
                    .outbox
                    .iter()
                    .filter(|q| {
                        db.setting(&format!("mono-delivered:{}:{}", summary.id, q.id))
                            .ok()
                            .flatten()
                            .is_some()
                    })
                    .map(|q| q.id.clone())
                    .collect::<Vec<_>>();
                return Ok::<_, String>((summary, None, delivered));
            }
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".into());
            let provider = prefs
                .default_provider
                .as_deref()
                .and_then(Kind::parse)
                .unwrap_or(Kind::Codex);
            // A workstream's orchestrator starts in its own checkout.
            let cwd = mono::workstreams::chat_cwd(mono.workstream.as_ref(), &home);
            let mut summary = db.create(provider.id(), cwd)?;
            summary.title = mono.label();
            db.title(summary.id, &summary.title)?;
            // A Mono's chat is conversation: it answers quickly at low effort
            // and hands heavy work to background sessions. Only new chats get
            // this; existing Monos keep whatever effort they already have.
            let settings = Settings {
                access: AccessMode::Auto,
                model: prefs.default_models.get(provider.id()).cloned(),
                effort: Some("low".into()),
                ..Default::default()
            };
            db.configure_agent(summary.id, provider.id(), &settings)?;
            Ok((summary, Some(settings), Vec::new()))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.loading_chats.remove(&id);
                let (summary, settings, delivered) = match result {
                    Ok(value) => value,
                    Err(e) => {
                        this.error = Some(e);
                        this.changed(cx);
                        return;
                    }
                };
                if !this.roster.hidden_sessions.contains(&summary.id) {
                    this.roster.hidden_sessions.push(summary.id);
                }
                let Some(mono) = this.roster.get_mut(&id) else {
                    return;
                };
                mono.session_id = Some(summary.id);
                mono.outbox.retain(|q| !delivered.contains(&q.id));
                let mono = mono.clone();
                let view = cx.new(|cx| Conversation::new(summary, this.store.clone(), cx));
                let root = this.root.clone();
                let env = this
                    .bridge
                    .as_ref()
                    .map(|b| b.environment(view.read(cx).summary.id))
                    .unwrap_or_default();
                let groups = this.group_context(&mono, None);
                let speaker = this.mono_speaker(&mono);
                let people = this.group_mention_people(&mono);
                view.update(cx, |view, cx| {
                    view.configure_mono(mono, root, env, Some(speaker), cx);
                    if let Some(settings) = settings {
                        let config = view.settings.mono.take();
                        let env = std::mem::take(&mut view.settings.environment);
                        view.install_transfer(settings, None, cx);
                        view.settings.mono = config;
                        view.settings.environment = env;
                    }
                    view.set_mono_groups(groups, cx);
                    view.set_mention_people(people, cx);
                    view.load(cx);
                });
                let source = id.clone();
                this.observations.push(cx.subscribe(
                    &view,
                    move |this, _, event: &crate::conversation::GroupAccessAnswer, cx| {
                        this.answer_access(&source, &event.request, event.access, cx)
                    },
                ));
                this.observations.push(cx.subscribe(
                    &view,
                    |this, _, _: &crate::conversation::MonoPresetsChanged, cx| {
                        if this.overview_tab == OverviewTab::Styles {
                            cx.notify();
                        }
                    },
                ));
                let source = id.clone();
                this.observations.push(cx.subscribe(
                    &view,
                    move |this, _, event: &MonoSubmitted, cx| {
                        // In a group's chat, @Name and rounds go to the members.
                        if this.group_submitted(&source, &event.text, &event.attachments, cx) {
                            return;
                        }
                        let _ = this.enqueue(
                            &source,
                            event.text.clone(),
                            event
                                .attachments
                                .iter()
                                .map(|p| p.to_string_lossy().to_string())
                                .collect(),
                            false,
                            cx,
                        );
                    },
                ));
                let source = id.clone();
                this.observations.push(cx.subscribe(
                    &view,
                    move |this, _, _: &SessionChanged, cx| {
                        this.chat_changed(&source, cx);
                    },
                ));
                let source = id.clone();
                this.observations.push(cx.subscribe(
                    &view,
                    move |this, _, _: &crate::conversation::UsageResumed, cx| {
                        let empty = this.roster.get(&source).is_none_or(|m| m.outbox.is_empty());
                        if empty {
                            this.enqueue(
                                &source,
                                "Continue from where you left off.".into(),
                                Vec::new(),
                                false,
                                cx,
                            );
                        } else {
                            if let Some(mono) = this.roster.get_mut(&source) {
                                for message in &mut mono.outbox {
                                    message.error = None;
                                }
                            }
                            this.save(cx);
                            this.configure_chat(&source, cx);
                        }
                    },
                ));
                let source = id.clone();
                this.observations
                    .push(cx.observe(&view, move |this, view, cx| {
                        if this.active.as_ref() == Some(&source) {
                            cx.notify();
                        }
                        // A streaming turn redraws its chat on each update;
                        // what was said is news for the notch too.
                        if view.read(cx).busy {
                            cx.emit(NotificationChanged::Content);
                        }
                        this.dispatch(&source, cx);
                    }));
                let source = id.clone();
                this.observations.push(cx.subscribe(
                    &view,
                    move |this, _, event: &crate::conversation::MonoQueueChanged, cx| {
                        this.queue_changed(&source, &event.id, event.remove, cx)
                    },
                ));
                let transcript = view.read(cx).transcript.clone();
                let target = view.clone();
                let source = id.clone();
                this.observations.push(cx.subscribe(
                    &transcript,
                    move |this, _, event: &crate::transcript::MonoResponseAction, cx| match event {
                        crate::transcript::MonoResponseAction::Queue { id, remove } => {
                            this.queue_changed(&source, id, *remove, cx)
                        }
                        crate::transcript::MonoResponseAction::Activity(turn) => {
                            this.activity_turn = Some(*turn);
                            this.show_overview(cx);
                            this.route = None;
                            this.overview_tab = OverviewTab::Activity;
                            cx.notify();
                        }
                        crate::transcript::MonoResponseAction::Quote { text, window } => {
                            let target = target.clone();
                            let text = text.clone();
                            let handle = *window;
                            let app: &mut App = cx;
                            app.defer(move |cx| {
                                let _ = handle.update(cx, |_, window, cx| {
                                    target.update(cx, |v, cx| v.mono_quote(&text, window, cx))
                                });
                            });
                        }
                    },
                ));
                let source = id.clone();
                this.observations.push(cx.subscribe(
                    &transcript,
                    move |this, _, event: &cards::CardAction, cx| {
                        this.card_action(&source, &event.0, cx)
                    },
                ));
                this.chats.insert(id.clone(), view);
                if let Some(handle) = this.parent_window {
                    let owner = cx.weak_entity();
                    let id = id.clone();
                    cx.defer(move |cx| {
                        let _ = handle.update(cx, |_, window, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                if this.active.as_deref() == Some(&id)
                                    && this.settings_focus.is_focused(window)
                                {
                                    this.focus_active(window, cx);
                                }
                            });
                        });
                    });
                }
                this.save(cx);
                this.dispatch(&id, cx);
            });
        })
        .detach();
    }
    /// Retry clears a queued message's error; discard drops it.
    fn queue_changed(&mut self, source: &str, id: &str, remove: bool, cx: &mut Context<Self>) {
        if let Some(mono) = self.roster.get_mut(source) {
            if remove {
                mono.outbox.retain(|m| m.id != id);
            } else if let Some(message) = mono.outbox.iter_mut().find(|m| m.id == id) {
                message.error = None;
            }
        }
        self.save(cx);
        self.configure_chat(source, cx);
    }
    /// Who speaks in a Mono's chat: for a group, its leader (the Principal);
    /// for every other Mono, itself.
    pub(super) fn mono_speaker(&self, mono: &Mono) -> (String, u32, String) {
        if mono.is_group()
            && let Some(group) = &mono.group
        {
            let lead = group
                .members
                .iter()
                .filter_map(|id| self.roster.get(id))
                .find(|m| m.is_principal())
                .or_else(|| self.roster.monos.iter().find(|m| m.is_principal()));
            if let Some(lead) = lead {
                return (lead.face().into_owned(), lead.color, lead.label());
            }
        }
        (mono.face().into_owned(), mono.color, mono.label())
    }

    /// The people an @ mention can name when typing in this Mono's composer:
    /// in a group chat, its leader and members; otherwise none.
    pub(super) fn group_mention_people(
        &self,
        mono: &Mono,
    ) -> Vec<crate::composer_completion::MentionPerson> {
        let Some(group) = &mono.group else {
            return Vec::new();
        };
        let roster = monocode_core::mono::groups::native::members(&self.roster.monos);
        let lead = monocode_core::mono::groups::brief::group_lead(group, &roster);
        let askable = monocode_core::mono::groups::brief::askable_members(group, &roster);

        let mut people = Vec::new();
        if let Some(lead) = lead {
            let (mascot, color, name) = self
                .roster
                .get(&lead.id)
                .map(|m| (m.face().into_owned(), m.color, m.label()))
                .unwrap_or_else(|| ("principal".to_string(), 0x38bdf8, lead.name.clone()));
            people.push(crate::composer_completion::MentionPerson {
                id: lead.id.clone(),
                name,
                mascot,
                color,
                note: Some("Leads the group".to_string()),
            });
        }
        for member in askable {
            let (mascot, color, name) = self
                .roster
                .get(&member.id)
                .map(|m| (m.face().into_owned(), m.color, m.label()))
                .unwrap_or_else(|| ("robot".to_string(), 0x38bdf8, member.name.clone()));
            people.push(crate::composer_completion::MentionPerson {
                id: member.id.clone(),
                name,
                mascot,
                color,
                note: Some(monocode_core::mono::groups::brief::member_role(member)),
            });
        }
        people
    }

    /// A group's members as avatar stack entries: the Principal first, then
    /// other members, capped at 3 circles with a `+N` badge if more.
    pub(crate) fn group_stack_members(&self, mono: &Mono) -> (Vec<avatar::StackAvatar>, usize) {
        let Some(group) = &mono.group else {
            return (
                vec![avatar::StackAvatar {
                    mascot: mono.face().into_owned(),
                    color: mono.color,
                }],
                0,
            );
        };
        let roster = monocode_core::mono::groups::native::members(&self.roster.monos);
        let lead = monocode_core::mono::groups::brief::group_lead(group, &roster);
        let askable = monocode_core::mono::groups::brief::askable_members(group, &roster);

        let mut members = Vec::new();
        if let Some(lead) = lead {
            let (mascot, color) = self
                .roster
                .get(&lead.id)
                .map(|m| (m.face().into_owned(), m.color))
                .unwrap_or_else(|| ("principal".to_string(), 0x38bdf8));
            members.push(avatar::StackAvatar { mascot, color });
        }
        for member in askable {
            let (mascot, color) = self
                .roster
                .get(&member.id)
                .map(|m| (m.face().into_owned(), m.color))
                .unwrap_or_else(|| ("robot".to_string(), 0x38bdf8));
            members.push(avatar::StackAvatar { mascot, color });
        }

        if members.is_empty() {
            return (
                vec![avatar::StackAvatar {
                    mascot: mono.face().into_owned(),
                    color: mono.color,
                }],
                0,
            );
        }

        const SHOWN: usize = 3;
        let count = members.len();
        let take = if count > SHOWN { SHOWN - 1 } else { count };
        let shown = members[..take].to_vec();
        let more = count.saturating_sub(take);
        (shown, more)
    }

    fn configure_chat(&mut self, id: &str, cx: &mut Context<Self>) {
        self.refresh_work(id, cx);
        if let Some(mono) = self.roster.get(id) {
            // The shell may be mid-update when this runs; reach it afterwards.
            let launches = mono.launches.iter().map(|l| l.session_id).collect();
            let owner = self.owner.clone();
            cx.defer(move |cx| {
                let _ = owner.update(cx, |shell, cx| shell.mono_track_sessions(launches, cx));
            });
        }
        if let (Some(mono), Some(view)) =
            (self.roster.get(id).cloned(), self.chats.get(id).cloned())
        {
            let env = self
                .bridge
                .as_ref()
                .map(|b| b.environment(view.read(cx).summary.id))
                .unwrap_or_default();
            let root = self.root.clone();
            let groups = self.group_context(&mono, None);
            let speaker = self.mono_speaker(&mono);
            let people = self.group_mention_people(&mono);
            view.update(cx, |view, cx| {
                view.configure_mono(mono, root, env, Some(speaker), cx);
                view.set_mono_groups(groups, cx);
                view.set_mention_people(people, cx);
            });
        }
    }
    fn enqueue(
        &mut self,
        id: &str,
        text: String,
        attachments: Vec<String>,
        internal: bool,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let message_id = mono::new_id();
        if let Some(mono) = self.roster.get_mut(id) {
            mono.outbox.push(Message {
                id: message_id.clone(),
                text,
                sent_at: Some(monocode_core::time::now_ms()),
                internal,
                error: None,
                attachments,
            });
            self.save(cx);
            self.configure_chat(id, cx);
            self.dispatch(id, cx);
            return Some(message_id);
        }
        None
    }
    fn dispatch(&mut self, id: &str, cx: &mut Context<Self>) {
        let delivered = self
            .chats
            .get(id)
            .map(|v| v.read(cx).mono_delivered().clone())
            .unwrap_or_default();
        let failures = self
            .chats
            .get(id)
            .cloned()
            .map(|view| view.update(cx, |view, _| view.mono_take_failures()))
            .unwrap_or_default();
        let mut changed = false;
        if let Some(mono) = self.roster.get_mut(id) {
            let before = mono.outbox.len();
            mono.outbox
                .retain(|message| !delivered.contains(&message.id));
            changed = before != mono.outbox.len();
            for (id, error) in failures {
                if let Some(message) = mono.outbox.iter_mut().find(|m| m.id == id) {
                    message.error = Some(error);
                    changed = true;
                }
            }
        }
        if changed {
            self.save(cx);
            self.configure_chat(id, cx);
        }

        if let Some(view) = self
            .chats
            .get(id)
            .cloned()
            .filter(|v| !self.failed_reports.contains(id) && v.read(cx).ready_for_external(cx))
        {
            let reports = self
                .roster
                .get_mut(id)
                .map(|m| std::mem::take(&mut m.pending_reports))
                .unwrap_or_default();
            if !reports.is_empty() {
                let mut failed = Vec::new();
                for report in reports {
                    if self.failed_reports.contains(id) {
                        failed.push(report);
                        continue;
                    }
                    let (role, text) = report
                        .strip_prefix("mono-card:")
                        .map(|s| ("mono-card", s.to_string()))
                        .unwrap_or_else(|| ("assistant", report.clone()));
                    let body = (role == "assistant").then(|| {
                        text.strip_prefix("**Habit report**")
                            .unwrap_or(&text)
                            .trim()
                            .to_owned()
                    });
                    if let Err(error) = view.update(cx, |v, cx| v.mono_post(role, text, cx)) {
                        self.error = Some(error);
                        self.failed_reports.insert(id.into());
                        failed.push(report);
                    } else if let Some(body) = body {
                        let session = view.read(cx).summary.id;
                        cx.emit(NotificationPosted { session, body });
                    }
                }
                if let Some(mono) = self.roster.get_mut(id) {
                    mono.pending_reports.extend(failed);
                }
                self.save(cx);
            }
        }
        let Some(view) = self.chats.get(id).cloned() else {
            return;
        };
        let busy = view.read(cx).busy;
        // A message whose send failed keeps its Retry/Discard card, but it
        // must not hold back the ones queued behind it (the desktop dequeues
        // a message once its turn takes it, so a mid-turn failure never
        // requeues there either).
        let Some(message) = self
            .roster
            .get(id)
            .and_then(|m| m.outbox.iter().find(|message| can_dispatch(message, busy)))
            .cloned()
            .filter(|m| self.persisted_messages.contains(&m.id))
        else {
            return;
        };
        if view.update(cx, |v, cx| v.mono_dispatch(&message, cx)) && !busy {
            self.inflight.insert(id.into(), message);
        }
    }
    fn chat_changed(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(view) = self.chats.get(id) else {
            return;
        };
        let view = view.read(cx);
        let row_count = view.mono_rows(cx).len();
        let finished = !view.busy;
        let completion =
            (finished && self.inflight.contains_key(id)).then(|| NotificationFinished {
                summary: view.summary.clone(),
                body: view.notification_reply_body(cx),
                habit: false,
                was_cancelled: view.notification_was_cancelled(),
            });
        cx.emit(NotificationChanged::Request);
        if let Some(completion) = completion {
            cx.emit(completion);
        }
        if self.active.as_deref() == Some(id)
            && let Some(mono) = self.roster.get_mut(id)
        {
            mono.seen_rows = row_count;
        }
        if finished {
            let (mut ok, mut status) = self.chats[id].read(cx).external_outcome(cx);
            if ok
                && self.chats[id].read(cx).notification_was_cancelled()
                && self.inflight.get(id).is_some_and(|message| {
                    monocode_core::mono::agents::message_sender(&message.text).is_some()
                })
            {
                ok = false;
                status = "Agent request was stopped. Retry the message.".into();
            }
            if let Some(sent) = self.inflight.get(id).map(|m| m.id.clone()) {
                if ok && !self.agent_answered(id, &sent, cx) {
                    ok = false;
                    status =
                        "Could not find the agent request in the conversation. Retry the message."
                            .into();
                }
                if ok {
                    self.bot_turn_news(id, cx);
                }
            }
            if let Some(mut sent) = self.inflight.remove(id)
                && !ok
                && !status.starts_with("Stopped")
            {
                let old_id = sent.id.clone();
                let mut retry_id = None;
                if let Some(mono) = self.roster.get_mut(id) {
                    if let Some(queued) = mono.outbox.iter_mut().find(|m| m.id == sent.id) {
                        queued.error = Some(status);
                    } else {
                        sent.id = mono::new_id();
                        retry_id = Some(sent.id.clone());
                        sent.error = Some(status);
                        mono.outbox.insert(0, sent);
                    }
                }
                if let Some(retry_id) = retry_id {
                    self.agent_calls.retry_route(&old_id, &retry_id);
                }
                self.save(cx);
                self.configure_chat(id, cx);
            }
            self.check_completions(cx);
            self.reload_files(id, cx);
            // A group's chat that is free again takes its members' posts
            // and the last word on a finished round.
            if self.roster.get(id).is_some_and(Mono::is_group) {
                self.check_groups(cx);
            }
        }
        self.dispatch(id, cx);
        self.changed(cx);
    }
    pub(crate) fn open(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        // The Monos switch hides the coding Monos; bots and groups stay open.
        let Some(agent) = self.roster.get(id).map(|m| !m.is_coding()) else {
            return;
        };
        if !self.prefs.monos && !agent {
            return;
        }
        // The bots' News lives with the Principal: opening it reads them.
        if self.roster.get(id).is_some_and(Mono::is_principal) {
            self.mark_news_seen(cx);
        }
        let mut overview = false;
        if self.active.as_deref() != Some(id) {
            self.reset_find(self.active.as_deref(), cx);
            self.call_hang_up(cx);
            self.save_open_file(cx);
            self.pause_changes(cx);
            self.route = None;
            self.details.update(cx, |details, cx| details.reset(cx));
            self.fields.retain(|k, _| k.starts_with("file:"));
            self.files = None;
            self.activity_turn = None;
            self.memory_adding = false;
            self.members_adding = false;
            self.memory_edit = None;
            self.settings = None;
            // A bot opens on what a person checks first: the Principal on
            // its news, any other bot or group on what it is doing.
            if let Some(mono) = self.roster.get(id).filter(|m| m.is_personal()) {
                let first = panels::overview_tabs(mono)[0];
                self.overview_previous = first;
                self.overview_tab = if mono.is_principal() {
                    OverviewTab::News
                } else {
                    OverviewTab::Activity
                };
                self.overview_reveal = None;
                overview = true;
            }
        }
        self.active = Some(id.into());
        if overview {
            self.show_overview(cx);
        }
        self.ensure_chat(id, cx);
        self.reload_files(id, cx);
        cx.emit(TabOpened(id.into()));
        self.focus_active(window, cx);
        if let Some(view) = self.chats.get(id) {
            let count = view.read(cx).mono_rows(cx).len();
            if let Some(mono) = self.roster.get_mut(id) {
                mono.seen_rows = count;
            }
        }
        self.save(cx);
    }
    /// App.tsx's openPersonalBot: the last bot or group used, else the
    /// Principal, made now if there is none yet.
    pub(crate) fn open_personal(
        &mut self,
        last: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let found = last
            .filter(|id| self.roster.get(id).is_some_and(Mono::is_personal))
            .map(str::to_owned)
            .or_else(|| {
                self.roster
                    .monos
                    .iter()
                    .find(|m| m.is_principal())
                    .map(|m| m.id.clone())
            });
        let created = found.is_none();
        let Some(id) = found.or_else(|| {
            self.roster
                .create_bot(&self.root, "principal", false)
                .map(|m| m.id.clone())
        }) else {
            return;
        };
        self.open(&id, window, cx);
        if created {
            self.changed(cx);
        }
    }
    /// Closes the find bar of the chat being left, so the next agent or
    /// group opens without the last one's search.
    pub(crate) fn reset_find(&self, id: Option<&str>, cx: &mut Context<Self>) {
        if let Some(chat) = id.and_then(|id| self.chats.get(id)) {
            chat.update(cx, |chat, cx| chat.reset_find(cx));
        }
    }
    pub(crate) fn focus_active(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.is_none()
            && let Some(chat) = self.chat()
        {
            window.focus(&chat.read(cx).input.focus_handle(cx), cx);
        } else {
            window.focus(&self.settings_focus, cx);
        }
    }
    pub(crate) fn close(&mut self, cx: &mut Context<Self>) {
        self.call_hang_up(cx);
        self.save_open_file(cx);
        self.pause_changes(cx);
        self.hide_html(cx);
        if let Some(id) = self.active.take() {
            self.reset_find(Some(&id), cx);
            self.details.update(cx, |details, cx| details.reset(cx));
            self.settings = None;
            self.picker = None;
            self.save(cx);
        }
    }
    pub(crate) fn close_group_call(&mut self, cx: &mut Context<Self>) {
        self.call_hang_up(cx);
    }
    pub(crate) fn dismiss_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.picker.take().is_some() || self.menu.take().is_some() {
            cx.notify();
            return true;
        }
        if self.settings.take().is_none() {
            return false;
        }
        self.save_open_file(cx);
        if let Some(chat) = self.chat() {
            window.focus(&chat.read(cx).input.focus_handle(cx), cx);
        }
        cx.notify();
        true
    }
    pub(crate) fn create(
        &mut self,
        projects: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.roster.create(projects).id.clone();
        self.save(cx);
        self.open(&id, window, cx);
        self.settings = Some(SettingsSection::General);
        window.focus(&self.settings_focus, cx);
        self.changed(cx);
    }
    fn reload_files(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.active.as_deref() != Some(id) {
            return;
        }
        self.file_load_revision += 1;
        let revision = self.file_load_revision;
        let Some(mono) = self.roster.get(id).cloned() else {
            return;
        };
        let root = self.root.clone();
        let id = id.to_string();
        let work = cx.background_executor().spawn(async move {
            Ok::<_, String>((
                mono::load_files(&root, &mono)?,
                habits::load(&root, &mono.id)?,
            ))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.active.as_deref() == Some(&id) && this.file_load_revision == revision {
                    match result {
                        Ok((files, habits)) => {
                            this.files = Some(files);
                            this.habits = habits;
                        }
                        Err(error) => this.error = Some(error),
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }
    /// The Code zone's Monos, then its workstreams (the project's, when
    /// one is given), as ProjectRail and the inset sidebar list them.
    pub(crate) fn rail(&mut self, project: Option<String>, cx: &mut Context<Self>) -> AnyElement {
        self.rail_ids.clear();
        let monos = self.rail_impl(project.clone(), false, false, agents::Section::Coding, cx);
        let workstreams = self.rail_impl(project, false, false, agents::Section::Workstreams, cx);
        div()
            .w_full()
            .flex()
            .flex_col()
            .child(monos)
            .child(workstreams)
            .into_any_element()
    }
    /// Original navigation lists every Mono, before the projects; the inset
    /// layout continues to list only the current project's Monos.
    pub(crate) fn project_navigation_rail(
        &mut self,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.rail_ids.clear();
        let monos = self.rail_impl(None, true, compact, agents::Section::Coding, cx);
        let workstreams = self.rail_impl(None, true, compact, agents::Section::Workstreams, cx);
        div()
            .w_full()
            .flex()
            .flex_col()
            .when(compact, |d| d.items_center().gap(px(6.)))
            .child(monos)
            .child(workstreams)
            .into_any_element()
    }
    fn rail_impl(
        &mut self,
        project: Option<String>,
        original: bool,
        compact: bool,
        section: agents::Section,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let workstreams = section == agents::Section::Workstreams;
        if !self.prefs.monos && (section == agents::Section::Coding || workstreams) {
            return div().into_any_element();
        }
        let roster: Vec<_> = self
            .roster
            .monos
            .iter()
            .filter(|m| section.holds(m))
            .filter(|m| project.as_ref().is_none_or(|p| m.works_on(p)))
            .cloned()
            .collect();
        // Every section on screen adds its rows, for dragging within it.
        self.rail_ids.extend(roster.iter().map(|m| m.id.clone()));
        // PersonalRail: the Principal's row says there is unread News.
        let news_dot = section == agents::Section::Principal && self.has_unseen_news();
        for mono in &roster {
            self.rail_bounds.entry(mono.id.clone()).or_default();
        }
        let count = roster.len();
        let has = count > 0;
        if original && !has && compact {
            return div().into_any_element();
        }
        let add_project = project.clone();
        let preview = self.mono_preview_card(&roster, cx);
        let heading = if original {
            div()
                .h(px(30.))
                .pl(px(10.))
                .pr_3()
                .pt_1()
                .pb(px(6.))
                .flex()
                .items_center()
                .gap_1()
                .when(compact, |d| d.px(px(14.)))
                .when(!compact, |d| {
                    d.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px_1()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(12.))
                            .line_height(px(20.))
                            .text_color(content(0.5))
                            .child(section.label())
                            .when(workstreams, |d| d.child(beta_badge())),
                    )
                })
        } else {
            section_label_with(section.label(), workstreams)
        }
        .when(has && section == agents::Section::Bots, |d| {
            d.child(self.new_bot_button(cx))
        })
        .when(has && section == agents::Section::Groups, |d| {
            d.child(self.new_group_button(cx))
        })
        .when(workstreams, |d| d.child(self.workstream_help_button(cx)))
        .when(has && workstreams, |d| {
            d.child(self.new_workstream_button(cx))
        })
        .when(has && section == agents::Section::Coding, |d| {
            d.child(
                icon_button("new-mono", "New mono", "plus")
                    .debug_selector(|| "sidebar-mono-button".into())
                    .size(px(20.))
                    // Centred on the sidebar's trailing column, with the
                    // session filter and the navigation hints and counts.
                    .when(!original, |d| d.mr(px(-3.)))
                    .on_click(cx.listener(move |this, _, w, cx| {
                        this.create(add_project.clone().into_iter().collect(), w, cx)
                    })),
            )
        });
        div()
            .on_scroll_wheel(cx.listener(|this, _, _, cx| this.close_mono_preview(cx)))
            .when(!original, |d| d.px_2().pb_3())
            .when(original && !compact, |d| d.pb_2())
            .when(compact, |d| {
                d.w_full().flex().flex_col().items_center().gap(px(6.))
            })
            .when(
                !compact
                    && section != agents::Section::Principal
                    && !(section == agents::Section::Coding && !has && project.is_none()),
                |d| d.child(heading),
            )
            // MonoIntroCard: with no Mono yet, the card stands in for the section.
            .when(
                !has && section == agents::Section::Coding && project.is_none() && !compact,
                |d| d.child(self.mono_intro_card(cx)),
            )
            .when(!has && section == agents::Section::Bots, |d| {
                d.child(self.new_bot_row(cx))
            })
            .when(!has && section == agents::Section::Groups, |d| {
                d.child(self.new_group_row(cx))
            })
            .when(!has && workstreams, |d| {
                d.child(self.new_workstream_row(original, cx))
            })
            .when(
                !has && section == agents::Section::Coding && project.is_some(),
                |d| {
                    d.child(
                        div()
                            .id("new-mono-row")
                            .debug_selector(|| "new-mono".into())
                            .role(Role::Button)
                            .aria_label("New mono")
                            .h(px(32.))
                            .when(original, |d| {
                                d.ml(px(6.)).mr_2().font_weight(gpui::FontWeight::MEDIUM)
                            })
                            .px_2()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(px(14.))
                            .text_color(content(0.5))
                            .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
                            .child(icon("plus", 16.))
                            .child("New mono")
                            .on_click(cx.listener(move |this, _, w, cx| {
                                this.create(project.clone().into_iter().collect(), w, cx)
                            })),
                    )
                },
            )
            .children(roster.into_iter().enumerate().map(|(index, mono)| {
                let active = self.active.as_deref() == Some(&mono.id);
                let state = self
                    .chats
                    .get(&mono.id)
                    .map(|v| v.read(cx).mono_status())
                    .unwrap_or("idle");
                let unread = self
                    .chats
                    .get(&mono.id)
                    .map(|v| {
                        v.read(cx)
                            .mono_rows(cx)
                            .iter()
                            .skip(mono.seen_rows)
                            .filter(|r| {
                                r.role == "assistant" && r.first && !r.text.trim().is_empty()
                            })
                            .count()
                    })
                    .unwrap_or(0);
                // Like MonoRailSection, an open Mono has already been seen.
                // Activity belongs on the mascot, rather than an idle dot at
                // the end of every row.
                let unread = if active { 0 } else { unread };
                let label = if unread == 0 {
                    format!("{}, {state}", mono.label())
                } else {
                    format!(
                        "{}, {state}, {unread} new {}",
                        mono.label(),
                        if unread == 1 { "reply" } else { "replies" }
                    )
                };
                let id = mono.id.clone();
                let options = id.clone();
                let context = id.clone();
                let drag = id.clone();
                let slot = self.rail_bounds[&id].clone();
                div()
                    .id(SharedString::from(format!("mono-rail-{id}")))
                    .debug_selector({
                        let id = id.clone();
                        move || format!("mono-rail-{id}")
                    })
                    .group("mono-rail-row")
                    .role(Role::Button)
                    .aria_label(label)
                    .h(px(32.))
                    .when(original && !compact, |d| d.ml(px(6.)).mr_2())
                    .when(original && !compact && index + 1 < count, |d| d.mb_px())
                    .when(compact, |d| d.w(px(32.)))
                    .px_2()
                    .rounded_md()
                    .relative()
                    .flex()
                    .items_center()
                    .gap_2()
                    .opacity(if active { 1. } else { 0.65 })
                    .when(active, |d| d.bg(content(0.1)))
                    .hover(|s| s.bg(content(0.08)).opacity(1.))
                    .child(if mono.is_group() && !compact {
                        let (shown, more) = self.group_stack_members(&mono);
                        avatar::group_avatar_stack(&shown, state, more, 18., original)
                    } else if mono.is_group() && compact {
                        let (shown, _) = self.group_stack_members(&mono);
                        avatar::group_avatar_stack(
                            &shown[..1.min(shown.len())],
                            state,
                            0,
                            16.,
                            original,
                        )
                    } else {
                        div()
                            .size(px(16.))
                            .relative()
                            .flex_shrink_0()
                            .child(avatar::avatar(&mono.face(), mono.color, state, 16., true))
                            .when(original, |d| {
                                d.child(
                                    div()
                                        .absolute()
                                        .right(px(-2.))
                                        .top(px(-2.))
                                        .size(px(6.))
                                        .rounded_full()
                                        .border_1()
                                        .border_color(crate::theme::background_base())
                                        .bg(if state == "working" {
                                            rgb(mono.color).into()
                                        } else if state == "needs-you" {
                                            accent_color()
                                        } else {
                                            content(0.3)
                                        }),
                                )
                            })
                            .into_any_element()
                    })
                    .when(!compact, |d| {
                        // PersonalRail: the pill takes room from the name,
                        // never from the counts after it.
                        let principal = mono.is_principal();
                        d.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .when(unread > 0, |d| d.pr(px(24.)))
                                .group_hover("mono-rail-row", |s| s.pr(px(24.)))
                                .text_size(px(14.))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .child(div().min_w_0().truncate().child(mono.label()))
                                .when(principal, |d| {
                                    d.child(
                                        div()
                                            .id(SharedString::from(format!(
                                                "orchestrator-pill-{}",
                                                mono.id
                                            )))
                                            .debug_selector(|| "mono-rail-orchestrator".into())
                                            .tooltip(chrome_tip(
                                                "Your main bot: it coordinates the others",
                                                None,
                                            ))
                                            .flex_shrink_0()
                                            .px_1()
                                            .text_size(px(10.))
                                            .line_height(px(16.))
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .bg(accent_color().opacity(0.1))
                                            .text_color(accent_color())
                                            .child("Orchestrator"),
                                    )
                                }),
                        )
                    })
                    .when(unread == 0 && news_dot && !compact, |d| {
                        d.child(
                            div()
                                .debug_selector(|| "mono-rail-news-dot".into())
                                .absolute()
                                .right(px(13.))
                                .top(px(13.))
                                .size(px(6.))
                                .rounded_full()
                                .bg(accent_color())
                                .group_hover("mono-rail-row", |s| s.invisible()),
                        )
                    })
                    .when(unread > 0 && !compact, |d| {
                        d.child(
                            div()
                                .absolute()
                                .right_1()
                                .top(px(4.))
                                .size(px(24.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .group_hover("mono-rail-row", |s| s.invisible())
                                .child(
                                    div()
                                        .debug_selector({
                                            let id = mono.id.clone();
                                            move || format!("mono-rail-unread-{id}")
                                        })
                                        .when(!original, |d| d.size(px(6.)))
                                        .when(original, |d| {
                                            d.h(px(16.))
                                                .min_w(px(16.))
                                                .px_1()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(10.))
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .text_color(rgb(0xffffff))
                                                .child(if unread > 99 {
                                                    "99+".into()
                                                } else {
                                                    unread.to_string()
                                                })
                                        })
                                        .rounded_full()
                                        .bg(accent_color()),
                                ),
                        )
                    })
                    .when(!compact, |d| {
                        d.child(
                            div()
                                .absolute()
                                .right_1()
                                .top(px(4.))
                                .invisible()
                                .group_hover("mono-rail-row", |s| s.visible())
                                .child(
                                    icon_button("mono-options", "Mono options", "more-horizontal")
                                        .debug_selector({
                                            let id = mono.id.clone();
                                            move || format!("mono-rail-options-{id}")
                                        })
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        })
                                        .on_click(cx.listener(move |this, _, w, cx| {
                                            cx.stop_propagation();
                                            this.close_mono_preview(cx);
                                            this.open_options(&options, None, w, cx);
                                        })),
                                ),
                        )
                    })
                    .child(slot.probe())
                    // MonoCardPreview: the row's card opens beside it.
                    .on_hover({
                        let id = id.clone();
                        let slot = slot.clone();
                        cx.listener(move |this, hovered: &bool, w, cx| {
                            this.mono_row_hover(&id, slot.clone(), *hovered, w, cx)
                        })
                    })
                    .on_click(cx.listener(move |this, _, w, cx| {
                        if std::mem::take(&mut this.skip_click) {
                            return;
                        }
                        this.open(&id, w, cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, w, cx| {
                            this.close_mono_preview(cx);
                            this.open_options(&context, Some(event.position), w, cx)
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _, _| {
                            this.skip_click = false;
                            this.drag = Some((drag.clone(), event.position, None));
                        }),
                    )
            }))
            .children(preview)
            // MonoRailSection owns its menu and background dialog; the Mono
            // view paints them instead while it is open. The Monos section
            // paints them for the Workstreams below it: twice would break
            // their clicks.
            .when(
                self.current().is_none() && self.owns_modals(section, compact),
                |d| {
                    d.children(self.options_surface(cx))
                        .children(self.background_modal(cx))
                        .children(self.group_dialog_modal(cx))
                        .children(self.mail_connect_modal(cx))
                },
            )
            .into_any_element()
    }
    pub(crate) fn title_tab(&self, id: &str, width: f32, cx: &Context<Self>) -> Option<AnyElement> {
        self.roster.get(id).map(|mono| {
            let id = mono.id.clone();
            let close = id.clone();
            let active = self.active.as_deref() == Some(&id);
            let selector = format!("mono-title-tab-{id}");
            let group: SharedString = selector.clone().into();
            div()
                .id(SharedString::from(format!("mono-title-{id}")))
                .debug_selector(move || selector.clone())
                .role(Role::Tab)
                .aria_label(mono.label())
                .aria_selected(active)
                .h(px(TITLE_BAR_HEIGHT))
                .w(px(width))
                .flex_shrink_0()
                .relative()
                .flex()
                .items_center()
                .group(group.clone())
                .child(
                    title_tab_plate(active, false)
                        .w_full()
                        .min_w_0()
                        .pr_7()
                        .gap(px(crate::tab_tile::TAB_TILE_GAP))
                        .group_hover(group.clone(), move |s| {
                            s.opacity(1.).bg(title_tab_fill(active, true))
                        })
                        .child(crate::tab_tile::mark_tile(
                            &crate::tab_tile::TAB_TILE,
                            crate::tab_tile::Face::Neutral,
                            avatar::avatar(&mono.face(), mono.color, "idle", 14., true),
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h(px(20.))
                                .text_size(px(TITLE_TAB_TEXT_SIZE))
                                .text_color(content(if active { 1. } else { 0.5 }))
                                .group_hover(group.clone(), |s| s.text_color(content(1.)))
                                .child(crate::tab_label::tab_label(mono.label())),
                        ),
                )
                .child(
                    div()
                        .id("close-mono-tab")
                        .debug_selector(|| "close-mono-tab-button".into())
                        .role(Role::Button)
                        .aria_label("Close Mono tab")
                        .tooltip(chrome_tip("Close Mono tab", Some("Pane: Close")))
                        .absolute()
                        .right_1()
                        .top(px(10.))
                        .size(px(20.))
                        .rounded_sm()
                        .flex()
                        .items_center()
                        .justify_center()
                        .opacity(0.)
                        .group_hover(group, |s| s.opacity(1.))
                        .hover(|s| s.bg(content(0.1)))
                        .child(icon("close", 12.))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            let owner = this.owner.clone();
                            let close = close.clone();
                            window.defer(cx, move |window, cx| {
                                let _ = owner.update(cx, |shell, cx| {
                                    shell.close_mono_tab(&close, window, cx)
                                });
                            });
                        })),
                )
                .on_mouse_up(
                    MouseButton::Middle,
                    cx.listener({
                        let id = id.clone();
                        move |this, _, window, cx| {
                            cx.stop_propagation();
                            let owner = this.owner.clone();
                            let id = id.clone();
                            window.defer(cx, move |window, cx| {
                                let _ = owner
                                    .update(cx, |shell, cx| shell.close_mono_tab(&id, window, cx));
                            });
                        }
                    }),
                )
                .on_click(cx.listener(move |this, _, w, cx| {
                    let owner = this.owner.clone();
                    let id = id.clone();
                    w.defer(cx, move |w, cx| {
                        let _ = owner.update(cx, |shell, cx| shell.click_mono_tab(&id, w, cx));
                    });
                }))
                .into_any_element()
        })
    }
}
impl gpui::Render for Monos {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.hide_html(cx);
        let Some(mono) = self.current().cloned() else {
            return div().into_any_element();
        };
        self.ensure_changes_summary(&mono, cx);
        let bg = self
            .backgrounds
            .entry(mono.id.clone())
            .or_insert_with(|| cx.new(|_| crate::project_background::ProjectBackground::default()))
            .clone();
        let empty = self.chat().is_none_or(|v| !v.read(cx).summary.has_messages);
        bg.update(cx, |bg, cx| {
            bg.configure(mono.background.clone(), empty, None, cx)
        });
        div()
            .size_full()
            .min_w_0()
            .flex()
            .relative()
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape"
                    && this.settings.is_some()
                    && this.picker.is_none()
                    && this.menu.is_none()
                {
                    this.save_open_file(cx);
                    this.settings = None;
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .capture_any_mouse_down(|_, window, _| {
                #[cfg(target_os = "macos")]
                crate::traffic_lights::focus_content(window);
                #[cfg(not(target_os = "macos"))]
                let _ = window;
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .relative()
                    .child(bg)
                    .children(
                        self.attention_views(&mono.id, cx)
                            .into_iter()
                            .map(|view| view.update(cx, |v, cx| v.mono_attention(window, cx))),
                    )
                    .map(|d| {
                        if self.floating_shows(&mono.id, cx) {
                            d.child(self.floating_placeholder(&mono, cx))
                        } else if let Some(chat) = self.chat() {
                            d.child(div().flex_1().min_h_0().child(chat))
                        } else {
                            d
                        }
                    })
                    .when(!self.original_chrome, |d| d.child(self.topbar(&mono, cx))),
            )
            .children(self.settings_modal(&mono, window, cx))
            .children(self.picker_surface(cx))
            .children(self.options_surface(cx))
            .children(self.background_modal(cx))
            .children(self.group_dialog_modal(cx))
            .children(self.mail_connect_modal(cx))
            .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, _| {
                if let Some((_, start, over)) = &mut this.drag
                    && (event.position - *start).magnitude() > 5.
                {
                    this.skip_click = true;
                    this.mono_preview.close();
                    *over = this.rail_bounds.iter().find_map(|(id, s)| {
                        s.get()
                            .filter(|b| b.contains(&event.position))
                            .map(|_| id.clone())
                    });
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.finish_reorder(cx);
                }),
            )
            .into_any_element()
    }
}
