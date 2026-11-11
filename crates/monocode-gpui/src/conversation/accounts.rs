use super::*;
use gpui::App;
use monocode_core::{
    accounts::Profile,
    usage::{self, Limit},
};
pub(crate) struct UsageResumed;
impl EventEmitter<UsageResumed> for Conversation {}

/// A limit reset for this conversation's account: whether one is being
/// used, the one last tried by itself (never tried again by itself), and
/// whether an offer shows.
#[derive(Default)]
pub(crate) struct LimitReset {
    pending: bool,
    tried: Option<String>,
    offering: bool,
}

impl Conversation {
    fn usage_key(&self) -> crate::accounts::Key {
        crate::accounts::Key::new(
            Kind::from_id(&self.summary.provider),
            self.settings.account.clone(),
            self.account_model(),
        )
    }

    /// Repaints when a reset starts or stops being offered near a limit.
    pub(super) fn watch_reset_offer(&mut self, cx: &mut Context<Self>) {
        let offering = !self.is_remote()
            && crate::accounts::get(cx)
                .is_some_and(|center| center.read(cx).reset_offer(&self.usage_key(), cx).is_some());
        if offering != self.limit_reset.offering {
            self.limit_reset.offering = offering;
            cx.notify();
        }
    }

    /// At a limit: uses the planned reset when the user said yes to it near
    /// the limit or chose Automatic, and goes on once it went through.
    fn try_limit_reset(&mut self, cx: &mut Context<Self>) {
        let Some(center) = crate::accounts::get(cx) else {
            return;
        };
        let key = self.usage_key();
        if self.limit_reset.pending {
            let result = center
                .read(cx)
                .reset_actions
                .get(&key)
                .and_then(|action| action.result.clone());
            match result {
                None => {}
                Some(Ok(outcome)) if outcome == "reset" || outcome == "nothingToReset" => {
                    self.limit_reset.pending = false;
                    self.resume_usage(cx);
                }
                Some(_) => {
                    self.limit_reset.pending = false;
                    cx.notify();
                }
            }
            return;
        }
        if self.busy || self.is_remote() {
            return;
        }
        let mode = crate::accounts::resets::mode(cx);
        let Some(plan) = center.read(cx).reset_plan(&key, cx) else {
            return;
        };
        let approved = center.read(cx).reset_approved(&key, &plan);
        if !monocode_core::usage::resets::use_now(mode, &plan, approved)
            || self.limit_reset.tried.as_deref() == Some(plan.credit.id.as_str())
        {
            return;
        }
        self.use_limit_reset(plan, cx);
    }

    fn use_limit_reset(
        &mut self,
        plan: monocode_core::usage::resets::Plan,
        cx: &mut Context<Self>,
    ) {
        let Some(center) = crate::accounts::get(cx) else {
            return;
        };
        let key = self.usage_key();
        self.limit_reset.tried = Some(plan.credit.id.clone());
        let started = center.update(cx, |center, cx| center.use_planned_reset(key, &plan, cx));
        if started {
            self.limit_reset.pending = true;
        }
        cx.notify();
    }

    /// Near a limit, with a reset for it and Ask as the setting: one line
    /// above the composer offering to use it when the limit is reached.
    pub(super) fn reset_offer_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.is_remote() || self.settings.usage_limit.is_some() {
            return None;
        }
        let center = crate::accounts::get(cx)?;
        let key = self.usage_key();
        let plan = center.read(cx).reset_offer(&key, cx)?;
        let left = center
            .read(cx)
            .usage
            .get(&key)
            .and_then(|state| state.snapshot.as_ref())
            .and_then(|snapshot| snapshot.reset_credits.as_ref())
            .map_or(1, |resets| {
                monocode_core::usage::resets::left_of_kind(resets, &plan.credit)
            });
        let kind = Kind::from_id(&self.summary.provider);
        let text = format!(
            "{} is at {:.0}% of its {}. Use your {} ({left} left) when you reach it?",
            kind.label(),
            plan.percent.floor(),
            monocode_core::usage::resets::limit_label(&plan.limit),
            monocode_core::usage::resets::reset_label(&plan.credit),
        );
        let answer = move |yes: bool| {
            let key = key.clone();
            let plan = plan.clone();
            move |this: &mut Self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>| {
                if let Some(center) = crate::accounts::get(cx) {
                    center.update(cx, |center, cx| {
                        center.answer_reset(key.clone(), &plan, yes, cx)
                    });
                }
                this.watch_reset_offer(cx);
            }
        };
        Some(
            div()
                .id("limit-reset-offer")
                .debug_selector(|| "limit-reset-offer".into())
                .role(Role::Status)
                .px(px(8.))
                .child(
                    div()
                        .min_h(px(32.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded_t(px(10.))
                        .border_1()
                        .border_b_0()
                        .border_color(content(0.1))
                        .bg(content(0.04))
                        .px(px(8.))
                        .py(px(4.))
                        .child(icon("gauge", 14.).text_color(gpui::rgb(AMBER_400)))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(content(0.7))
                                .child(text),
                        )
                        .child(
                            usage_button(
                                "limit-reset-yes",
                                "Use the reset at the limit",
                                true,
                                false,
                                false,
                            )
                            .child("Use at the limit")
                            .on_click(cx.listener(answer(true))),
                        )
                        .child(
                            usage_button("limit-reset-no", "Not now", false, false, false)
                                .child("Not now")
                                .on_click(cx.listener(answer(false))),
                        ),
                )
                .into_any_element(),
        )
    }

