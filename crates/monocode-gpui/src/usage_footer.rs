use crate::{
    popover::{Anchor, AnchorSlot, Side, popover},
    theme::*,
};
use gpui::{
    App, Context, FocusHandle, FontWeight, KeyBinding, Role, Window, actions, div, prelude::*, px,
    rgb,
};
#[cfg(test)]
use monocode_core::usage::{FetchError, Snapshot};
use monocode_core::{provider::Kind, usage};
use std::time::{Duration, Instant};

mod resets;

actions!(usage_footer, [DismissUsage, ToggleUsage, RefreshUsage]);
pub(crate) fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", DismissUsage, Some("UsageDetails")),
        KeyBinding::new("enter", ToggleUsage, Some("UsageChip")),
        KeyBinding::new("space", ToggleUsage, Some("UsageChip")),
        KeyBinding::new("enter", RefreshUsage, Some("UsageRefresh")),
        KeyBinding::new("space", RefreshUsage, Some("UsageRefresh")),
    ]);
}

use crate::accounts::{Cached as CachedUsage, Key};

pub(crate) const RATE_LIMIT_PROVIDERS: [Kind; 4] =
    [Kind::Claude, Kind::Codex, Kind::Devin, Kind::OpenCode];

/// The plans the footer shows: the one in view first, then every other
/// provider whose CLI is installed and that this project does not hide.
pub(crate) fn footer_usage_providers(
    in_view: Option<Kind>,
    installed: impl Fn(Kind) -> bool,
    hidden: impl Fn(Kind) -> bool,
) -> Vec<Kind> {
    let mut out = Vec::new();
    if let Some(kind) = in_view
        && RATE_LIMIT_PROVIDERS.contains(&kind)
    {
        out.push(kind);
    }
    for provider in RATE_LIMIT_PROVIDERS {
        if Some(provider) != in_view && installed(provider) && !hidden(provider) {
            out.push(provider);
        }
    }
    out
}

fn index(kind: Kind) -> usize {
    match kind {
        Kind::Codex => 0,
        Kind::Claude => 1,
        _ => {
            2 + Kind::ALL
                .iter()
                .filter(|k| !matches!(k, Kind::Codex | Kind::Claude))
                .position(|k| *k == kind)
                .unwrap()
        }
    }
}

