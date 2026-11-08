//! Keeping the inbox in order, as Mailflare and Gmail do: snoozing a message until later,
//! stopping a list's mail with the unsubscribe its sender offers, the earlier messages of a
//! conversation, and Gmail's labels. Snoozes are this computer's own; labels and archives are
//! the server's.
use super::*;

/// When a snooze can end, as the menu offers it.
pub(super) const SNOOZES: [(&str, &str); 3] = [
    ("later", "In 3 hours"),
    ("tomorrow", "Tomorrow 8:00"),
    ("week", "Next Monday 8:00"),
];

/// Gmail's labels of a message as small chips.
pub(super) fn label_chips(labels: &[String]) -> Div {
    div()
        .flex()
        .flex_wrap()
        .gap(px(4.))
        .children(labels.iter().map(|label| {
            div()
                .px(px(6.))
                .py(px(1.))
                .rounded(px(4.))
                .bg(content(0.07))
                .text_size(px(10.))
                .text_color(content(0.65))
                .child(label.clone())
        }))
}

impl Mailbox {
    /// Hides `id` until `choice` (one of `SNOOZES`), or wakes it with `None`.
    pub(super) fn snooze(&mut self, id: String, choice: Option<&str>, cx: &mut Context<Self>) {
        let until = match choice {
            Some(choice) => match model::snooze_until(choice, model::now_ms()) {
                Some(until) => Some(until),
                None => return,
            },
            None => None,
        };
        self.snooze_menu = false;
        let service = self.service.clone();
        let task = cx.background_executor().spawn({
            let id = id.clone();
            async move { service.snooze(&id, until) }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(snoozed) => {
                        this.snoozed = snoozed;
                        if until.is_some() && this.selected.as_deref() == Some(id.as_str()) {
                            this.close_message(cx);
                        }
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Stops the list's mail as its sender offers: one press, its page, or a message.
    pub(super) fn unsubscribe(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let task = cx
            .background_executor()
            .spawn(async move { service.unsubscribe(&id) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                let notice = match result {
                    Ok(model::Unsubscribed::Done) => {
                        "Unsubscribed. The sender may take a few days to stop.".to_owned()
                    }
                    Ok(model::Unsubscribed::OpenPage(url)) => {
                        cx.open_url(&url);
                        "Opened the sender's page to finish unsubscribing.".to_owned()
                    }
                    Ok(model::Unsubscribed::WriteTo(address, subject)) => {
                        let note = format!("Send the message to {address} to unsubscribe.");
                        this.compose_to(address, subject, window, cx);
                        note
                    }
                    Err(error) => error,
                };
                if let Some(reader) = this.reader.as_mut() {
                    reader.notice = Some(notice);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A new message to `to` with `subject`, for a list that unsubscribes by mail.
    fn compose_to(
        &mut self,
        to: String,
        subject: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.compose = None;
        self.start_compose(false, window, cx);
        if let Some(compose) = &self.compose {
            compose.update(cx, |compose, cx| {
                compose.prefill(&to, &subject, "unsubscribe", cx)
            });
        }
    }

    /// The earlier messages of `message`'s conversation that the reading holds, to open.
    pub(super) fn thread_view(
        &self,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let others = model::thread_of(self.messages(), message);
        if others.is_empty() {
            return None;
        }
        Some(
            div()
                .id("mail-thread")
                .debug_selector(|| "mail-thread".into())
                .mt(px(12.))
                .flex()
                .flex_col()
                .border_1()
                .border_color(content(0.08))
                .child(
                    div()
                        .px(px(10.))
                        .py(px(6.))
                        .text_size(px(11.))
                        .text_color(content(0.5))
                        .child(format!("{} more in this conversation", others.len())),
                )
                .children(others.into_iter().map(|other| {
                    let id = other.id.clone();
                    div()
                        .id(SharedString::from(format!("mail-thread-{}", other.id)))
                        .role(Role::Button)
                        .aria_label(format!("Open the message from {}", other.from))
                        .flex()
                        .gap(px(8.))
                        .px(px(10.))
                        .py(px(6.))
                        .border_t_1()
                        .border_color(content(0.06))
                        .text_size(px(12.))
                        .cursor_pointer()
                        .hover(|s| s.bg(content(0.04)))
                        .child(
                            div()
                                .w(px(120.))
                                .flex_shrink_0()
                                .truncate()
                                .text_color(content(0.85))
                                .child(other.from.clone()),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .text_color(content(0.5))
                                .child(other.preview.clone()),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_color(content(0.4))
                                .child(model::row_time(other.at, model::now_ms())),
                        )
                        .on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.open(id.clone(), window, cx)
                            }),
                        )
                }))
                .into_any_element(),
        )
    }

    /// The row of snooze choices under the reader's bar.
    pub(super) fn snooze_menu_view(&self, id: &str, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("mail-snooze-menu")
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(12.))
            .py(px(6.))
            .border_b_1()
            .border_color(content(0.07))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(content(0.5))
                    .child("Snooze until"),
            )
            .children(SNOOZES.into_iter().map(|(choice, label)| {
                let id = id.to_owned();
                text_button(format!("mail-snooze-{choice}"), label, 0.8)
                    .debug_selector(move || format!("mail-snooze-{choice}"))
                    .on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.snooze(id.clone(), Some(choice), cx)
                        }),
                    )
            }))
            .into_any_element()
    }
}