    /// Update notifications refresh the account-specific picker after a CLI update.
    pub(crate) fn refresh_models_after_harness_update(&mut self, cx: &mut Context<Self>) {
        monocode_core::catalog::invalidate(
            Kind::from_id(&self.summary.provider),
            &self.settings.account,
        );
        self.refresh_catalog(cx);
        cx.notify();
    }

    pub(super) fn auth_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let kind = Kind::from_id(&self.summary.provider);
        let action = monocode_core::accounts::AuthAction::SignIn;
        if self.busy || monocode_core::accounts::login_arguments(kind, action).is_none() {
            return None;
        }
        let status = self.status.to_ascii_lowercase();
        if ![
            "not signed in",
            "not authenticated",
            "authentication required",
            "please log in",
            "please login",
            "no longer signed in",
        ]
        .iter()
        .any(|s| status.contains(s))
        {
            return None;
        }
        let id = self.settings.account.id.clone();
        Some(
            div()
                .mx_2()
                .mb_2()
                .p_3()
                .rounded_lg()
                .bg(content(0.04))
                .flex()
                .items_center()
                .gap_2()
                .child(provider_icon(kind.id(), 20.))
                .child(
                    div()
                        .flex_1()
                        .child(format!("Sign in to {} to continue", kind.label())),
                )
                .child(
                    crate::settings::widgets::secondary_button(
                        "conversation-sign-in",
                        "Sign in",
                        true,
                        false,
                    )
                    .child("Sign in")
                    .on_click(cx.listener(move |_, _, _, cx| {
                        if let Some(center) = crate::accounts::get(cx) {
                            center.update(cx, |center, cx| {
                                center.begin_auth(kind, &id, action, None, cx)
                            });
                        }
                    })),
                )
                .into_any_element(),
        )
    }
    pub(crate) fn attach_account(&mut self, cx: &App) {
        if self.is_remote() {
            self.settings.account = Default::default();
            return;
        }
        if self.settings.account.id == monocode_core::accounts::DEFAULT {
            self.settings.account.root = None;
            return;
        }
        if let Some(center) = crate::accounts::get(cx) {
            self.settings.account.root = Some(center.read(cx).root.clone());
        }
    }
    pub(crate) fn account_model(&self) -> Option<&str> {
        self.model.as_deref().or(self.resolved_model.as_deref())
    }
    pub(crate) fn set_blank_account(&mut self, profile: Profile, cx: &mut Context<Self>) {
        if self.busy || self.summary.has_messages {
            return;
        }
        self.settings.account = profile;
        self.catalog.clear();
        self.resolved_model = None;
        self.refresh_catalog(cx);
        self.save_settings(cx);
        cx.emit(SessionChanged);
        cx.notify();
    }
    pub(super) fn detect_usage_limit(&mut self, cx: &mut Context<Self>) {
        if let Some(limit) = self
            .settings
            .usage_limit
            .clone()
            .or_else(|| Limit::from_error(&self.status))
        {
            if let Some(pending) = &mut self.settings.pending_provider_switch {
                pending.skip_outgoing_recap = true;
            }
            self.settings.usage_limit = Some(limit);
            self.save_settings(cx);
            self.follow_ups.state.paused = true;
            self.save_follow_ups(cx);
            let kind = Kind::from_id(&self.summary.provider);
            if let Some(center) = crate::accounts::get(cx) {
                let key = crate::accounts::Key::new(
                    kind,
                    self.settings.account.clone(),
                    self.account_model(),
                );
                center.update(cx, |center, cx| center.refresh(key, true, cx));
            }
            self.tick_usage(cx);
        }
    }
    /// Account recovery also works for saved conversations from before native
    /// limit metadata was recorded, and after dismissing the banner. Inspect
    /// only the final error/notice: an older limit must not affect a later turn.
    pub(crate) fn account_recovery_limit(&self, cx: &App) -> Option<Limit> {
        self.settings
            .usage_limit
            .clone()
            .or_else(|| Limit::from_error(&self.status))
            .or_else(|| {
                let rows = self.transcript.read(cx).source_rows();
                let last = rows.last()?;
                if !matches!(last.role.as_str(), "error" | "system") {
                    return None;
                }
                let start = rows.iter().rposition(|row| row.first).unwrap_or(0);
                let text: String = rows[start..].iter().map(|row| row.text.as_str()).collect();
                Limit::from_error(&text)
            })
    }
    pub(super) fn tick_usage(&mut self, cx: &mut Context<Self>) {
        let Some(limit) = self.settings.usage_limit.clone() else {
            return;
        };
        self.try_limit_reset(cx);
        if self.settings.usage_limit.is_none() {
            return;
        }
        if limit.resets_at.is_none() {
            let kind = Kind::from_id(&self.summary.provider);
            let key = crate::accounts::Key::new(
                kind,
                self.settings.account.clone(),
                self.account_model(),
            );
            if let Some(center) = crate::accounts::get(cx) {
                center.update(cx, |center, cx| center.refresh(key.clone(), false, cx));
            }
            let reset = crate::accounts::get(cx).and_then(|center| {
                center
                    .read(cx)
                    .usage
                    .get(&key)
                    .and_then(|state| state.snapshot.as_ref())
                    .and_then(|s| s.exhausted_reset())
            });
            if let Some(at) = reset {
                self.settings.usage_limit.as_mut().unwrap().resets_at = Some(at);
                self.save_settings(cx);
            }
        }
        if self
            .settings
            .usage_limit
            .as_ref()
            .is_some_and(|l| l.resume_due(usage::now_ms(), self.busy))
        {
            self.resume_usage(cx);
        }
        cx.notify();
    }
    pub(crate) fn resume_usage(&mut self, cx: &mut Context<Self>) {
        if self.needs_history_restore() {
            self.defer_history_action(|this, cx| this.resume_usage(cx), cx);
            return;
        }
        if self.busy
            || self.stopping
            || self.account_switching
            || self.settings.usage_limit.take().is_none()
        {
            return;
        }
        self.status = "Ready".into();
        self.save_settings(cx);
        if self.settings.mono.is_some() {
            cx.emit(UsageResumed);
            cx.emit(SessionChanged);
            cx.notify();
            return;
        }
        self.launch_prompt("Continue from where you left off.".into(), cx);
        cx.emit(SessionChanged);
        cx.notify();
    }
    pub(super) fn recover_usage_after_model_change(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.stopping || self.settings.usage_limit.take().is_none() {
            return;
        }
        self.status = "Ready".into();
        self.follow_ups.state.paused = false;
        for message in &mut self.follow_ups.state.messages {
            message.error = None;
        }
        self.save_settings(cx);
        self.save_follow_ups(cx);
        if self.settings.mono.is_some() {
            cx.emit(UsageResumed);
        }
        cx.emit(SessionChanged);
        cx.notify();
    }
    /// Moves this conversation to another account in place. A limited
    /// session resumes on the new account; an idle one just waits for the
    /// next message there.
    pub(crate) fn switch_account(&mut self, profile: Profile, cx: &mut Context<Self>) {
        if self.editing_last_turn.pending() {
            return;
        }
        if self.needs_history_restore() {
            self.defer_history_action(move |this, cx| this.switch_account(profile, cx), cx);
            return;
        }
        if self.busy
            || self.stopping
            || self.account_switching
            || self.settings.account.id == profile.id
        {
            return;
        }
        let limit = self.account_recovery_limit(cx);
        let limited = limit.is_some();
        if limited {
            self.settings.usage_limit = limit;
        }
        self.attach_account(cx);
        let from = self.settings.account.clone();
        let thread = self.summary.provider_id.clone();
        let expected = thread.clone();
        let kind = Kind::from_id(&self.summary.provider);
        let can_transfer = !(kind == Kind::Codex && self.settings.mono.is_some());
        let to = profile.clone();
        self.account_switching = true;
        let work = cx.background_executor().spawn(async move {
            thread
                .as_ref()
                .is_some_and(|id| can_transfer && from.transfer(&to, kind, id).is_ok())
        });
        let old = self.settings.account.id.clone();
        cx.spawn(async move |this, cx| {
            let carried = work.await;
            let _ = this.update(cx, |this, cx| {
                this.account_switching = false;
                if this.busy
                    || (limited && this.settings.usage_limit.is_none())
                    || this.settings.account.id != old
                    || this.summary.provider_id != expected
                    || crate::accounts::get(cx)
                        .is_some_and(|center| !center.read(cx).book.exists(kind, &profile.id))
                {
                    return;
                }
                if !carried {
                    if this.settings.pending_provider_switch.is_none() {
                        this.settings.pending_provider_switch =
                            Some(monocode_core::agent::ProviderSwitch {
                                from: this.summary.provider.clone(),
                                model_label: this.model_label().to_string(),
                                cwd: this.execution_cwd().to_owned(),
                                model: this.model.clone(),
                                model_settings: this.settings.model_settings.clone(),
                                effort: this.settings.effort.clone(),
                                fast: this.settings.fast,
                                service_tier: this.settings.service_tier.clone(),
                                thinking: this.settings.thinking,
                                skip_outgoing_recap: limited,
                                account: this.settings.account.clone(),
                                provider_id: this.summary.provider_id.clone(),
                            });
                    }
                    this.summary.provider_id = None;
                    let store = this.store.clone();
                    let id = this.summary.id;
                    let expected = expected.clone();
                    cx.background_executor()
                        .spawn(async move {
                            let _ = locked(&store)
                                .and_then(|s| s.reset_provider_id_if(id, expected.as_deref()));
                        })
                        .detach();
                    this.settings.handoff = Some(monocode_core::agent::Handoff {
                        from: this.summary.provider.clone(),
                        brief: monocode_core::response::handoff(
                            this.transcript.read(cx).source_rows(),
                        ),
                    });
                } else if let Some(id) = &this.summary.provider_id {
                    monocode_core::provider::forget_session(
                        kind,
                        id,
                        &old,
                        Path::new(this.execution_cwd()),
                    );
                }
                this.settings.account = profile;
                this.catalog.clear();
                this.refresh_catalog(cx);
                this.save_settings(cx);
                if limited {
                    this.resume_usage(cx);
                } else {
                    cx.emit(SessionChanged);
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }
    pub(crate) fn account_unavailable(&mut self, cx: &mut Context<Self>) {
        self.stop_if_busy(cx);
        self.follow_ups.state.paused = true;
        self.follow_ups.requested = None;
        self.save_follow_ups(cx);
        self.catalog_epoch += 1;
        self.catalog.clear();
        self.status =
            "This provider account is no longer signed in. Choose another account to continue."
                .into();
        cx.notify();
    }
    pub(crate) fn account_authenticated(&mut self, cx: &mut Context<Self>) {
        self.attach_account(cx);
        self.refresh_catalog(cx);
        cx.notify();
    }
    pub(super) fn usage_notice(&self, mono: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        let limit = self.settings.usage_limit.as_ref()?;
        let now = usage::now_ms();
        let waiting = limit.waiting(now);
        let armed = limit.resume_at_reset;
        let switching = self.account_switching;
        let kind = Kind::from_id(&self.summary.provider);
        let reset = usage_reset_label(limit, now);
        let mut controls = div()
            .flex()
            .when(!mono, |d| d.flex_shrink_0())
            .items_center()
            .gap(px(if mono { 12. } else { 8. }))
            .when(mono, |d| d.flex_wrap().gap_y(px(6.)))
            .when(mono, |d| {
                d.child(
                    usage_button(
                        "usage-model-picker",
                        "Choose another model",
                        false,
                        switching,
                        true,
                    )
                    .h(px(26.))
                    .gap(px(4.))
                    .text_size(px(11.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(content(1.))
                    .bg(content(0.1))
                    .child("Choose another model")
                    .child(icon("chevron-down", 11.).text_color(content(0.5)))
                    .when(!switching, |d| {
                        d.on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_menu(ComposerMenu::Model, window, cx)
                        }))
                    }),
                )
            });
        if monocode_core::accounts::supports_profiles(kind)
            && crate::accounts::get(cx).is_some_and(|center| {
                center
                    .read(cx)
                    .book
                    .list(kind)
                    .iter()
                    .any(|a| a.id != self.settings.account.id)
            })
        {
            controls = controls.child(
                usage_button(
                    "usage-account-picker",
                    "Choose another account",
                    false,
                    switching,
                    mono,
                )
                .relative()
                .gap(px(4.))
                .when(mono, |d| {
                    d.h(px(26.))
                        .text_size(px(11.))
                        .text_color(content(1.))
                        .bg(content(0.1))
                })
                .child(self.account_anchor.probe())
                .child(if switching {
                    "Switching…"
                } else if mono {
                    "Choose another account"
                } else {
                    "Switch account"
                })
                .child(icon("chevron-down", 12.).text_color(content(0.5)))
                .when(!switching, |d| {
                    d.on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_menu(ComposerMenu::Account, window, cx)
                    }))
                }),
            );
        }
        // A reset for the limit reached: one click uses it and goes on.
        let reset_plan = crate::accounts::get(cx).and_then(|center| {
            center
                .read(cx)
                .reset_plan(&self.usage_key(), cx)
                .filter(|plan| plan.reached())
        });
        let reset_failed = crate::accounts::get(cx).and_then(|center| {
            center
                .read(cx)
                .reset_actions
                .get(&self.usage_key())
                .filter(|action| action.result.as_ref().is_some_and(Result::is_err))
                .and_then(|action| action.message().map(str::to_owned))
        });
        if let Some(plan) = reset_plan.filter(|_| !self.is_remote()) {
            let pending = self.limit_reset.pending;
            let label = if pending {
                "Using reset…".to_owned()
            } else {
                let name = monocode_core::usage::resets::reset_label(&plan.credit);
                let mut chars = name.chars();
                let name: String = chars
                    .next()
                    .map(|first| first.to_uppercase().chain(chars).collect())
                    .unwrap_or_default();
                format!("Use {name}")
            };
            controls = controls.child(
                usage_button(
                    "use-limit-reset",
                    "Use a limit reset and continue",
                    true,
                    pending || switching,
                    mono,
                )
                .when(mono, |d| d.h(px(26.)).text_size(px(11.)).bg(content(0.1)))
                .child(icon("refresh", 13.).text_color(gpui::rgb(AMBER_400)))
                .child(label)
                .when(!pending && !switching, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(center) = crate::accounts::get(cx) {
                            let key = this.usage_key();
                            center
                                .update(cx, |center, cx| center.answer_reset(key, &plan, true, cx));
                        }
                        this.use_limit_reset(plan.clone(), cx);
                    }))
                }),
            );
        }
        let reset = reset_failed.or(reset);
        let resume = if waiting {
            if armed {
                "Resuming at reset"
            } else {
                "Resume at reset"
            }
        } else {
            "Resume"
        };
        controls = controls
            .child(
                usage_button(
                    "resume-usage-limit",
                    if waiting && armed {
                        "Cancel the automatic resume"
                    } else {
                        resume
                    },
                    waiting && armed,
                    switching,
                    false,
                )
                .when(mono && !(waiting && armed), |d| d.text_color(content(0.6)))
                .child(
                    icon(if waiting { "clock" } else { "play" }, 14.)
                        .text_color(if waiting && armed {
                            gpui::rgb(AMBER_400).into()
                        } else {
                            content(if mono { 0.6 } else { 0.55 })
                        })
                        .group_hover("resume-usage-limit", |s| s.text_color(content(1.))),
                )
                .child(resume)
                .when(!switching, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        if waiting {
                            if let Some(limit) = &this.settings.usage_limit {
                                let enabled = !limit.resume_at_reset;
                                this.set_usage_resume_at_reset(enabled, cx);
                            }
                        } else {
                            this.resume_usage(cx);
                        }
                    }))
                }),
            )
            .when(!mono, |controls| {
                controls.child(
                    usage_button(
                        "dismiss-usage-limit",
                        "Dismiss usage limit notice",
                        false,
                        false,
                        false,
                    )
                    .w(px(24.))
                    .px_0()
                    .tooltip(crate::theme::chrome_tip("Dismiss usage limit notice", None))
                    .child(
                        icon("close", 14.)
                            .text_color(content(0.55))
                            .group_hover("dismiss-usage-limit", |s| s.text_color(content(1.))),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.settings.usage_limit = None;
                        this.save_settings(cx);
                        cx.notify();
                    })),
                )
            });
        let heading = div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(8.))
            .whitespace_nowrap()
            .child(icon("gauge", 14.).text_color(gpui::rgb(AMBER_400)))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(content(0.85))
                    .child(if mono {
                        format!("{} usage limit reached", kind.label())
                    } else {
                        "Usage limit reached".into()
                    }),
            );
        let notice = div()
            .id("usage-limit-notice")
            .debug_selector(|| "usage-limit-notice".into())
            .role(Role::Status)
            .aria_label("Usage limit reached")
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(content(if mono { 0.6 } else { 0.55 }));
        Some(if mono {
            notice.mx(px(6.)).mb(px(8.)).rounded(px(8.)).border_1().border_color(tint(AMBER_400,0.2)).bg(tint(AMBER_400,0.06))
                .px(px(12.)).py(px(10.)).child(heading)
                .child(div().pl(px(22.))
                    .child(div().mt(px(4.)).text_size(px(12.)).line_height(px(19.5)).text_color(content(0.6))
                        .child("Your conversation is saved. Choose another model or account to keep going."))
                    .children(reset.map(|label| div().mt(px(4.)).text_color(content(0.45)).child(label)))
                    .child(controls.mt(px(8.))))
                .into_any_element()
        } else {
            notice
                .px(px(8.))
                .child(
                    div()
                        .id("usage-limit-banner")
                        .debug_selector(|| "usage-limit-banner".into())
                        .h(px(32.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded_t(px(10.))
                        .border_1()
                        .border_b_0()
                        .border_color(tint(AMBER_400, 0.25))
                        .bg(tint(AMBER_400, 0.1))
                        .px(px(8.))
                        .child(heading)
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .child(reset.unwrap_or_default()),
                        )
                        .child(controls),
                )
                .into_any_element()
        })
    }
}