/// Its own cached surface: clock ticks and probe completions don't rebuild chat.
pub(crate) struct UsageFooter {
    kind: Kind,
    details_kind: Kind,
    key: Key,
    project: String,
    session: Option<i64>,
    show_accounts: bool,
    confirm_reset: Option<String>,
    _accounts_observation: Option<gpui::Subscription>,
    _account_events: Option<gpui::Subscription>,
    installed: [bool; Kind::ALL.len()],
    cache: [CachedUsage; Kind::ALL.len()],
    anchor: AnchorSlot,
    anchors: [AnchorSlot; Kind::ALL.len()],
    trigger: FocusHandle,
    triggers: [FocusHandle; Kind::ALL.len()],
    refresh_focus: FocusHandle,
    details: FocusHandle,
    previous_focus: Option<FocusHandle>,
    open: bool,
    /// UsageFooter's `footer` variant: the classic layout's bordered strip.
    classic: bool,
    show_remaining: bool,
    _clock: gpui::Task<()>,
}
impl UsageFooter {
    pub(crate) fn context(
        &mut self,
        kind: Kind,
        profile: monocode_core::accounts::Profile,
        model: Option<&str>,
        project: String,
        session: Option<i64>,
        cx: &mut Context<Self>,
    ) {
        let key = Key::new(kind, profile, model);
        let project_changed = self.project != project;
        self.project = project;
        self.session = session;
        if self.key != key || project_changed {
            if self.key != key {
                self.kind = kind;
                self.details_kind = kind;
                self.key = key;
                self.open = false;
                self.show_accounts = false;
                self.confirm_reset = None;
                self.cache[index(kind)] = Default::default();
            }
            self.sync_from_center(cx);
            self.refresh(false, cx);
            cx.notify();
        }
    }
    pub(crate) fn set_project(&mut self, project: String, cx: &mut Context<Self>) {
        if self.project != project {
            self.project = project;
            self.session = None;
            self.sync_from_center(cx);
            self.refresh(false, cx);
            cx.notify();
        }
    }
    pub(crate) fn new(kind: Kind, cx: &mut Context<Self>) -> Self {
        let clock = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(30))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        this.refresh(false, cx);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let observation = crate::accounts::get(cx).map(|center| {
            cx.observe(&center, |this, _, cx| {
                this.sync_from_center(cx);
                cx.notify();
            })
        });
        let triggers = std::array::from_fn(|_| cx.focus_handle());
        let mut this = Self {
            _account_events: crate::accounts::get(cx).map(|center| {
                cx.subscribe(
                    &center,
                    |_this, center, event: &crate::accounts::AccountEvent, cx| {
                        let modal_auth =
                            matches!(event, crate::accounts::AccountEvent::AuthStarted)
                                && center.read(cx).has_modal_auth();
                        if modal_auth
                            || matches!(
                                event,
                                crate::accounts::AccountEvent::Select { .. }
                                    | crate::accounts::AccountEvent::PickerDismiss
                                    | crate::accounts::AccountEvent::Manage
                            )
                        {
                            cx.spawn(async move |this, cx| {
                                let _ =
                                    this.update_in(cx, |this, window, cx| this.dismiss(window, cx));
                            })
                            .detach();
                        }
                    },
                )
            }),
            kind,
            details_kind: kind,
            key: Key::new(kind, Default::default(), None),
            project: String::new(),
            session: None,
            show_accounts: false,
            confirm_reset: None,
            _accounts_observation: observation,
            installed: [false; Kind::ALL.len()],
            cache: Default::default(),
            anchor: AnchorSlot::default(),
            anchors: Default::default(),
            trigger: cx.focus_handle(),
            triggers,
            refresh_focus: cx.focus_handle(),
            details: cx.focus_handle(),
            previous_focus: None,
            open: false,
            classic: false,
            show_remaining: false,
            _clock: clock,
        };
        this.detect_installed(cx);
        this.refresh(false, cx);
        this
    }
    pub(crate) fn detect_installed(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) {
            return;
        }
        let work = cx.background_executor().spawn(async move {
            RATE_LIMIT_PROVIDERS.map(|k| (k, monocode_core::provider::resolve(k).is_some()))
        });
        cx.spawn(async move |this, cx| {
            let found = work.await;
            let _ = this.update(cx, |this, cx| {
                let mut changed = false;
                for (kind, ok) in found {
                    let slot = index(kind);
                    if this.installed[slot] != ok {
                        this.installed[slot] = ok;
                        changed = true;
                    }
                }
                if changed {
                    this.sync_from_center(cx);
                    this.refresh(false, cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }
    pub(super) fn key_for(&self, kind: Kind, cx: &App) -> Key {
        if kind == self.kind {
            return self.key.clone();
        }
        if monocode_core::accounts::supports_accounts(kind)
            && let Some(center) = crate::accounts::get(cx)
        {
            return Key::new(kind, center.read(cx).selected(kind, &self.project), None);
        }
        Key::new(kind, Default::default(), None)
    }
    fn sync_from_center(&mut self, cx: &App) {
        let Some(center) = crate::accounts::get(cx) else {
            return;
        };
        for &kind in &Kind::ALL {
            let key = self.key_for(kind, cx);
            if let Some(state) = center.read(cx).usage.get(&key).cloned() {
                self.cache[index(kind)] = state;
            }
        }
    }
    pub(crate) fn providers(&self, cx: &App) -> Vec<Kind> {
        if self.kind == Kind::Pi {
            return vec![Kind::Pi];
        }
        let hidden = cx
            .try_global::<crate::settings::Preferences>()
            .map(|p| p.hidden_for(&self.project))
            .unwrap_or_default();
        footer_usage_providers(
            Some(self.kind),
            |k| {
                self.installed[index(k)]
                    || self.cache[index(k)].snapshot.is_some()
                    || self.cache[index(k)].error.is_some()
                    || self.cache[index(k)].fetching
            },
            |k| hidden.iter().any(|id| id == k.id()),
        )
    }
    fn visible_providers(&self, cx: &App) -> Vec<Kind> {
        let providers = self.providers(cx);
        providers
            .into_iter()
            .filter(|&k| {
                k != Kind::OpenCode
                    || (cfg!(test) && self.kind == Kind::OpenCode)
                    || !self.cache[index(Kind::OpenCode)]
                        .error
                        .as_ref()
                        .is_some_and(|e| e.unavailable)
            })
            .collect()
    }
    fn anchor_for(&self, kind: Kind) -> &AnchorSlot {
        if kind == self.kind {
            &self.anchor
        } else {
            &self.anchors[index(kind)]
        }
    }
    fn trigger_for(&self, kind: Kind) -> &FocusHandle {
        if kind == self.kind {
            &self.trigger
        } else {
            &self.triggers[index(kind)]
        }
    }
    pub(crate) fn set_remaining(&mut self, remaining: bool, cx: &mut Context<Self>) {
        self.show_remaining = remaining;
        self.detect_installed(cx);
        self.sync_from_center(cx);
        self.refresh(false, cx);
        cx.notify();
    }
    fn display_percent(&self, used: f64) -> f64 {
        if self.show_remaining {
            100. - used
        } else {
            used
        }
    }
    fn percent_label(&self, used: f64) -> String {
        format!(
            "{:.0}% {}",
            self.display_percent(used),
            if self.show_remaining { "left" } else { "used" }
        )
    }
    pub(crate) fn set_classic(&mut self, classic: bool, cx: &mut Context<Self>) {
        if self.classic != classic {
            self.classic = classic;
            cx.notify();
        }
    }
    #[cfg(test)]
    pub(crate) fn provider(&mut self, kind: Kind, cx: &mut Context<Self>) {
        if self.kind != kind {
            self.kind = kind;
            self.details_kind = kind;
            self.key = Key::new(kind, Default::default(), None);
            self.open = false;
            self.refresh(false, cx);
            cx.notify();
        }
    }
    fn refresh(&mut self, force: bool, cx: &mut Context<Self>) {
        if force {
            self.detect_installed(cx);
        }
        let providers = self.providers(cx);
        if let Some(center) = crate::accounts::get(cx) {
            for kind in providers {
                let key = self.key_for(kind, cx);
                center.update(cx, |center, cx| center.refresh(key.clone(), force, cx));
                if let Some(state) = center.read(cx).usage.get(&key).cloned() {
                    self.cache[index(kind)] = state;
                }
            }
            cx.notify();
            return;
        }
        if cfg!(test) {
            return;
        }
        for kind in providers {
            let slot = index(kind);
            let state = &mut self.cache[slot];
            let reset = state.snapshot.as_ref().is_some_and(|s| {
                s.windows.iter().any(|w| {
                    w.resets_at
                        .is_some_and(|at| at > s.updated_at && at <= usage::now_ms())
                })
            });
            let interval = if reset { 300 } else { 900 };
            if state.fetching
                || (!force
                    && state
                        .attempted
                        .is_some_and(|at| at.elapsed() < Duration::from_secs(interval)))
            {
                continue;
            }
            state.fetching = true;
            state.attempted = Some(Instant::now());
            let work = cx
                .background_executor()
                .spawn(async move { usage::fetch(kind) });
            cx.spawn(async move |this, cx| {
                let result = work.await;
                let _ = this.update(cx, |this, cx| {
                    // Store by the requested provider, even if the user changed tabs.
                    this.cache[slot].complete(result);
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }
    #[cfg(test)]
    pub(crate) fn kind(&self) -> Kind {
        self.kind
    }
    #[cfg(test)]
    fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_for(self.details_kind, window, cx);
    }
    fn toggle_for(&mut self, kind: Kind, window: &mut Window, cx: &mut Context<Self>) {
        if self.open && self.details_kind == kind {
            self.dismiss(window, cx);
        } else {
            let trigger = self.trigger_for(kind).clone();
            if !self.open {
                self.previous_focus = window.focused(cx).filter(|f| *f != trigger);
            }
            self.details_kind = kind;
            self.open = true;
            self.show_accounts = false;
            self.confirm_reset = None;
            if let Some(center) = crate::accounts::get(cx) {
                let key = self.key_for(kind, cx);
                center.update(cx, |center, cx| {
                    if center
                        .reset_actions
                        .get(&key)
                        .is_some_and(|a| !a.applying() && a.row.is_none())
                    {
                        center.reset_actions.remove(&key);
                    }
                    center.hide_emails();
                    center.load_provider(kind, false, cx);
                });
            }
            window.focus(&self.details, cx);
            cx.notify();
        }
    }
    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.open = false;
        self.confirm_reset = None;
        let fallback = self.trigger_for(self.details_kind).clone();
        window.focus(self.previous_focus.as_ref().unwrap_or(&fallback), cx);
        cx.notify();
    }
    fn login_view(&self, cx: &App) -> bool {
        let kind = self.details_kind;
        let key = self.key_for(kind, cx);
        let state = &self.cache[index(kind)];
        monocode_core::accounts::login_arguments(kind, monocode_core::accounts::AuthAction::SignIn)
            .is_some()
            && state.snapshot.as_ref().is_none_or(|s| s.windows.is_empty())
            && (state.error.as_ref().is_some_and(|e| e.unavailable)
                || crate::accounts::get(cx)
                    .is_some_and(|center| center.read(cx).has_footer_auth(kind, &key.profile.id)))
    }
    /// React's AccountSwitchRow, above the sign-in panel.
    fn account_row(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let kind = self.details_kind;
        let key = self.key_for(kind, cx);
        let label = crate::accounts::get(cx)
            .and_then(|center| {
                center
                    .read(cx)
                    .book
                    .list(kind)
                    .into_iter()
                    .find(|a| a.id == key.profile.id)
            })
            .map(|a| a.label)
            .unwrap_or_else(|| "Removed account".into());
        div()
            .id("usage-switch-account")
            .role(Role::Button)
            .aria_label(format!("Switch account from {label}"))
            .h_8()
            .px(px(10.))
            .rounded_lg()
            .bg(content(0.045))
            .border_1()
            .border_color(content(0.06))
            .hover(|s| s.bg(content(0.08)))
            .cursor_pointer()
            .flex()
            .items_center()
            .gap_2()
            .text_size(px(11.))
            .child(div().flex_1().min_w_0().truncate().child(label))
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(content(0.4))
                    .child("Switch"),
            )
            .child(icon("chevron-right", 12.).text_color(content(0.35)))
            .on_click(cx.listener(|this, _, window, cx| this.open_accounts(window, cx)))
            .into_any_element()
    }
    fn details(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let kind = self.details_kind;
        let key = self.key_for(kind, cx);
        let session = (kind == self.kind).then_some(self.session).flatten();
        if self.show_accounts
            && let Some(center) = crate::accounts::get(cx)
        {
            let selected = key.profile.id.clone();
            let project = self.project.clone();
            let weak = cx.weak_entity();
            let picker = center.update(cx, |center, cx| {
                center.picker(
                    kind,
                    &selected,
                    &project,
                    session,
                    move |window, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            this.show_accounts = false;
                            window.focus(&this.details, cx);
                            cx.notify();
                        });
                    },
                    cx,
                )
            });
            return div()
                .id("footer-account-picker")
                .role(Role::Dialog)
                .aria_label("Choose account")
                .track_focus(&self.details)
                .key_context("UsageDetails")
                .on_action(
                    cx.listener(|this, _: &DismissUsage, window, cx| this.dismiss(window, cx)),
                )
                .child(picker)
                .into_any_element();
        }

        if self.login_view(cx)
            && let Some(center) = crate::accounts::get(cx)
        {
            let panel = center.update(cx, |center, cx| {
                center.footer_auth_panel(kind, &key.profile.id, cx)
            });
            return div()
                .id("usage-details-content")
                .role(Role::Dialog)
                .aria_label(format!("{} usage details", kind.label()))
                .track_focus(&self.details)
                .key_context("UsageDetails")
                .on_action(
                    cx.listener(|this, _: &DismissUsage, window, cx| this.dismiss(window, cx)),
                )
                .flex()
                .flex_col()
                .when(monocode_core::accounts::supports_accounts(kind), |d| {
                    d.child(div().px(px(10.)).pt(px(10.)).child(self.account_row(cx)))
                })
                .child(panel)
                .into_any_element();
        }
        let state = &self.cache[index(kind)];
        let now = usage::now_ms();
        let updated = match &state.snapshot {
            Some(snapshot) if snapshot.updated_at > 0 => {
                match (now - snapshot.updated_at).max(0) / 60_000 {
                    0 => "Updated just now".to_owned(),
                    minutes @ 1..60 => format!("Updated {minutes}m ago"),
                    minutes => format!("Updated {}h ago", minutes / 60),
                }
            }
            _ => "Rate-limit details".into(),
        };
        let windows = state.snapshot.iter().flat_map(|s| &s.windows);
        let has_windows = state
            .snapshot
            .as_ref()
            .is_some_and(|s| !s.windows.is_empty());
        div()
            .id("usage-details-content")
            .role(Role::Dialog)
            .aria_label(format!("{} usage details", kind.label()))
            .track_focus(&self.details)
            .key_context("UsageDetails")
            .on_action(cx.listener(|this, _: &DismissUsage, window, cx| {
                this.dismiss(window, cx);
            }))
            .flex()
            .flex_col()
            .text_size(px(11.))
            .text_color(content(1.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .px_1()
                    .pt(px(2.))
                    .pb(px(10.))
                    .child(
                        div()
                            .size_7()
                            .flex_shrink_0()
                            .rounded_lg()
                            .bg(content(0.06))
                            .border_1()
                            .border_color(content(0.07))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(provider_icon(kind.id(), 16.)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(format!("{} usage", kind.label())),
                            )
                            .child(
                                div()
                                    .mt(px(2.))
                                    .text_size(px(10.))
                                    .line_height(px(16.))
                                    .text_color(content(0.4))
                                    .child(updated),
                            )
                            .when(monocode_core::accounts::supports_accounts(kind), |d| {
                                d.child(self.account_line(cx))
                            }),
                    )
                    .when(state.fetching, |d| {
                        d.child(
                            div()
                                .mt(px(2.))
                                .flex()
                                .items_center()
                                .gap_1()
                                .text_size(px(10.))
                                .text_color(content(0.4))
                                .child(icon("refresh", 10.).text_color(content(0.4)))
                                .child("Updating"),
                        )
                    }),
            )
            .when(
                state.error.is_some() && (has_windows || self.has_banked_resets()),
                |d| {
                    d.child(
                        div()
                            .mb_2()
                            .rounded_lg()
                            .px(px(10.))
                            .py_2()
                            .bg(gpui::rgba(0xfbbf241a))
                            .text_size(px(10.))
                            .line_height(px(16.))
                            .text_color(rgb(0xfcd34d))
                            .child("Couldn’t refresh. Showing the last available snapshot."),
                    )
                },
            )
            .when(!has_windows && !self.has_banked_resets(), |d| {
                let loading = state.fetching || (state.snapshot.is_none() && state.error.is_none());
                d.child(
                    div()
                        .rounded_lg()
                        .bg(content(0.04))
                        .border_1()
                        .border_color(content(0.06))
                        .px_3()
                        .py_4()
                        .flex()
                        .flex_col()
                        .items_center()
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(content(0.65))
                                .child(if loading {
                                    "Loading usage…"
                                } else if state.error.as_ref().is_some_and(|e| e.unavailable) {
                                    "Not connected"
                                } else {
                                    "Usage unavailable"
                                }),
                        )
                        .children(state.error.as_ref().map(|error| {
                            div()
                                .mt_1()
                                .max_w(px(240.))
                                .text_center()
                                .text_size(px(10.))
                                .line_height(px(16.))
                                .text_color(content(0.4))
                                .child(error.message.clone())
                        })),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .children(windows.map(|w| self.window_card(w, now))),
            )
            .children(
                state
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.extra_usage_balance)
                    .map(|balance| {
                        div()
                            .mt(px(6.))
                            .rounded_lg()
                            .bg(content(0.045))
                            .border_1()
                            .border_color(content(0.06))
                            .px_3()
                            .py(px(10.))
                            .flex()
                            .justify_between()
                            .items_baseline()
                            .gap_3()
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(content(0.65))
                                    .child("Extra usage balance"),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(format!("${balance:.2}")),
                            )
                    }),
            )
            .children(self.suggestion(cx))
            .children(self.banked_resets(cx))
            .into_any()
    }
    /// The active account under the title: "Default account Pro · email ›".
    fn account_line(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let kind = self.details_kind;
        let id = self.key_for(kind, cx).profile.id;
        let center = crate::accounts::get(cx);
        let label = center
            .as_ref()
            .and_then(|center| {
                center
                    .read(cx)
                    .book
                    .list(kind)
                    .into_iter()
                    .find(|a| a.id == id)
            })
            .map(|a| a.label)
            .unwrap_or_else(|| "Removed account".into());
        let subtitle = center
            .and_then(|center| center.update(cx, |center, cx| center.subtitle(kind, &id, cx)));
        div()
            .id("usage-switch-account")
            .role(Role::Button)
            .aria_label(format!("Switch {} account", kind.label()))
            .mt_1()
            .ml(px(-4.))
            .max_w_full()
            .px_1()
            .py(px(2.))
            .rounded_sm()
            .flex()
            .items_center()
            .gap_1()
            .text_size(px(10.))
            .text_color(content(0.55))
            .cursor_pointer()
            .hover(|s| s.bg(content(0.1)))
            .child(
                div()
                    .max_w(gpui::relative(0.6))
                    .flex_shrink_0()
                    .truncate()
                    .child(label),
            )
            .children(subtitle.map(|d| d.text_color(content(0.35))))
            .child(icon("chevron-right", 10.).text_color(content(0.55)))
            .on_click(cx.listener(|this, _, window, cx| this.open_accounts(window, cx)))
            .into_any_element()
    }
    fn open_accounts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_reset = None;
        self.show_accounts = true;
        let kind = self.details_kind;
        let id = self.key_for(kind, cx).profile.id;
        let session = (kind == self.kind).then_some(self.session).flatten();
        if let Some(center) = crate::accounts::get(cx) {
            center.update(cx, |center, cx| {
                center.open_picker(kind, &id, &self.project, session, window, cx)
            });
        }
        cx.notify();
    }
    fn window_card(&self, w: &usage::Window, now: i64) -> gpui::Div {
        let used = w.used_percent.clamp(0., 100.);
        let shown = self.display_percent(used);
        let label = match w.minutes {
            1_440 => "Daily limit",
            10_080 => "Weekly limit",
            43_200 => "Monthly limit",
            _ => "5-hour limit",
        };
        div()
            .rounded_lg()
            .bg(content(0.045))
            .border_1()
            .border_color(content(0.06))
            .px_3()
            .py(px(10.))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_baseline()
                    .gap_3()
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(content(0.65))
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .font_weight(FontWeight::MEDIUM)
                            .child(format!(
                                "{shown:.0}% {}",
                                if self.show_remaining {
                                    "remaining"
                                } else {
                                    "used"
                                }
                            )),
                    ),
            )
            .child(
                div()
                    .mt_2()
                    .h(px(6.))
                    .w_full()
                    .rounded_full()
                    .bg(content(0.1))
                    .overflow_hidden()
                    .child(
                        div()
                            .h_full()
                            .w(gpui::relative(shown as f32 / 100.))
                            .rounded_full()
                            .bg(usage_color(used)),
                    ),
            )
            .child(
                div()
                    .mt(px(6.))
                    .flex()
                    .justify_between()
                    .gap_3()
                    .text_size(px(10.))
                    .line_height(px(16.))
                    .text_color(content(0.4))
                    .child(format!(
                        "{:.0}% {}",
                        100. - shown,
                        if self.show_remaining {
                            "used"
                        } else {
                            "remaining"
                        }
                    ))
                    .child(div().truncate().child(w.reset_label(now))),
            )
    }
    fn suggestion(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let center = crate::accounts::get(cx)?;
        let kind = self.details_kind;
        let id = self.key_for(kind, cx).profile.id;
        let account = center.read(cx).suggestion(kind, &id)?;
        let project = self.project.clone();
        let session = (kind == self.kind).then_some(self.session).flatten();
        let (status, exhausted) = {
            let center = center.read(cx);
            (
                center.status(kind, &account.id),
                center.status(kind, &id).tone == crate::accounts::Tone::Exhausted,
            )
        };
        Some(
            div()
                .mt_2()
                .rounded_lg()
                .bg(content(0.045))
                .border_1()
                .border_color(content(0.06))
                .px_3()
                .py(px(10.))
                .flex()
                .items_center()
                .gap(px(10.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(10.))
                                .line_height(px(16.))
                                .text_color(content(0.45))
                                .child(format!(
                                    "{} · switch to",
                                    if exhausted {
                                        "Out of usage"
                                    } else {
                                        "Running low"
                                    }
                                )),
                        )
                        .child(
                            div()
                                .mt(px(2.))
                                .flex()
                                .items_center()
                                .gap_2()
                                .min_w_0()
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(account.label.clone()),
                                )
                                .child(
                                    crate::accounts::ui::status_label(&status).text_size(px(10.)),
                                ),
                        ),
                )
                .child(
                    crate::settings::widgets::primary_button(
                        "switch-suggested-account",
                        "Switch account",
                        false,
                    )
                    .h_7()
                    .flex_shrink_0()
                    .px(px(10.))
                    .rounded_md()
                    .text_size(px(11.))
                    .child("Switch")
                    .on_click(cx.listener(move |_, _, _, cx| {
                        center.update(cx, |center, cx| {
                            center.choose(kind, account.id.clone(), project.clone(), session, cx)
                        });
                    })),
                )
                .into_any_element(),
        )
    }
    fn chip(
        &self,
        kind: Kind,
        primary: bool,
        now: i64,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let state = &self.cache[index(kind)];
        let label = if let Some(snapshot) = &state.snapshot
            && !snapshot.windows.is_empty()
        {
            snapshot
                .windows
                .iter()
                .map(|w| {
                    format!(
                        "{:.0}% {}",
                        self.display_percent(w.used_percent),
                        w.countdown(now)
                    )
                })
                .collect::<Vec<_>>()
                .join(" · ")
        } else if state.fetching || (state.snapshot.is_none() && state.error.is_none()) {
            "···".into()
        } else if state.error.as_ref().is_some_and(|e| e.unavailable) {
            "not connected".into()
        } else {
            "usage unavailable".into()
        };
        let description = state
            .snapshot
            .as_ref()
            .filter(|s| !s.windows.is_empty())
            .map(|s| {
                s.windows
                    .iter()
                    .map(|w| {
                        format!(
                            "{} · {}",
                            self.percent_label(w.used_percent),
                            w.reset_label(now)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" · ")
            })
            .or_else(|| state.error.as_ref().map(|e| e.message.clone()))
            .unwrap_or_else(|| "Loading usage".into());
        let tightest = state
            .snapshot
            .as_ref()
            .and_then(|s| s.windows.iter().map(|w| w.used_percent).reduce(f64::max));
        let classic = self.classic;
        let id: gpui::SharedString = if primary {
            "usage-provider-chip".into()
        } else {
            format!("usage-provider-chip-{}", kind.id()).into()
        };
        let trigger = self.trigger_for(kind).clone();
        let anchor = self.anchor_for(kind).probe();
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(format!("{} usage details", kind.label()))
            .aria_description(description)
            .track_focus(&trigger)
            .key_context("UsageChip")
            .tab_index(0)
            .relative()
            .h(px(20.))
            .map(|d| {
                if classic {
                    d.mx(px(-4.)).px(px(4.)).rounded_md()
                } else {
                    d.px(px(8.)).rounded_full()
                }
            })
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(6.))
            .cursor_pointer()
            .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
            .child(anchor)
            .child(provider_icon(kind.id(), 12.))
            .when_some(tightest, |d, percent| {
                d.child(
                    div()
                        .w(px(32.))
                        .h(px(4.))
                        .rounded_full()
                        .bg(content(0.1))
                        .overflow_hidden()
                        .child(
                            div()
                                .h_full()
                                .w(gpui::relative(percent as f32 / 100.))
                                .rounded_full()
                                .bg(usage_color(percent)),
                        ),
                )
            })
            .child(label)
            .on_click(cx.listener(move |this, _, window, cx| this.toggle_for(kind, window, cx)))
            .on_action(cx.listener(move |this, _: &ToggleUsage, window, cx| {
                this.toggle_for(kind, window, cx)
            }))
            .into_any_element()
    }
}
use crate::accounts::ui::bar_color as usage_color;
impl Render for UsageFooter {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = usage::now_ms();
        let providers = self.visible_providers(cx);
        let fetching = providers.iter().any(|&k| self.cache[index(k)].fetching);
        let weak = cx.weak_entity();
        let classic = self.classic;
        let primary_kind = providers
            .iter()
            .copied()
            .find(|&k| k == self.kind)
            .or_else(|| providers.first().copied());
        div()
            .id("usage-footer")
            .role(Role::Group)
            .aria_label(if providers.is_empty() {
                "Session"
            } else {
                "Provider usage"
            })
            .h(px(if classic { 28. } else { 30. }))
            .w_full()
            .flex_shrink_0()
            .when(!classic, |d| d.pb(px(2.)))
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(11.))
            .text_color(content(0.55))
            .when(providers.is_empty(), |d| {
                d.child(
                    div()
                        .h(px(20.))
                        .map(|d| {
                            if classic {
                                d.mx(px(-4.)).px(px(4.))
                            } else {
                                d.px(px(8.))
                            }
                        })
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(provider_icon(self.kind.id(), 12.))
                        .child(self.kind.label()),
                )
            })
            .children(
                providers
                    .iter()
                    .copied()
                    .map(|kind| self.chip(kind, Some(kind) == primary_kind, now, cx)),
            )
            .when(!providers.is_empty(), |d| {
                d.child(
                    div()
                        .id("refresh-usage")
                        .role(Role::Button)
                        .aria_label("Refresh usage")
                        .track_focus(&self.refresh_focus)
                        .tab_index(0)
                        .key_context("UsageRefresh")
                        .map(|d| {
                            if classic {
                                d.size(px(18.)).rounded_md()
                            } else {
                                d.size(px(20.)).ml(px(-4.)).rounded_full()
                            }
                        })
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .opacity(if fetching { 0.5 } else { 1. })
                        .when(!fetching, |d| d.hover(|s| s.bg(content(0.1))))
                        .child(icon("refresh", 12.))
                        .on_click(cx.listener(|this, _, _, cx| this.refresh(true, cx)))
                        .on_action(
                            cx.listener(|this, _: &RefreshUsage, _, cx| this.refresh(true, cx)),
                        ),
                )
            })
            .when(self.open, |d| {
                let anchor = self.anchor_for(self.details_kind).clone();
                d.child(
                    popover("usage-details", Anchor::Slot(anchor))
                        .side(Side::Top)
                        .gap(7.)
                        .width(if self.show_accounts { 340. } else { 300. })
                        .max_height(460.)
                        .padding(if !self.show_accounts && self.login_view(cx) {
                            0.
                        } else {
                            10.
                        })
                        .on_dismiss(move |window, cx| {
                            let _ = weak.update(cx, |this, cx| this.dismiss(window, cx));
                        })
                        .child(self.details(cx)),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[gpui::test]
    fn details_open_with_keyboard_escape_restores_focus_and_provider_caches_stay_separate(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(bind);
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| UsageFooter::new(Kind::Codex, cx))
            })
            .unwrap()
        });
        window
            .update(cx, |footer, w, cx| {
                footer.cache[0].complete(Ok(Snapshot {
                    windows: vec![usage::Window {
                        used_percent: 28.,
                        minutes: 300,
                        resets_at: None,
                    }],
                    updated_at: usage::now_ms(),
                    ..Default::default()
                }));
                w.focus(&footer.trigger, cx);
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        window
            .update(cx, |footer, w, cx| {
                assert!(footer.open);
                assert!(footer.details.is_focused(w));
                cx.notify();
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();
        window
            .update(cx, |footer, w, cx| {
                assert!(!footer.open);
                assert!(footer.trigger.is_focused(w));
                footer.provider(Kind::Claude, cx);
                assert!(footer.cache[index(footer.kind)].snapshot.is_none());
                // A late Codex response belongs to Codex, while Claude stays in view.
                footer.cache[0].complete(Ok(Snapshot {
                    windows: vec![usage::Window {
                        used_percent: 42.,
                        minutes: 300,
                        resets_at: None,
                    }],
                    updated_at: usage::now_ms(),
                    ..Default::default()
                }));
                assert!(footer.cache[index(footer.kind)].snapshot.is_none());
                footer.provider(Kind::Codex, cx);
                assert_eq!(
                    footer.cache[0].snapshot.as_ref().unwrap().windows[0].used_percent,
                    42.
                );
            })
            .unwrap();
    }
    #[test]
    fn failed_refresh_keeps_snapshot_but_sign_out_clears_it() {
        let mut state = CachedUsage {
            snapshot: Some(Snapshot::default()),
            fetching: true,
            ..Default::default()
        };
        state.complete(Err(FetchError {
            message: "Offline".into(),
            unavailable: false,
        }));
        assert!(state.snapshot.is_some());
        assert!(!state.fetching);
        state.complete(Err(FetchError {
            message: "Signed out".into(),
            unavailable: true,
        }));
        assert!(state.snapshot.is_none());
        state.complete(Ok(Snapshot::default()));
        assert!(state.error.is_none());
    }
    #[test]
    fn footer_usage_providers_places_active_plan_first_and_skips_hidden_or_non_plan_harnesses() {
        let installed = |kind: Kind| matches!(kind, Kind::Claude | Kind::Codex | Kind::Devin);
        assert_eq!(
            footer_usage_providers(Some(Kind::Devin), installed, |_| false),
            vec![Kind::Devin, Kind::Claude, Kind::Codex]
        );
        assert_eq!(
            footer_usage_providers(Some(Kind::Droid), installed, |_| false),
            vec![Kind::Claude, Kind::Codex, Kind::Devin]
        );
        assert_eq!(
            footer_usage_providers(Some(Kind::Droid), installed, |k| k == Kind::Codex),
            vec![Kind::Claude, Kind::Devin]
        );
    }
}
