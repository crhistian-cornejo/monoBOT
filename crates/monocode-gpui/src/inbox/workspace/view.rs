use super::*;
use gpui::{
    div, prelude::*, px, rgb, AnyElement, Div, Focusable, FontWeight, Role, SharedString, Stateful,
    Window,
};

const LABEL: f32 = 0.7;
/// The reading column of both pages.
const COLUMN: f32 = 720.;

/// The square icon buttons of the bars (`size-7`).
fn tool(id: impl Into<SharedString>, label: &str, name: &'static str) -> Stateful<Div> {
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
        .cursor_pointer()
        .tooltip(chrome_tip(tip, None))
        .hover(|s| s.bg(content(0.08)))
        .child(icon(name, 14.).text_color(content(0.7)))
}

/// The flat text buttons of the mail (`rounded-md px-2.5 py-1 text-[12px] hover:bg-content/8`).
fn text_button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    ink: f32,
    enabled: bool,
) -> Stateful<Div> {
    let id: SharedString = id.into();
    let label: SharedString = label.into();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .flex_shrink_0()
        .rounded(px(6.))
        .px(px(10.))
        .py(px(4.))
        .text_size(px(12.))
        .text_color(content(if enabled { ink } else { ink * 0.5 }))
        .when(enabled, |d| {
            d.cursor_pointer().hover(|s| s.bg(content(0.08)))
        })
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

fn alert(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(12.))
        .line_height(px(18.))
        .text_color(rgb(0xf87171))
        .child(text.into())
}

fn hint(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(11.))
        .line_height(px(16.))
        .text_color(content(0.5))
        .child(text.into())
}

/// A labelled field: `text-[12px] text-content/70` over an `h-9` bordered box.
fn labelled(
    label: impl Into<SharedString>,
    input: &Entity<Input>,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let focused = input.focus_handle(cx).is_focused(window);
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(12.))
                .text_color(content(LABEL))
                .child(label.into()),
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

/// The 36x20 switch of the app's settings.
fn switch(id: impl Into<SharedString>, label: &str, on: bool) -> Stateful<Div> {
    let id: SharedString = id.into();
    div()
        .id(id)
        .role(Role::Switch)
        .aria_label(label.to_owned())
        .aria_toggled(if on {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .relative()
        .h(px(20.))
        .w(px(36.))
        .flex_shrink_0()
        .cursor_pointer()
        .bg(if on { accent_color() } else { content(0.2) })
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(if on { 18. } else { 2. }))
                .size(px(16.))
                .bg(rgb(0xffffff)),
        )
}

/// A choice among a few, as the mail's provider picker draws it.
fn choice(id: impl Into<SharedString>, label: &str, chosen: bool) -> Stateful<Div> {
    let id: SharedString = id.into();
    div()
        .id(id)
        .role(Role::RadioButton)
        .aria_label(label.to_owned())
        .px(px(12.))
        .py(px(6.))
        .text_size(px(12.))
        .cursor_pointer()
        .text_color(content(if chosen { 1. } else { 0.6 }))
        .when(chosen, |d| d.bg(content(0.1)))
        .when(!chosen, |d| d.hover(|s| s.bg(content(0.08))))
        .child(label.to_owned())
}

/// A settings section: its title and what it is for, with the fields added after.
fn section(title: &'static str, text: &'static str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .pb(px(24.))
        .mb(px(24.))
        .border_b_1()
        .border_color(content(0.07))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(content(1.))
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(19.))
                        .text_color(content(0.55))
                        .child(text),
                ),
        )
}

