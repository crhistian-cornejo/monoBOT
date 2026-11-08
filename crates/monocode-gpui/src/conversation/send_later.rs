//! Send later (SendLaterButton.tsx, scheduledSend.ts): a clock beside Send
//! schedules the message for one of Remind me's times, or any date and time.
//! It waits in the message queue, saying when it sends, until its time comes;
//! then it goes like any queued message. Presets and checks are core's.
use super::*;
use gpui::{App, Task};
use monocode_core::scheduled_send as later;

#[derive(Default)]
pub(super) struct SendLater {
    pub(super) anchor: AnchorSlot,
    /// The date and time form, while open.
    form: Option<Form>,
    /// Wakes when the next scheduled message is due.
    clock: Option<Task<()>>,
}

struct Form {
    input: Entity<input::Input>,
    error: Option<&'static str>,
    _submit: Subscription,
}

impl Conversation {
    /// Only where a scheduled message survives a restart: a local session's
    /// own queue (a Mono's messages go through its own outbox).
    pub(super) fn can_send_later(&self, cx: &App) -> bool {
        self.settings.mono.is_none()
            && !self.is_remote()
            && !self.settings.worktree_removed
            && self.loaded
            && !self.attaching
            && !self.draft_active(cx)
            && (self.input.read(cx).has_message_text()
                || !self.attachments.is_empty()
                || !self.quotes.is_empty())
    }

    /// The clock beside Send.
    pub(super) fn send_later_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let open = self.menu_is(ComposerMenu::SendLater) || self.send_later.form.is_some();
        div()
            .id("send-later")
            .debug_selector(|| "send-later".into())
            .role(Role::Button)
            .aria_label("Send later")
            .tooltip(crate::theme::chrome_tip("Send later", None))
            .aria_expanded(open)
            .size(px(26.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .text_color(content(if open { 0.9 } else { 0.45 }))
            .when(open, |d| d.bg(content(0.08)))
            .hover(|s| s.bg(content(0.08)).text_color(content(0.9)))
            .child(self.send_later.anchor.probe())
            .child(icon("clock", 14.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| {
                this.send_later.form = None;
                this.toggle_menu(ComposerMenu::SendLater, window, cx);
            }))
            .into_any_element()
    }

    /// The menu: the same times as Remind me, then any date and time.
    pub(super) fn send_later_items(&self) -> Vec<MenuItem> {
        let now = chrono::Local::now();
        let mut items: Vec<MenuItem> = later::preset_choices(&now)
            .into_iter()
            .map(|choice| {
                MenuAction::new(choice.preset.id(), choice.label)
                    .disabled(!choice.enabled)
                    .into()
            })
            .collect();
        items.push(MenuItem::Separator);
        items.push(MenuAction::new(later::CUSTOM, "Choose date and time").into());
        items
    }

    /// A pick from the Send later menu; false when the id is not one.
    pub(super) fn send_later_choice(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        let Some(preset) = later::Preset::from_id(id) else {
            return false;
        };
        if self.menu_is(ComposerMenu::SendLater)
            && let Some(at) = later::preset_time(preset, &chrono::Local::now())
        {
            self.send_later(at, cx);
        }
        true
    }

    /// "Choose date and time": the form opens over the clock, focused.
    pub(super) fn open_send_later_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let initial = chrono::Local::now() + chrono::Duration::hours(1);
        let text = later::format_local_datetime(&initial).replace('T', " ");
        let input = cx.new(|cx| {
            let mut field = input::Input::field("Send at", "YYYY-MM-DD HH:MM", content(0.4), cx);
            field.set_text(text, cx);
            field
        });
        let submit = cx.subscribe(&input, |this, _, _: &input::Submitted, cx| {
            this.submit_send_later_form(cx)
        });
        window.focus(&input.focus_handle(cx), cx);
        self.send_later.form = Some(Form {
            input,
            error: None,
            _submit: submit,
        });
        cx.notify();
    }