fn usage_button(
    id: &'static str,
    label: &'static str,
    primary: bool,
    disabled: bool,
    raised: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .group(id)
        .role(Role::Button)
        .aria_label(label)
        .h(px(24.))
        .px(px(6.))
        .rounded_md()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .text_size(px(12.))
        .line_height(px(16.))
        .whitespace_nowrap()
        .text_color(if primary {
            gpui::rgb(AMBER_400).into()
        } else {
            content(0.55)
        })
        .when(disabled, |d| d.opacity(0.45))
        .when(!disabled, |d| {
            d.cursor_pointer().hover(move |s| {
                s.bg(content(if raised { 0.15 } else { 0.1 }))
                    .text_color(content(1.))
            })
        })
}

/// Shared reset wording for desktop and its paired phone.
pub(super) fn usage_reset_label(limit: &Limit, now: i64) -> Option<String> {
    limit.resets_at.map(|at| {
        if at <= now {
            return "Limit has reset".to_owned();
        }
        let when = chrono::DateTime::from_timestamp_millis(at)
            .map(|d| {
                let local = d.with_timezone(&chrono::Local);
                let today = chrono::Local::now().date_naive() == local.date_naive();
                local
                    .format(if today {
                        "%-I:%M %p"
                    } else {
                        "%b %-d, %-I:%M %p"
                    })
                    .to_string()
            })
            .unwrap_or_default();
        format!(
            "Resets {when} · in {}",
            usage::Window {
                used_percent: 100.,
                minutes: 300,
                resets_at: Some(at)
            }
            .countdown(now)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn account_recovery_uses_the_terminal_limit_without_reviving_an_older_failure(
        cx: &mut gpui::TestAppContext,
    ) {
        use monocode_core::transcript::Row;
        let store = Arc::new(std::sync::Mutex::new(
            monocode_core::store::Store::open(Path::new(":memory:")).unwrap(),
        ));
        let summary = locked(&store)
            .unwrap()
            .create("claude", "/fixture")
            .unwrap();
        let view = cx.new(|cx| Conversation::new(summary, store, cx));
        view.update(cx, |view, cx| {
            let row = |role: &str, text: &str| Row {
                role: role.into(),
                text: text.into(),
                first: true,
                sent_at: None,
            };
            for role in ["error", "system"] {
                view.transcript.update(cx, |v, cx| {
                    v.patch(
                        0,
                        vec![row("user", "Keep going"), row(role, "rate_limit")],
                        cx,
                    );
                });
                assert!(view.account_recovery_limit(cx).is_some());
            }
            view.transcript.update(cx, |v, cx| {
                v.patch(
                    2,
                    vec![row("user", "Try again"), row("assistant", "Completed")],
                    cx,
                );
            });
            assert!(view.account_recovery_limit(cx).is_none());
            view.transcript.update(cx, |v, cx| {
                v.patch(4, vec![row("system", "Network timeout")], cx);
            });
            assert!(view.account_recovery_limit(cx).is_none());
        });
    }

    #[gpui::test]
    fn claude_limit_code_opens_recovery_and_preserves_the_reported_reset(
        cx: &mut gpui::TestAppContext,
    ) {
        let store = Arc::new(std::sync::Mutex::new(
            monocode_core::store::Store::open(Path::new(":memory:")).unwrap(),
        ));
        let summary = locked(&store)
            .unwrap()
            .create("claude", "/fixture")
            .unwrap();
        let view = cx.new(|cx| Conversation::new(summary, store, cx));
        view.update(cx, |view, cx| {
            view.status = "rate_limit".into();
            view.detect_usage_limit(cx);
            assert!(view.settings.usage_limit.is_some());
            assert!(view.follow_ups.state.paused);
            let reported = Limit {
                resets_at: Some(usage::now_ms() + 3_600_000),
                resume_at_reset: false,
            };
            view.settings.usage_limit = Some(reported.clone());
            view.detect_usage_limit(cx);
            assert_eq!(view.settings.usage_limit, Some(reported));
        });
    }

    #[gpui::test]
    fn usage_banner_matches_the_composer_and_can_arm_cancel_and_dismiss(
        cx: &mut gpui::TestAppContext,
    ) {
        let store = Arc::new(std::sync::Mutex::new(
            monocode_core::store::Store::open(Path::new(":memory:")).unwrap(),
        ));
        let mut summary = locked(&store)
            .unwrap()
            .create("claude", "/fixture")
            .unwrap();
        summary.has_messages = true;
        let id = summary.id;
        let saved = store.clone();
        let window = cx.update(|cx| {
            crate::accounts::install(store.clone(), cx);
            crate::accounts::get(cx).unwrap().update(cx, |center, _| {
                center.book.add(Kind::Claude, "Other account").unwrap();
            });
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(summary, store, cx);
                    view.loaded = true;
                    view.status = "Claude Code reached its usage limit.".into();
                    view.settings.usage_limit = Some(Limit {
                        resets_at: Some(usage::now_ms() + 3_600_000),
                        resume_at_reset: false,
                    });
                    view
                })
            })
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        let banner = visual.debug_bounds("usage-limit-banner").unwrap();
        let composer = visual.debug_bounds("composer-box").unwrap();
        let resume = visual.debug_bounds("resume-usage-limit").unwrap();
        let account = visual.debug_bounds("usage-account-picker").unwrap();
        let dismiss = visual.debug_bounds("dismiss-usage-limit").unwrap();
        assert_eq!(banner.size.height, px(32.));
        assert_eq!(resume.size.height, px(24.));
        assert_eq!(dismiss.size, gpui::size(px(24.), px(24.)));
        assert_eq!(banner.bottom(), composer.top());
        assert_eq!(banner.left(), composer.left() + px(8.));
        assert_eq!(banner.right(), composer.right() - px(8.));
        assert_eq!(account.right() + px(8.), resume.left());
        assert_eq!(resume.right() + px(8.), dismiss.left());
        assert!(visual.debug_bounds("usage-model-picker").is_none());
        assert!(visual.debug_bounds("provider-status").is_none());

        visual.simulate_click(resume.center(), Default::default());
        assert!(
            window
                .update(visual, |v, _, _| v
                    .settings
                    .usage_limit
                    .as_ref()
                    .unwrap()
                    .resume_at_reset)
                .unwrap()
        );
        assert!(
            locked(&saved)
                .unwrap()
                .agent_settings(id)
                .unwrap()
                .unwrap()
                .usage_limit
                .unwrap()
                .resume_at_reset
        );
        visual.refresh().unwrap();
        let resume = visual.debug_bounds("resume-usage-limit").unwrap();
        visual.simulate_click(resume.center(), Default::default());
        assert!(
            !window
                .update(visual, |v, _, _| v
                    .settings
                    .usage_limit
                    .as_ref()
                    .unwrap()
                    .resume_at_reset)
                .unwrap()
        );

        // Unknown or elapsed reset times keep a manual Resume control and the
        // same right-aligned actions, as in React's composer variant.
        for resets_at in [None, Some(usage::now_ms() - 1_000)] {
            window
                .update(visual, |v, _, cx| {
                    v.settings.usage_limit.as_mut().unwrap().resets_at = resets_at;
                    cx.notify();
                })
                .unwrap();
            visual.refresh().unwrap();
            assert_eq!(
                visual.debug_bounds("resume-usage-limit").unwrap().right(),
                resume.right()
            );
        }
        visual.simulate_click(dismiss.center(), Default::default());
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("usage-limit-notice").is_none());
        assert!(
            window
                .update(visual, |v, _, _| v.settings.usage_limit.is_none())
                .unwrap()
        );
    }

    #[gpui::test]
    fn subscription_limits_persist_and_release_a_mono_only_once_when_armed(
        cx: &mut gpui::TestAppContext,
    ) {
        let store = Arc::new(std::sync::Mutex::new(
            monocode_core::store::Store::open(Path::new(":memory:")).unwrap(),
        ));
        let summary = locked(&store).unwrap().create("codex", "/fixture").unwrap();
        let id = summary.id;
        let view = cx.new(|cx| Conversation::new(summary, store.clone(), cx));
        cx.run_until_parked();
        view.update(cx, |v, cx| {
            v.settings.mono = Some(monocode_core::agent::MonoContext {
                mono: Default::default(),
                root: "/fixture".into(),
                executable: "/fixture/binary".into(),
                ..Default::default()
            });
            v.status = "Usage limit reached: Fixture subscription exhausted".into();
            v.detect_usage_limit(cx);
            assert!(v.settings.usage_limit.is_some());
            assert!(v.follow_ups.state.paused);
        });
        cx.run_until_parked();
        assert!(
            locked(&store)
                .unwrap()
                .agent_settings(id)
                .unwrap()
                .unwrap()
                .usage_limit
                .is_some()
        );
        let resumed = std::rc::Rc::new(std::cell::Cell::new(0));
        cx.update(|cx| {
            let count = resumed.clone();
            cx.subscribe(&view, move |_, _: &UsageResumed, _| {
                count.set(count.get() + 1)
            })
            .detach();
        });
        view.update(cx, |v, cx| {
            v.settings.usage_limit.as_mut().unwrap().resets_at = Some(usage::now_ms() - 31_000);
            v.tick_usage(cx);
            assert!(v.settings.usage_limit.is_some());
            v.settings.usage_limit.as_mut().unwrap().resume_at_reset = true;
            v.tick_usage(cx);
            assert!(v.settings.usage_limit.is_none());
            v.tick_usage(cx);
        });
        cx.run_until_parked();
        assert_eq!(resumed.get(), 1);
    }

    fn near_limit(percent: f64) -> usage::Snapshot {
        usage::Snapshot {
            windows: vec![usage::Window {
                used_percent: percent,
                minutes: 300,
                resets_at: Some(usage::now_ms() + 3_600_000),
            }],
            reset_credits: Some(usage::ResetCredits {
                available_count: 2,
                credits: vec![usage::ResetCredit {
                    id: "g-session".into(),
                    status: "available".into(),
                    reset_type: "claude:session".into(),
                    clears: vec!["five_hour".into()],
                    left: Some(2),
                    needs_limit: true,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn limited_window(
        mode: monocode_core::usage::resets::Mode,
        cx: &mut gpui::TestAppContext,
    ) -> gpui::WindowHandle<Conversation> {
        let store = Arc::new(std::sync::Mutex::new(
            monocode_core::store::Store::open(Path::new(":memory:")).unwrap(),
        ));
        let mut summary = locked(&store)
            .unwrap()
            .create("claude", "/fixture")
            .unwrap();
        summary.has_messages = true;
        cx.update(|cx| {
            cx.set_global(crate::settings::Preferences {
                limit_resets: mode,
                ..Default::default()
            });
            crate::accounts::install(store.clone(), cx);
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(summary, store, cx);
                    view.loaded = true;
                    // A bot's conversation goes on by an event, never a new run.
                    view.settings.mono = Some(monocode_core::agent::MonoContext {
                        mono: Default::default(),
                        root: "/fixture".into(),
                        executable: "/fixture/binary".into(),
                        ..Default::default()
                    });
                    view
                })
            })
            .unwrap()
        })
    }

    fn report_usage(
        window: gpui::WindowHandle<Conversation>,
        visual: &mut gpui::VisualTestContext,
        snapshot: usage::Snapshot,
    ) {
        window
            .update(visual, |view, _, cx| {
                let key = view.usage_key();
                crate::accounts::get(cx).unwrap().update(cx, |center, cx| {
                    center.usage.entry(key).or_default().complete(Ok(snapshot));
                    cx.notify();
                });
            })
            .unwrap();
        visual.run_until_parked();
    }

    #[gpui::test]
    fn a_reset_is_offered_near_the_limit_and_used_at_it_after_a_yes(cx: &mut gpui::TestAppContext) {
        let window = limited_window(Default::default(), cx);
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        // 60%: nothing to say.
        report_usage(window, visual, near_limit(60.));
        assert!(visual.debug_bounds("limit-reset-offer").is_none());
        // 98%: the offer.
        report_usage(window, visual, near_limit(98.));
        assert!(visual.debug_bounds("limit-reset-offer").is_some());
        let yes = visual.debug_bounds("limit-reset-yes").unwrap();
        visual.simulate_click(yes.center(), Default::default());
        visual.run_until_parked();
        assert!(visual.debug_bounds("limit-reset-offer").is_none());
        let resumed = std::rc::Rc::new(std::cell::Cell::new(0));
        let count = resumed.clone();
        window
            .update(visual, |_, _, cx| {
                cx.subscribe(&cx.entity(), move |_, _, _: &UsageResumed, _| {
                    count.set(count.get() + 1)
                })
                .detach();
            })
            .unwrap();
        // At the limit the yes is acted on: the reset is used and the
        // conversation goes on by itself.
        window
            .update(visual, |view, _, cx| {
                crate::accounts::get(cx).unwrap().update(cx, |center, _| {
                    center.reset_stub = Some(Ok("reset".into()));
                });
                view.settings.usage_limit = Some(Limit::default());
            })
            .unwrap();
        report_usage(window, visual, near_limit(100.));
        window
            .update(visual, |view, _, cx| view.tick_usage(cx))
            .unwrap();
        visual.run_until_parked();
        let left = window
            .update(visual, |view, _, _| view.settings.usage_limit.clone())
            .unwrap();
        assert_eq!(left, None);
        assert_eq!(resumed.get(), 1);
    }

    #[gpui::test]
    fn at_the_limit_ask_waits_for_a_click_automatic_does_not_and_off_offers_nothing(
        cx: &mut gpui::TestAppContext,
    ) {
        use monocode_core::usage::resets::Mode;
        // Ask, never answered: the limit notice holds the reset, nothing is used.
        let window = limited_window(Mode::Ask, cx);
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        window
            .update(visual, |view, _, _| {
                view.settings.usage_limit = Some(Limit::default())
            })
            .unwrap();
        report_usage(window, visual, near_limit(100.));
        window
            .update(visual, |view, _, cx| view.tick_usage(cx))
            .unwrap();
        visual.run_until_parked();
        assert!(visual.debug_bounds("use-limit-reset").is_some());
        assert!(visual.debug_bounds("limit-reset-offer").is_none());
        let used = window
            .update(visual, |view, _, cx| {
                let key = view.usage_key();
                crate::accounts::get(cx)
                    .unwrap()
                    .read(cx)
                    .reset_actions
                    .contains_key(&key)
            })
            .unwrap();
        assert!(!used, "Ask never uses a reset without a yes");

        // Automatic: used at the limit without asking; a failure is said
        // and not tried again by itself.
        let window = limited_window(Mode::Automatic, cx);
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        window
            .update(visual, |view, _, cx| {
                view.settings.usage_limit = Some(Limit::default());
                crate::accounts::get(cx).unwrap().update(cx, |center, _| {
                    center.reset_stub = Some(Err(
                        "Claude couldn't reset your limits right now. Nothing was used.".into(),
                    ));
                });
            })
            .unwrap();
        report_usage(window, visual, near_limit(100.));
        window
            .update(visual, |view, _, cx| view.tick_usage(cx))
            .unwrap();
        visual.run_until_parked();
        let (limited, tried) = window
            .update(visual, |view, _, cx| {
                view.tick_usage(cx);
                (
                    view.settings.usage_limit.is_some(),
                    view.limit_reset.tried.clone(),
                )
            })
            .unwrap();
        assert!(limited);
        assert_eq!(tried.as_deref(), Some("g-session"));
        let pending = window
            .update(visual, |view, _, _| view.limit_reset.pending)
            .unwrap();
        assert!(!pending, "a failed reset is not tried again by itself");

        // Off: no offer near the limit and no reset at it.
        let window = limited_window(Mode::Off, cx);
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        report_usage(window, visual, near_limit(99.));
        assert!(visual.debug_bounds("limit-reset-offer").is_none());
        window
            .update(visual, |view, _, _| {
                view.settings.usage_limit = Some(Limit::default())
            })
            .unwrap();
        report_usage(window, visual, near_limit(100.));
        assert!(visual.debug_bounds("use-limit-reset").is_none());
    }
}