impl Workspace {
    fn event_row(&self, entry: &Entry, cx: &mut Context<Self>) -> Div {
        let event = &entry.event;
        let note = event
            .description
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_owned();
        let confirming = self.deleting.as_deref() == Some(event.id.as_str());
        let idle = self.busy.is_none();
        let id = event.id.clone();
        div()
            .flex()
            .items_start()
            .gap(px(12.))
            .py(px(10.))
            .border_t_1()
            .border_color(content(0.06))
            .child(
                div()
                    .w(px(140.))
                    .flex_shrink_0()
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(content(0.6))
                    .child(when_label(entry))
                    .when_some(entry.origin.clone(), |d, origin| {
                        d.child(
                            div()
                                .truncate()
                                .text_size(px(11.))
                                .text_color(content(0.4))
                                .child(origin),
                        )
                    }),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(content(1.))
                            .child(if event.title.trim().is_empty() {
                                "Untitled".to_owned()
                            } else {
                                event.title.clone()
                            }),
                    )
                    .when(!note.is_empty(), |d| {
                        d.child(
                            div()
                                .truncate()
                                .text_size(px(12.))
                                .line_height(px(18.))
                                .text_color(content(0.5))
                                .child(note),
                        )
                    }),
            )
            .when_some(source_label(&event.source), |d, label| {
                d.child(
                    div()
                        .flex_shrink_0()
                        .rounded(px(3.))
                        .px(px(6.))
                        .py(px(1.))
                        .bg(content(0.08))
                        .text_size(px(10.))
                        .text_color(content(0.5))
                        .child(label),
                )
            })
            .when(entry.removable() && !confirming, |d| {
                let id = id.clone();
                d.child(
                    tool(
                        SharedString::from(format!("agenda-event-remove-{id}")),
                        "Remove event",
                        "trash",
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.deleting = Some(id.clone());
                        cx.notify();
                    })),
                )
            })
            .when(entry.removable() && confirming, |d| {
                let id = id.clone();
                d.child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            text_button(
                                SharedString::from(format!("agenda-event-keep-{id}")),
                                "Keep",
                                0.7,
                                true,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.deleting = None;
                                cx.notify();
                            })),
                        )
                        .child(
                            text_button(
                                SharedString::from(format!("agenda-event-delete-{id}")),
                                "Delete",
                                1.,
                                idle,
                            )
                            .text_color(rgb(0xf87171))
                            .on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.delete_event(id.clone(), cx)
                                }),
                            ),
                        ),
                )
            })
    }

    fn page_tab(
        &self,
        id: &'static str,
        label: &'static str,
        page: Page,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let selected = self.page == page;
        div()
            .id(id)
            .role(Role::Tab)
            .aria_label(label)
            .h(px(24.))
            .px(px(12.))
            .flex()
            .items_center()
            .rounded(px(6.))
            .text_size(px(12.))
            .cursor_pointer()
            .text_color(content(if selected { 1. } else { 0.5 }))
            .when(selected, |d| d.bg(content(0.1)))
            .when(!selected, |d| {
                d.hover(|s| s.bg(content(0.05)).text_color(content(1.)))
            })
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| this.set_page(page, cx)))
    }

    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        let idle = self.busy.is_none();
        let agenda = self.page == Page::Agenda;
        div()
            .h(px(36.))
            .w_full()
            .px(px(8.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(2.))
            .border_b_1()
            .border_color(content(0.07))
            .child(self.page_tab("agenda-page-agenda", "Agenda", Page::Agenda, cx))
            .child(self.page_tab("agenda-page-setup", "Setup", Page::Setup, cx))
            .child(div().flex_1())
            .when(agenda, |d| {
                d.child(
                    text_button(
                        "agenda-import",
                        if self.busy == Some(Busy::Import) {
                            "Importing…"
                        } else {
                            "Import .ics"
                        },
                        0.7,
                        idle,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.import_ics(cx))),
                )
                .child(
                    text_button(
                        "agenda-export",
                        if self.busy == Some(Busy::Export) {
                            "Exporting…"
                        } else {
                            "Export .ics"
                        },
                        0.7,
                        idle,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.export_ics(cx))),
                )
                .child(
                    tool(
                        "agenda-refresh",
                        "Refresh agenda",
                        if self.loading { "loader" } else { "refresh-cw" },
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if !this.loading {
                            this.load_agenda(cx);
                        }
                    })),
                )
                .child(
                    primary("agenda-new", "New event", !self.form_open).on_click(cx.listener(
                        |this, _, window, cx| {
                            if !this.form_open {
                                this.open_form(cx);
                                window.focus(&this.title.focus_handle(cx), cx);
                            }
                        },
                    )),
                )
            })
            .into_any_element()
    }

    fn agenda_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let today = Local::now().date_naive();
        let days = group_days(&self.events);
        let waiting = self.loading && self.refreshed.is_none();
        let local_only = self.config_loaded && self.config.mailflare_url.trim().is_empty();
        let mut column = div()
            .w_full()
            .max_w(px(COLUMN))
            .px(px(20.))
            .py(px(20.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(content(1.))
                            .child("Next 30 days"),
                    )
                    .child(hint(
                        "Reminders and time blocks kept on this computer. MonoBOT never invites anyone.",
                    )),
            );
        if let Some(notice) = self.notice.clone() {
            column = column.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(12.))
                    .text_color(content(0.7))
                    .child(icon("check", 14.).text_color(content(0.7)))
                    .child(notice),
            );
        }
        if let Some(error) = self.error.clone() {
            column = column.child(alert(error));
        }
        if self.form_open {
            column = column.child(self.event_form(window, cx));
        }
        if waiting {
            column = column.child(hint("Loading your agenda…"));
        } else if days.is_empty() && !self.form_open {
            column = column.child(
                div()
                    .py(px(40.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(10.))
                    .child(icon("clock", 24.).text_color(content(0.3)))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(content(0.8))
                            .child("Nothing scheduled in the next 30 days"),
                    )
                    .child(
                        div()
                            .max_w(px(420.))
                            .text_size(px(12.))
                            .line_height(px(19.))
                            .text_color(content(0.5))
                            .text_center()
                            .child(if local_only {
                                "Add a reminder or time block, paste a calendar file with Import .ics, or connect Mailflare in Setup to see your mail schedule here."
                            } else {
                                "Add a reminder or time block, or paste a calendar file with Import .ics."
                            }),
                    ),
            );
        }
        for day in days {
            let heading = day
                .date
                .map(|date| day_heading(date, today))
                .unwrap_or_else(|| "Unknown date".into());
            let is_today = day.date == Some(today);
            let rows = day
                .entries
                .iter()
                .map(|entry| self.event_row(entry, cx))
                .collect::<Vec<_>>();
            column = column.child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .pb(px(6.))
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if is_today {
                                accent_color()
                            } else {
                                content(0.5)
                            })
                            .child(heading),
                    )
                    .children(rows),
            );
        }
        div()
            .id("agenda-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(div().w_full().flex().justify_center().child(column))
            .into_any_element()
    }

    fn event_form(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let busy = self.busy == Some(Busy::Event);
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(16.))
            .border_1()
            .border_color(content(0.1))
            .child(labelled("Title", &self.title, window, cx))
            .child(
                div()
                    .flex()
                    .gap(px(12.))
                    .child(div().flex_1().child(labelled("Starts", &self.start, window, cx)))
                    .child(div().flex_1().child(labelled("Ends", &self.end, window, cx))),
            )
            .child(labelled("Notes (optional)", &self.notes, window, cx))
            .child(hint(format!(
                "Times are on this computer's clock ({}). This is a reminder or time block for you alone: no invitation is sent to anyone.",
                local_zone()
            )))
            .when_some(self.form_error.clone(), |d, error| d.child(alert(error)))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        text_button("agenda-form-cancel", "Cancel", 0.7, !busy).on_click(
                            cx.listener(|this, _, _, cx| {
                                if this.busy.is_none() {
                                    this.form_open = false;
                                    cx.notify();
                                }
                            }),
                        ),
                    )
                    .child(
                        primary(
                            "agenda-form-add",
                            if busy { "Adding…" } else { "Add to agenda" },
                            !busy,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.add_event(cx))),
                    ),
            )
            .into_any_element()
    }

    // Setup --------------------------------------------------------------------------

    fn secret_field(
        &self,
        kind: &'static str,
        label: &'static str,
        help: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let secret = self.secret(kind);
        let confirming = self.removing == Some(kind);
        let idle = self.busy.is_none();
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(labelled(label, &secret.input, window, cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .min_h(px(24.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(hint(if secret.saved {
                                format!("Saved. {help} It is never shown again; leave this blank to keep it.")
                            } else {
                                format!("{help} It is never shown again once saved; leave it blank later to keep it.")
                            })),
                    )
                    .when(!confirming, |d| {
                        d.child(
                            text_button(
                                SharedString::from(format!("agenda-remove-{kind}")),
                                "Remove saved",
                                0.55,
                                idle,
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.removing = Some(kind);
                                cx.notify();
                            })),
                        )
                    })
                    .when(confirming, |d| {
                        d.child(
                            div()
                                .text_size(px(11.))
                                .text_color(content(0.7))
                                .child("Delete the saved value?"),
                        )
                        .child(
                            text_button(
                                SharedString::from(format!("agenda-remove-cancel-{kind}")),
                                "Keep",
                                0.7,
                                true,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.removing = None;
                                cx.notify();
                            })),
                        )
                        .child(
                            text_button(
                                SharedString::from(format!("agenda-remove-confirm-{kind}")),
                                "Delete",
                                1.,
                                idle,
                            )
                            .text_color(rgb(0xf87171))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.remove_secret(kind, cx)
                            })),
                        )
                    }),
            )
            .into_any_element()
    }

    fn route_card(
        &self,
        ix: usize,
        options: &[(String, String)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let route = &self.routes[ix];
        let alias = route.alias.read(cx).content.to_string();
        let senders = route.senders.read(cx).content.to_string();
        let touched = self.validated || !alias.trim().is_empty() || !senders.trim().is_empty();
        let issue = route_issue(&alias, &senders, &route.bot_id, route.enabled)
            .filter(|_| touched || route.enabled);
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(14.))
            .border_1()
            .border_color(content(0.1))
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(8.))
                    .child(div().flex_1().child(labelled(
                        "Address this bot receives mail at",
                        &route.alias,
                        window,
                        cx,
                    )))
                    .child(
                        tool(
                            SharedString::from(format!("agenda-route-remove-{ix}")),
                            "Remove route",
                            "trash",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if ix < this.routes.len() {
                                this.routes.remove(ix);
                                this.error = None;
                                cx.notify();
                            }
                        })),
                    ),
            )
            .child(labelled(
                "Allowed senders (full addresses, separated by commas)",
                &route.senders,
                window,
                cx,
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(content(LABEL))
                            .child("Bot that handles it"),
                    )
                    .child(if options.is_empty() {
                        hint("No bots yet. Create one in Agents, then pick it here.")
                            .into_any_element()
                    } else {
                        div()
                            .id(SharedString::from(format!("agenda-route-bots-{ix}")))
                            .role(Role::RadioGroup)
                            .aria_label("Bot that handles it")
                            .flex()
                            .flex_wrap()
                            .gap(px(4.))
                            .children(options.iter().map(|(id, name)| {
                                let id = id.clone();
                                choice(
                                    SharedString::from(format!("agenda-route-{ix}-bot-{id}")),
                                    name,
                                    route.bot_id == id,
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(route) = this.routes.get_mut(ix) {
                                        route.bot_id = id.clone();
                                        cx.notify();
                                    }
                                }))
                            }))
                            .into_any_element()
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(content(1.))
                                    .child("Handle this mail automatically"),
                            )
                            .child(hint(
                                "Off until you turn it on. Once on, the bot reads and acts on what these senders write, so list only people you trust.",
                            )),
                    )
                    .child(
                        switch(
                            SharedString::from(format!("agenda-route-enabled-{ix}")),
                            "Handle this mail automatically",
                            route.enabled,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(route) = this.routes.get_mut(ix) {
                                route.enabled = !route.enabled;
                                this.error = None;
                                cx.notify();
                            }
                        })),
                    ),
            )
            .when_some(issue, |d, issue| d.child(alert(issue)))
            .into_any_element()
    }

    fn setup_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if !self.config_loaded {
            let failed = !self.config_loading;
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.))
                .px(px(20.))
                .when(!failed, |d| d.child(hint("Loading settings…")))
                .when(failed, |d| {
                    d.when_some(self.error.clone(), |d, error| d.child(alert(error)))
                        .child(
                            text_button("agenda-setup-retry", "Try again", 1., true).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.error = None;
                                    this.load_config(cx);
                                }),
                            ),
                        )
                })
                .into_any_element();
        }
        let provider = self.send_provider.clone();
        let options = self.bot_options();
        let pending = self.pending(cx);
        let idle = self.busy.is_none();
        let saving = self.busy == Some(Busy::Save);

        let mailflare = section(
            "Mailflare",
            "A Mailflare server you host on Cloudflare lets bots read your mail and schedule through MCP. MonoBOT connects to the address below with the token you save here.",
        )
        .child(labelled("Server URL", &self.mailflare_url, window, cx))
        .child(self.secret_field(
            MAILFLARE,
            "Access token",
            "The token your Mailflare server expects.",
            window,
            cx,
        ))
        .child(
            div()
                .flex()
                .items_start()
                .gap(px(12.))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(content(1.))
                                .child("Turn on the Mailflare connector"),
                        )
                        .child(hint(
                            "Needs the URL and a saved token. It does not give any bot access by itself: a bot can use Mailflare only after you add it to that bot's tools.",
                        )),
                )
                .child(
                    switch(
                        "agenda-mailflare-enabled",
                        "Turn on the Mailflare connector",
                        self.mailflare_on,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.mailflare_on = !this.mailflare_on;
                        this.error = None;
                        this.notice = None;
                        cx.notify();
                    })),
                ),
        );

        let sending = section(
            "Sending",
            "How a reply leaves MonoBOT when you send it from the Email tab.",
        )
        .child(
            div()
                .id("agenda-send-provider")
                .role(Role::RadioGroup)
                .aria_label("Sending provider")
                .flex()
                .flex_wrap()
                .gap(px(4.))
                .children(
                    [
                        ("smtp", "SMTP (your mail account)"),
                        ("resend", "Resend"),
                        ("cloudflare", "Cloudflare Email"),
                    ]
                    .into_iter()
                    .map(|(id, label)| {
                        choice(
                            SharedString::from(format!("agenda-send-{id}")),
                            label,
                            provider == id,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.send_provider = id.into();
                            this.error = None;
                            this.notice = None;
                            cx.notify();
                        }))
                    }),
                ),
        )
        .when(provider == "smtp", |d| {
            d.child(hint(
                "Replies go out through the mail account you connected in the Email tab.",
            ))
        })
        .when(provider == "resend", |d| {
            d.child(self.secret_field(
                "resend",
                "Resend API key",
                "Create it on Resend's API Keys page; sending access is enough.",
                window,
                cx,
            ))
        })
        .when(provider == "cloudflare", |d| {
            d.child(labelled(
                "Cloudflare account ID",
                &self.cloudflare_account,
                window,
                cx,
            ))
            .child(self.secret_field(
                "cloudflare",
                "Cloudflare API token",
                "A token that may send email for that account.",
                window,
                cx,
            ))
        });

        let inbound = section(
            "Inbound mail",
            "Mail for your own domain arrives at a Worker you host. MonoBOT fetches it from there with the secret below; nothing is pushed into the app.",
        )
        .child(labelled("Worker URL", &self.inbound_url, window, cx))
        .child(self.secret_field(
            "inbound",
            "Shared secret",
            "The same secret your inbound Worker checks.",
            window,
            cx,
        ));

        let cards = (0..self.routes.len())
            .map(|ix| self.route_card(ix, &options, window, cx))
            .collect::<Vec<_>>();
        let routes = section(
            "Bot routes",
            "Give a bot its own address on your domain, such as support@your-domain.com, for mail you want it to take on.",
        )
        .child(
            div()
                .flex()
                .items_start()
                .gap(px(10.))
                .p(px(12.))
                .bg(content(0.05))
                .child(icon("shield-alert", 14.).text_color(content(0.7)))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .text_size(px(12.))
                        .line_height(px(19.))
                        .text_color(content(0.7))
                        .child("A bot receives a message only when all three hold: the route is on, the sender is on its allowed list, and the Worker reports SPF/DMARC checks passed. Verify Cloudflare's authentication headers with real mail before enabling a route.")
                        .child("Everything else stays in your inbox and no bot sees it. Routes start off, and only you can turn one on."),
                ),
        )
        .children(cards)
        .child(
            div().child(
                text_button("agenda-route-add", "Add route", 1., true)
                    .border_1()
                    .border_color(content(0.1))
                    .on_click(cx.listener(|this, _, _, cx| this.add_route(cx))),
            ),
        );

        let status = if let Some(error) = self.error.clone() {
            alert(error)
        } else if let Some(notice) = self.notice.clone() {
            div()
                .text_size(px(12.))
                .text_color(content(0.7))
                .child(notice)
        } else if pending {
            hint("Unsaved changes")
        } else {
            div()
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("agenda-setup-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div().w_full().flex().justify_center().child(
                            div()
                                .w_full()
                                .max_w(px(COLUMN))
                                .px(px(20.))
                                .pt(px(20.))
                                .flex()
                                .flex_col()
                                .child(mailflare)
                                .child(sending)
                                .child(inbound)
                                .child(routes),
                        ),
                    ),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(content(0.07))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(COLUMN))
                            .px(px(20.))
                            .py(px(10.))
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(div().min_w_0().flex_1().child(status))
                            .child(
                                primary(
                                    "agenda-setup-save",
                                    if saving { "Saving…" } else { "Save" },
                                    pending && idle,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        if this.pending(cx) {
                                            this.save(cx)
                                        }
                                    },
                                )),
                            ),
                    ),
            )
            .into_any_element()
    }
}

impl gpui::Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.page {
            Page::Agenda => self.agenda_page(window, cx),
            Page::Setup => self.setup_page(window, cx),
        };
        div()
            .id("inbox-agenda")
            .role(Role::Group)
            .aria_label("Agenda")
            .size_full()
            .flex()
            .flex_col()
            .child(self.header(cx))
            .child(div().flex_1().min_h_0().min_w_0().child(body))
    }
}