    fn submit_send_later_form(&mut self, cx: &mut Context<Self>) {
        let Some(form) = &mut self.send_later.form else {
            return;
        };
        let value = form.input.read(cx).content.trim().replace(' ', "T");
        match later::validate_custom(&value, &chrono::Local::now()) {
            Ok(at) => {
                self.send_later.form = None;
                self.send_later(at, cx);
            }
            Err(error) => {
                form.error = Some(error);
                cx.notify();
            }
        }
    }

    /// Takes the composer's message into the queue, to send at `send_at`.
    pub(super) fn send_later(&mut self, send_at: u64, cx: &mut Context<Self>) {
        if !self.can_send_later(cx) || self.input.read(cx).is_composing() {
            return;
        }
        let text = self
            .input
            .update(cx, |input, cx| input.take(cx))
            .unwrap_or_default();
        let text = self.take_quotes(text);
        let mode = if self.settings.orchestrator {
            Some("orchestrator")
        } else if self.settings.plan {
            Some("plan")
        } else if self.settings.operator {
            Some("operator")
        } else {
            None
        };
        let attachments = std::mem::take(&mut self.attachments);
        let mut message = self.message(text, attachments, mode.map(str::to_owned), cx);
        message.send_at = Some(send_at);
        self.follow_ups.motion.insert(
            &message.id,
            self.follow_ups.state.messages.is_empty(),
            !cx.reduce_motion(),
        );
        self.follow_ups.state.messages.push(message);
        self.save_follow_ups(cx);
        self.arm_scheduled_sends(cx);
    }

    /// Sends a scheduled message now instead of at its time.
    pub(super) fn send_scheduled_now(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(message) = self
            .follow_ups
            .state
            .messages
            .iter_mut()
            .find(|m| m.id == id)
        else {
            return;
        };
        message.send_at = None;
        self.save_follow_ups(cx);
        self.arm_scheduled_sends(cx);
    }

