//! A transient provider failure (overloaded, a 5xx, a dropped connection) is
//! sent again by itself after a growing wait, with a notice above the composer
//! that counts down and offers Retry now (providerRetry.ts,
//! ProviderRetryNotice.tsx). The decisions are core's (provider_retry).
use super::*;
use monocode_core::provider_retry::{self as retry, ProviderRetry};

/// What a retry sends: the same words as resuming after a usage limit.
const CONTINUE_PROMPT: &str = "Continue from where you left off.";

#[derive(Default)]
pub(super) struct RetryState {
    pub(super) retry: Option<ProviderRetry>,
    /// Ticks the countdown each second and sends the retry when it is due.
    clock: Option<gpui::Task<()>>,
}

impl Conversation {
    fn retry_session(&self) -> retry::Session<'_> {
        retry::Session {
            usage_limited: self.settings.usage_limit.is_some(),
            automation: false,
            ephemeral: false,
            busy: self.busy,
            retry: self.provider_retry.retry.as_ref(),
        }
    }

    /// Whether a retry waits for its time, for keeping the computer awake.
    pub(crate) fn retry_scheduled(&self) -> bool {
        self.provider_retry
            .retry
            .as_ref()
            .is_some_and(|r| r.retry_at.is_some())
    }

    /// A typed message starts the count over; only a retry keeps it.
    pub(super) fn provider_retry_on_launch(&mut self) {
        if !self
            .provider_retry
            .retry
            .as_ref()
            .is_some_and(|r| r.sending)
        {
            self.provider_retry = RetryState::default();
        }
    }

    /// At the end of a turn: a transient failure schedules the resend; a
    /// retried turn that ended well clears it. Mono chats and remote
    /// sessions keep their own notices.
    pub(super) fn note_provider_retry(&mut self, stopping: bool, cx: &mut Context<Self>) {
        if self.is_remote() || self.settings.mono.is_some() {
            return;
        }
        if stopping || self.status == "Ready" {
            if stopping || retry::succeeded(&self.retry_session()) {
                self.provider_retry = RetryState::default();
            }
            return;
        }
        let now = monocode_core::time::now_ms();
        // Spreads the waits; the clock's low digits are random enough here.
        let random = (now % 997) as f64 / 997.;
        self.provider_retry.retry = retry::next(&self.retry_session(), &self.status, now, random);
        self.arm_provider_retry(cx);
        // The notice shows now, not on the countdown's first tick.
        cx.notify();
    }

    fn arm_provider_retry(&mut self, cx: &mut Context<Self>) {
        let counting = self
            .provider_retry
            .retry
            .as_ref()
            .is_some_and(|r| r.retry_at.is_some());
        self.provider_retry.clock = counting.then(|| {
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(1))
                        .await;
                    let Ok(waiting) = this.update(cx, |this, cx| {
                        cx.notify();
                        let now = monocode_core::time::now_ms();
                        if retry::due(&this.retry_session(), now) {
                            this.retry_provider_now(cx);
                            return false;
                        }
                        this.provider_retry.retry.is_some()
                    }) else {
                        break;
                    };
                    if !waiting {
                        break;
                    }
                }
            })
        });
    }

    /// Sends the turn again now, counting it as the next attempt.
    pub(super) fn retry_provider_now(&mut self, cx: &mut Context<Self>) {
        let Some(current) = self.provider_retry.retry.clone() else {
            return;
        };
        if current.sending || self.busy || self.stopping {
            return;
        }
        self.provider_retry.retry = Some(current.sending());
        self.provider_retry.clock = None;
        self.status = "Ready".into();
        self.launch_prompt(CONTINUE_PROMPT.into(), cx);
        cx.emit(SessionChanged);
        cx.notify();
    }

    fn cancel_provider_retry(&mut self, cx: &mut Context<Self>) {
        self.provider_retry = RetryState::default();
        cx.notify();
    }

    /// The notice above the composer; hidden while the retry runs, since that
    /// turn shows its own progress.
    pub(super) fn provider_retry_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let current = self.provider_retry.retry.as_ref().filter(|r| !r.sending)?;
        let now = monocode_core::time::now_ms();
        let counting = current.retry_at.is_some();
        let button = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .debug_selector(move || id.into())
                .role(Role::Button)
                .aria_label(label)
                .h(px(24.))
                .px(px(6.))
                .flex()
                .flex_shrink_0()
                .items_center()
                .gap(px(4.))
                .rounded(px(6.))
                .text_color(content(0.75))
                .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
        };
        Some(
            div()
                .id("provider-retry-notice")
                .debug_selector(|| "provider-retry-notice".into())
                .role(Role::Status)
                .aria_label(retry::title(current))
                .px(px(8.))
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(content(0.55))
                .child(
                    div()
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
                        .child(icon("refresh-cw", 14.).text_color(gpui::rgb(AMBER_400)))
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_color(content(0.85))
                                .child(retry::title(current)),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .child(retry::status(current, now)),
                        )
                        .child(
                            button("provider-retry-now", "Retry now")
                                .child(icon("rotate-ccw", 14.))
                                .child("Retry now")
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.retry_provider_now(cx)),
                                ),
                        )
                        .child(
                            button(
                                "provider-retry-cancel",
                                if counting { "Don't retry" } else { "Dismiss" },
                            )
                            .size(px(24.))
                            .px_0()
                            .justify_center()
                            .child(icon("close", 14.))
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_provider_retry(cx))),
                        ),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::Path, sync::Arc};

    #[gpui::test]
    fn an_overloaded_provider_counts_down_and_the_user_can_stop_it(cx: &mut gpui::TestAppContext) {
        let store = Arc::new(std::sync::Mutex::new(
            monocode_core::store::Store::open(Path::new(":memory:")).unwrap(),
        ));
        let mut summary = locked(&store)
            .unwrap()
            .create("claude", "/fixture")
            .unwrap();
        summary.has_messages = true;
        let window = cx.update(|cx| {
            crate::accounts::install(store.clone(), cx);
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(summary, store, cx);
                    view.loaded = true;
                    view.status = "API Error: 529 Overloaded".into();
                    view
                })
            })
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        window
            .update(visual, |v, _, cx| v.note_provider_retry(false, cx))
            .unwrap();
        let retry = window
            .update(visual, |v, _, _| v.provider_retry.retry.clone())
            .unwrap()
            .unwrap();
        assert_eq!(retry.attempt, 1);
        assert!(retry.retry_at.is_some());
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("provider-retry-notice").is_some());
        let cancel = visual.debug_bounds("provider-retry-cancel").unwrap();
        visual.simulate_click(cancel.center(), Default::default());
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("provider-retry-notice").is_none());

        // A usage limit is not a retry.
        window
            .update(visual, |v, _, cx| {
                v.status = "Claude Code reached its usage limit.".into();
                v.note_provider_retry(false, cx);
            })
            .unwrap();
        assert!(
            window
                .update(visual, |v, _, _| v.provider_retry.retry.is_none())
                .unwrap()
        );
    }
}
