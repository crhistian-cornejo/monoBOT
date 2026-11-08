//! The open message (the desktop app's MailReader and MailAttachments): its bar of
//! actions, who sent it and when, its files in place, its body in a page that runs
//! nothing, and the reply at its foot.
use super::*;
use monocode_core::inbox::mail::{Attachment, Level};

impl Mailbox {
    pub(super) fn reader_view(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(message) = self.selected_message().cloned() else {
            return self.idle();
        };
        let reader = self.reader.as_ref();
        let held = message.triage == model::Triage::Held;
        let priority = message.triage == model::Triage::Priority;
        let archived = message.folder == model::Folder::Archive;
        let starred = message.starred;
        let has_html = reader.is_some_and(|r| r.html.is_some());
        let plain_text = reader.is_some_and(|r| r.plain_text);
        let id = message.id.clone();
        let snoozed = self
            .snoozed
            .get(&message.id)
            .is_some_and(|until| *until > model::now_ms());
        let toolbar = div()
            .h(px(40.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(8.))
            .border_b_1()
            .border_color(content(0.07))
            .child(
                tool("mail-back", "Back", "arrow-left", content(0.6))
                    .on_click(cx.listener(|this, _, _, cx| this.close_message(cx))),
            )
            .child(div().flex_1())
            .child(
                tool(
                    "mail-star",
                    if starred { "Unstar" } else { "Star" },
                    if starred { "star-filled" } else { "star" },
                    if starred {
                        accent_color()
                    } else {
                        content(0.6)
                    },
                )
                .on_click(cx.listener({
                    let id = id.clone();
                    move |this, _, _, cx| {
                        this.act(
                            &id,
                            if starred {
                                Action::Unstar
                            } else {
                                Action::Star
                            },
                            cx,
                        )
                    }
                })),
            )
            .child(
                tool_enabled(
                    "mail-archive",
                    "Archive",
                    "archive",
                    content(if archived { 0.25 } else { 0.6 }),
                    !archived,
                )
                .on_click(cx.listener({
                    let id = id.clone();
                    move |this, _, _, cx| {
                        if !archived {
                            this.act(&id, Action::Archive, cx)
                        }
                    }
                })),
            )
            .child(
                text_button(
                    "mail-priority",
                    if priority {
                        "Not priority"
                    } else {
                        "Mark sender as priority"
                    },
                    0.7,
                )
                .on_click(cx.listener({
                    let address = message.address.clone();
                    move |this, _, _, cx| this.mark_interesting(address.clone(), !priority, cx)
                })),
            )
            .when(has_html, |d| {
                d.child(
                    text_button(
                        "mail-toggle-text",
                        if plain_text {
                            "Show original"
                        } else {
                            "Plain text"
                        },
                        0.7,
                    )
                    .debug_selector(|| "mail-toggle-text".into())
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(reader) = this.reader.as_mut() {
                            reader.plain_text = !reader.plain_text;
                            cx.notify();
                        }
                    })),
                )
            })
            .when(message.unsubscribe.is_some() && !held, |d| {
                d.child(
                    text_button("mail-unsubscribe", "Unsubscribe", 0.7)
                        .debug_selector(|| "mail-unsubscribe".into())
                        .on_click(cx.listener({
                            let id = id.clone();
                            move |this, _, window, cx| this.unsubscribe(id.clone(), window, cx)
                        })),
                )
            })
            .when(message.folder == model::Folder::Inbox, |d| {
                if snoozed {
                    d.child(
                        text_button("mail-wake", "Unsnooze", 0.7).on_click(cx.listener({
                            let id = id.clone();
                            move |this, _, _, cx| this.snooze(id.clone(), None, cx)
                        })),
                    )
                } else {
                    d.child(
                        text_button("mail-snooze", "Snooze", 0.7)
                            .debug_selector(|| "mail-snooze".into())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.snooze_menu = !this.snooze_menu;
                                cx.notify();
                            })),
                    )
                }
            })
            .when(!held, |d| {
                d.child(text_button("mail-forward", "Forward (F)", 0.7).on_click(
                    cx.listener(|this, _, window, cx| this.start_compose(true, window, cx)),
                ))
                .child(
                    text_button("mail-reply", "Reply (R)", 1.)
                        .on_click(cx.listener(|this, _, window, cx| this.start_reply(window, cx))),
                )
            });

        let opened = reader.and_then(|r| r.opened.as_ref());
        let remote = opened.map_or(0, |o| o.remote.len());
        let images_landed = reader.is_some_and(|r| !r.images.is_empty());
        let loading_images = reader.is_some_and(|r| r.loading_images);
        // Native pages draw over the window: one under a dialog would hide it.
        let covered = self.settings_open || reader.is_some_and(|r| r.zoomed.is_some());
        let header = div()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .px(px(20.))
            .pt(px(16.))
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(content(1.))
                    .child(if message.subject.is_empty() {
                        "(no subject)".to_string()
                    } else {
                        message.subject.clone()
                    }),
            )
            .child(
                div()
                    .mt(px(4.))
                    .text_size(px(12.))
                    .text_color(content(0.6))
                    .child(format!(
                        "{} <{}> · {}",
                        message.from,
                        message.address,
                        model::full_time(message.at)
                    )),
            )
            .when(!message.labels.is_empty(), |d| {
                d.child(div().mt(px(6.)).child(organize::label_chips(&message.labels)))
            })
            .children(self.thread_view(&message, cx))
            .when(held, |d| {
                d.child(
                    div()
                        .mt(px(12.))
                        .px(px(12.))
                        .py(px(8.))
                        .border_1()
                        .border_color(gpui::Hsla::from(rgb(0xf87171)).opacity(0.3))
                        .bg(gpui::Hsla::from(rgb(0xf87171)).opacity(0.08))
                        .text_size(px(12.))
                        .text_color(content(1.))
                        .child(format!(
                            "Held back{}. It may be a scam, so it can't be answered from here. Don't open its links or files unless you trust the sender.",
                            message
                                .reason
                                .as_ref()
                                .map(|reason| format!(": {reason}"))
                                .unwrap_or_default()
                        )),
                )
            })
            .when_some(opened, |d, opened| {
                d.child(self.attachments_view(&opened.attachments, cx))
            })
            .when(remote > 0 && !images_landed, |d| {
                if loading_images {
                    d.child(
                        div()
                            .mt(px(12.))
                            .text_size(px(12.))
                            .text_color(content(0.45))
                            .child("Loading pictures…"),
                    )
                } else {
                    d.child(
                        div().mt(px(12.)).flex().child(
                            text_button(
                                "mail-show-pictures",
                                format!(
                                    "Show {remote} picture{} from the web",
                                    if remote == 1 { "" } else { "s" }
                                ),
                                0.6,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.load_images(cx))),
                        ),
                    )
                }
            });

        let body: AnyElement = match reader {
            Some(Reader {
                error: Some(error), ..
            }) => div()
                .px(px(20.))
                .child(alert(error.clone()))
                .into_any_element(),
            Some(Reader {
                opened: Some(opened),
                html,
                ..
            }) => {
                let paragraphs = if opened.text.is_empty() {
                    if message.body.is_empty() {
                        vec![message.preview.clone()]
                    } else {
                        message.body.clone()
                    }
                } else {
                    opened.text.clone()
                };
                let text = div()
                    .id("mail-text")
                    .debug_selector(|| "mail-text".into())
                    .size_full()
                    .overflow_y_scroll()
                    .px(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .text_size(px(13.))
                    .line_height(px(21.))
                    .text_color(content(1.))
                    .children(paragraphs.into_iter().map(|p| div().child(p)))
                    .into_any_element();
                match html {
                    Some(html) if !covered && !plain_text => div()
                        .id("mail-html")
                        .debug_selector(|| "mail-html".into())
                        .relative()
                        .size_full()
                        .child(text)
                        .child(div().absolute().inset_0().px(px(20.)).child(html.clone()))
                        .into_any_element(),
                    Some(_) if covered => div().into_any_element(),
                    _ => text,
                }
            }
            _ => div()
                .px(px(20.))
                .text_size(px(13.))
                .text_color(content(0.5))
                .child("Opening…")
                .into_any_element(),
        };
        let notice = reader.and_then(|r| r.notice.clone());
        let zoomed = reader.and_then(|r| {
            let index = r.zoomed?;
            let picture = r.pictures.iter().find(|(i, _)| *i == index)?.1.clone();
            let name = r
                .opened
                .as_ref()?
                .attachments
                .iter()
                .find(|a| a.index == index)?
                .name
                .clone();
            Some((name, picture))
        });

        div()
            .id("mail-reader")
            .role(Role::Article)
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .when(self.snooze_menu && !snoozed, |d| {
                d.child(self.snooze_menu_view(&id, cx))
            })
            .child(
                div()
                    .min_h_0()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(header)
                    .child(
                        div()
                            .mt(px(16.))
                            .min_h_0()
                            .flex_1()
                            .overflow_hidden()
                            .child(body),
                    )
                    .when_some(notice, |d, notice| {
                        d.child(div().px(px(20.)).mt(px(8.)).child(alert(notice)))
                    })
                    .when_some(self.reply.clone(), |d, reply| {
                        d.child(
                            div()
                                .flex_shrink_0()
                                .w_full()
                                .px(px(20.))
                                .pt(px(12.))
                                .pb(px(12.))
                                .flex()
                                .justify_center()
                                .child(div().w_full().max_w(px(560.)).child(reply)),
                        )
                    }),
            )
            .when_some(zoomed, |d, (name, picture)| {
                d.child(crate::modal::window_overlay(
                    window,
                    self.zoom_view(name, picture, window, cx),
                ))
            })
            .into_any_element()
    }

    /// A message's files, in place: pictures show from memory, the rest say what a look at
    /// their bytes found. Nothing is saved to disk until the user presses Open, and a
    /// blocked file can't be.
    fn attachments_view(&self, attachments: &[Attachment], cx: &mut Context<Self>) -> AnyElement {
        if attachments.is_empty() {
            return div().into_any_element();
        }
        let pictures = self
            .reader
            .as_ref()
            .map(|r| r.pictures.clone())
            .unwrap_or_default();
        let files: Vec<&Attachment> = attachments.iter().filter(|a| a.preview.is_none()).collect();
        div()
            .id("mail-attachments")
            .role(Role::Group)
            .aria_label("Attachments")
            .mt(px(12.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .when(!pictures.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .children(pictures.into_iter().map(|(index, picture)| {
                            let name = attachments
                                .iter()
                                .find(|a| a.index == index)
                                .map(|a| a.name.clone())
                                .unwrap_or_default();
                            div()
                                .id(SharedString::from(format!("mail-picture-{index}")))
                                .role(Role::Button)
                                .aria_label(name)
                                .overflow_hidden()
                                .border_1()
                                .border_color(content(0.1))
                                .hover(|s| s.border_color(content(0.25)))
                                .cursor_pointer()
                                .child(
                                    gpui::img(picture)
                                        .h(px(96.))
                                        .max_w(px(192.))
                                        .object_fit(gpui::ObjectFit::Cover),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(reader) = this.reader.as_mut() {
                                        reader.zoomed = Some(index);
                                        cx.notify();
                                    }
                                }))
                        })),
                )
            })
            .when(!files.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .children(files.into_iter().map(|file| {
                            let index = file.index;
                            let blocked = file.risk.level == Level::Danger;
                            let (verdict, ink) = match file.risk.level {
                                Level::Safe => ("Checked", content(0.45)),
                                Level::Caution => ("Careful", accent_color()),
                                Level::Danger => ("Blocked", rgb(0xf87171).into()),
                            };
                            let red: gpui::Hsla = rgb(0xf87171).into();
                            div()
                                .max_w(px(320.))
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .px(px(10.))
                                .py(px(6.))
                                .border_1()
                                .border_color(if blocked {
                                    red.opacity(0.3)
                                } else {
                                    content(0.1)
                                })
                                .when(blocked, |d| d.bg(red.opacity(0.05)))
                                .text_size(px(12.))
                                .child(icon("file", 16.).text_color(content(0.45)))
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .child(
                                            div()
                                                .truncate()
                                                .text_color(content(1.))
                                                .child(file.name.clone()),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(4.))
                                                .text_size(px(11.))
                                                .child(
                                                    div()
                                                        .text_color(content(0.4))
                                                        .child(model::size_label(file.size)),
                                                )
                                                .child(div().text_color(content(0.25)).child("·"))
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap(px(2.))
                                                        .text_color(ink)
                                                        .when(file.risk.level != Level::Safe, |d| {
                                                            d.child(
                                                                icon("shield-alert", 12.)
                                                                    .text_color(ink),
                                                            )
                                                        })
                                                        .child(verdict),
                                                ),
                                        )
                                        .when_some(
                                            file.risk.reasons.first().cloned(),
                                            |d, reason| {
                                                d.child(
                                                    div()
                                                        .truncate()
                                                        .text_size(px(11.))
                                                        .text_color(content(0.5))
                                                        .child(reason),
                                                )
                                            },
                                        ),
                                )
                                .when(!blocked, |d| {
                                    d.child(
                                        text_button(
                                            SharedString::from(format!("mail-open-file-{index}")),
                                            "Open",
                                            0.7,
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.open_attachment(index, cx)
                                            }),
                                        ),
                                    )
                                })
                        })),
                )
            })
            .into_any_element()
    }

    fn zoom_view(
        &self,
        name: String,
        picture: Arc<Image>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("mail-zoom-dismiss")
                    .absolute()
                    .inset_0()
                    .child(crate::modal::scrim("mail-zoom-scrim"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(reader) = this.reader.as_mut() {
                            reader.zoomed = None;
                            cx.notify();
                        }
                    })),
            )
            .child(crate::modal::enter(
                "mail-zoom-card",
                div()
                    .id("mail-zoom")
                    .role(Role::Dialog)
                    .aria_label(name.clone())
                    .occlude()
                    .relative()
                    .max_w(gpui::relative(0.85))
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
                            .gap(px(8.))
                            .px(px(20.))
                            .pt(px(16.))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(content(1.))
                                    .child(name),
                            )
                            .child(
                                tool("mail-zoom-close", "Close", "close", content(0.5)).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        if let Some(reader) = this.reader.as_mut() {
                                            reader.zoomed = None;
                                            cx.notify();
                                        }
                                    }),
                                ),
                            ),
                    )
                    .child(
                        div().relative().p(px(16.)).flex().justify_center().child(
                            gpui::img(picture)
                                .max_h(window.viewport_size().height * 0.7)
                                .max_w_full()
                                .object_fit(gpui::ObjectFit::Contain),
                        ),
                    ),
            ))
            .into_any_element()
    }
}