    /// Wakes when the next scheduled message is due and lets it go.
    pub(super) fn arm_scheduled_sends(&mut self, cx: &mut Context<Self>) {
        let next = self
            .follow_ups
            .state
            .messages
            .iter()
            .filter_map(|m| m.send_at)
            .min();
        self.send_later.clock = next.map(|at| {
            let wait = at.saturating_sub(monocode_core::time::now_ms());
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(wait))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    let now = monocode_core::time::now_ms();
                    let mut released = false;
                    for message in &mut this.follow_ups.state.messages {
                        if message.send_at.is_some_and(|at| at <= now) {
                            message.send_at = None;
                            released = true;
                        }
                    }
                    if released {
                        this.save_follow_ups(cx);
                    }
                    this.arm_scheduled_sends(cx);
                });
            })
        });
    }

    /// Whether a message waits for its time, for keeping the computer awake.
    pub(crate) fn has_scheduled_message(&self) -> bool {
        self.follow_ups
            .state
            .messages
            .iter()
            .any(|m| m.send_at.is_some())
    }

    /// The date and time form over the clock.
    pub(super) fn send_later_form(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.send_later.form.as_ref()?;
        let weak = cx.entity().downgrade();
        let action = |id: &'static str, label: &'static str, primary: bool| {
            div()
                .id(id)
                .debug_selector(move || id.into())
                .role(Role::Button)
                .aria_label(label)
                .rounded_md()
                .px_3()
                .py_1p5()
                .text_size(px(12.))
                .map(|d| {
                    if primary {
                        d.bg(accent_color()).text_color(gpui::white())
                    } else {
                        d.hover(|s| s.bg(content(0.08)))
                    }
                })
                .child(label)
        };
        Some(
            popover(
                "send-later-form",
                Anchor::Slot(self.send_later.anchor.clone()),
            )
            .side(Side::Top)
            .width(280.)
            .on_dismiss(move |_, cx| {
                let _ = weak.update(cx, |this, cx| {
                    this.send_later.form = None;
                    cx.notify();
                });
            })
            .child(
                div()
                    .id("send-later-form")
                    .debug_selector(|| "send-later-form".into())
                    .role(Role::Dialog)
                    .aria_label("Send later")
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(content(0.7))
                            .child("Send this message on"),
                    )
                    .child(
                        div()
                            .h(px(36.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded_md()
                            .border_1()
                            .border_color(content(0.1))
                            .bg(background_base())
                            .text_size(px(13.))
                            .child(form.input.clone()),
                    )
                    .children(form.error.map(|error| {
                        div()
                            .id("send-later-error")
                            .role(Role::Alert)
                            .text_size(px(12.))
                            .text_color(gpui::rgb(0xf87171))
                            .child(error)
                    }))
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(action("send-later-cancel", "Cancel", false).on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.send_later.form = None;
                                    window.focus(&this.input.focus_handle(cx), cx);
                                    cx.notify();
                                }),
                            ))
                            .child(action("send-later-schedule", "Schedule", true).on_click(
                                cx.listener(|this, _, _, cx| this.submit_send_later_form(cx)),
                            )),
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
    fn a_message_sent_later_waits_in_the_queue_until_sent_now(cx: &mut gpui::TestAppContext) {
        let store = Arc::new(std::sync::Mutex::new(
            monocode_core::store::Store::open(Path::new(":memory:")).unwrap(),
        ));
        let summary = locked(&store)
            .unwrap()
            .create("claude", "/fixture")
            .unwrap();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(summary, store, cx);
                    view.loaded = true;
                    view
                })
            })
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let at = monocode_core::time::now_ms() + 3_600_000;
        window
            .update(visual, |v, _, cx| {
                v.input
                    .update(cx, |input, cx| input.set_text("Ship it".into(), cx));
                assert!(v.can_send_later(cx));
                v.send_later(at, cx);
            })
            .unwrap();
        visual.run_until_parked();
        let (queued, scheduled, busy) = window
            .update(visual, |v, _, cx| {
                (
                    v.follow_ups.state.messages.clone(),
                    v.has_scheduled_message(),
                    v.busy || v.input.read(cx).has_message_text(),
                )
            })
            .unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].text, "Ship it");
        assert_eq!(queued[0].send_at, Some(at));
        assert!(scheduled);
        assert!(
            !busy,
            "it waits instead of sending, and the composer is clear"
        );
        let id = queued[0].id.clone();
        window
            .update(visual, |v, _, cx| v.send_scheduled_now(&id, cx))
            .unwrap();
        assert!(
            !window
                .update(visual, |v, _, _| v.has_scheduled_message())
                .unwrap()
        );
    }

    #[gpui::test]
    fn added_text_waits_as_a_quote_chip_and_goes_out_as_a_quote(cx: &mut gpui::TestAppContext) {
        let store = Arc::new(std::sync::Mutex::new(
            monocode_core::store::Store::open(Path::new(":memory:")).unwrap(),
        ));
        let summary = locked(&store)
            .unwrap()
            .create("claude", "/fixture")
            .unwrap();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(summary, store, cx);
                    view.loaded = true;
                    view
                })
            })
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        window
            .update(visual, |v, w, cx| {
                v.mono_quote("The pump reached 62% efficiency.", w, cx);
                v.mono_quote("The pump reached 62% efficiency.", w, cx);
            })
            .unwrap();
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("composer-quote-chip").is_some());
        let sent = window
            .update(visual, |v, _, cx| {
                assert_eq!(v.quotes.len(), 1, "the same words wait once");
                assert!(v.can_send_later(cx), "a quote alone is something to send");
                v.take_quotes("Why so low?".into())
            })
            .unwrap();
        assert!(sent.starts_with("> The pump reached 62% efficiency."));
        assert!(sent.ends_with("\n\nWhy so low?"));
        assert!(
            window
                .update(visual, |v, _, _| v.quotes.is_empty())
                .unwrap()
        );
    }
}
