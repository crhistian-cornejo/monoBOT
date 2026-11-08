//! The Agenda as a calendar: a month or a week laid out by day, the chosen day's events below
//! it, and the panel that follows a calendar by its private iCal address (Google Calendar's
//! «Secret address in iCal format»), kept in the system's store of secrets and read only.
use super::view::{alert, hint, labelled, primary, text_button, tool};
use super::*;
use gpui::{
    AnyElement, Div, FontWeight, Role, SharedString, Stateful, Window, div, prelude::*, px,
};

/// How many events a month's day shows before «+N more».
const DAY_CHIPS: usize = 3;

fn segment(id: &'static str, label: &'static str, chosen: bool) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.to_owned())
        .role(Role::Tab)
        .aria_label(label)
        .px(px(10.))
        .py(px(4.))
        .text_size(px(12.))
        .cursor_pointer()
        .text_color(content(if chosen { 1. } else { 0.55 }))
        .when(chosen, |d| d.bg(content(0.1)))
        .when(!chosen, |d| d.hover(|s| s.bg(content(0.05))))
        .child(label)
}

/// One event in a day's cell: its time (none for a whole day) and its title.
fn chip(entry: &Entry) -> Div {
    let time = match entry.start {
        Some(Moment::Timed(start)) => Some(start.format("%H:%M").to_string()),
        _ => None,
    };
    let followed = entry.event.source == "feed";
    div()
        .flex()
        .items_center()
        .gap(px(4.))
        .min_w_0()
        .px(px(4.))
        .rounded(px(3.))
        .when(time.is_none(), |d| d.bg(content(0.07)))
        .text_size(px(11.))
        .line_height(px(16.))
        .child(
            div()
                .size(px(5.))
                .flex_shrink_0()
                .rounded_full()
                .bg(if followed {
                    content(0.45)
                } else {
                    accent_color()
                }),
        )
        .when_some(time, |d, time| {
            d.child(div().flex_shrink_0().text_color(content(0.5)).child(time))
        })
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_color(content(0.9))
                .child(entry.event.title.clone()),
        )
}

impl Workspace {
    /// ‹ Today › and the span's title, with Week and Month.
    fn calendar_bar(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                tool("agenda-prev", "Previous", "chevron-left")
                    .on_click(cx.listener(|this, _, _, cx| this.shift(-1, cx))),
            )
            .child(
                text_button("agenda-today", "Today", 0.8, true)
                    .on_click(cx.listener(|this, _, _, cx| this.shift(0, cx))),
            )
            .child(
                tool("agenda-next", "Next", "chevron-right")
                    .on_click(cx.listener(|this, _, _, cx| this.shift(1, cx))),
            )
            .child(
                div()
                    .id("agenda-title")
                    .debug_selector(|| "agenda-title".into())
                    .ml(px(6.))
                    .flex_1()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(content(1.))
                    .child(span_title(self.span, self.anchor)),
            )
            .when(self.loading, |d| d.child(hint("Loading…")))
            .child(
                div()
                    .id("agenda-span")
                    .role(Role::TabList)
                    .flex()
                    .gap(px(2.))
                    .child(
                        segment("agenda-span-week", "Week", self.span == Span::Week)
                            .on_click(cx.listener(|this, _, _, cx| this.set_span(Span::Week, cx))),
                    )
                    .child(
                        segment("agenda-span-month", "Month", self.span == Span::Month)
                            .on_click(cx.listener(|this, _, _, cx| this.set_span(Span::Month, cx))),
                    ),
            )
    }

    /// The span's days in a grid: seven columns, a row per week.
    fn calendar_grid(&self, days: &[Day], cx: &mut Context<Self>) -> Stateful<Div> {
        let today = Local::now().date_naive();
        let shown = grid_days(self.span, self.anchor);
        let week = self.span == Span::Week;
        let names =
            div().flex().children(
                ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].map(|name| {
                    div()
                        .flex_1()
                        .px(px(6.))
                        .pb(px(4.))
                        .text_size(px(11.))
                        .text_color(content(0.45))
                        .child(name)
                }),
            );
        let rows = shown.chunks(7).map(|row| {
            div().flex().children(row.iter().map(|day| {
                let day = *day;
                let entries: Vec<&Entry> = days
                    .iter()
                    .find(|d| d.date == Some(day))
                    .map(|d| d.entries.iter().collect())
                    .unwrap_or_default();
                let outside = !week && day.month() != self.anchor.month();
                let chosen = day == self.selected;
                let limit = if week { usize::MAX } else { DAY_CHIPS };
                let extra = entries.len().saturating_sub(limit);
                div()
                    .id(SharedString::from(format!("agenda-day-{day}")))
                    .debug_selector(move || format!("agenda-day-{day}"))
                    .role(Role::Button)
                    .aria_label(day.format("%A %-d %B").to_string())
                    .flex_1()
                    .min_w_0()
                    .min_h(px(if week { 220. } else { 88. }))
                    .p(px(4.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .border_1()
                    .border_color(if chosen {
                        accent_color()
                    } else {
                        content(0.06)
                    })
                    .when(outside, |d| d.bg(content(0.02)))
                    .cursor_pointer()
                    .hover(|s| s.bg(content(0.04)))
                    .child(
                        div().flex().child(
                            div()
                                .px(px(5.))
                                .rounded_full()
                                .text_size(px(11.))
                                .when(day == today, |d| {
                                    d.bg(accent_color()).text_color(gpui::white())
                                })
                                .when(day != today, |d| {
                                    d.text_color(content(if outside { 0.3 } else { 0.7 }))
                                })
                                .child(if week {
                                    day.format("%-d %b").to_string()
                                } else {
                                    day.format("%-d").to_string()
                                }),
                        ),
                    )
                    .children(entries.iter().take(limit).map(|entry| chip(entry)))
                    .when(extra > 0, |d| {
                        d.child(
                            div()
                                .px(px(4.))
                                .text_size(px(11.))
                                .text_color(content(0.5))
                                .child(format!("+{extra} more")),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.select_day(day, cx)))
            }))
        });
        div()
            .id("agenda-grid")
            .debug_selector(|| "agenda-grid".into())
            .flex()
            .flex_col()
            .child(names)
            .children(rows)
    }

    /// The chosen day's events, to read and remove.
    fn day_list(&self, days: &[Day], cx: &mut Context<Self>) -> Stateful<Div> {
        let today = Local::now().date_naive();
        let entries: Vec<&Entry> = days
            .iter()
            .find(|d| d.date == Some(self.selected))
            .map(|d| d.entries.iter().collect())
            .unwrap_or_default();
        div()
            .id("agenda-day-list")
            .flex()
            .flex_col()
            .child(
                div()
                    .pb(px(6.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if self.selected == today {
                        accent_color()
                    } else {
                        content(0.6)
                    })
                    .child(day_heading(self.selected, today)),
            )
            .when(entries.is_empty(), |d| {
                d.child(hint(if self.feed.connected {
                    "Nothing on this day."
                } else {
                    "Nothing on this day. Add an event, or follow your Google Calendar with Calendar."
                }))
            })
            .children(entries.into_iter().map(|entry| self.event_row(entry, cx)))
    }

    /// Following a calendar by its private iCal address.
    fn feed_panel(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let state = &self.feed;
        let status = if state.connected {
            let when = if state.refreshed_at > 0 {
                let ago = (Utc::now().timestamp_millis() - state.refreshed_at) / 60_000;
                match ago {
                    0 => "just now".to_owned(),
                    1 => "a minute ago".to_owned(),
                    n if n < 120 => format!("{n} minutes ago"),
                    n => format!("{} hours ago", n / 60),
                }
            } else {
                "not yet".to_owned()
            };
            let mut line = format!(
                "Following a calendar · updated {when} · {} events",
                state.events
            );
            if state.simplified > 0 {
                line.push_str(&format!(
                    " · {} repeating event{} shown at its first time only",
                    state.simplified,
                    if state.simplified == 1 { "" } else { "s" }
                ));
            }
            Some(line)
        } else {
            None
        };
        div()
            .id("agenda-feed")
            .debug_selector(|| "agenda-feed".into())
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(16.))
            .border_1()
            .border_color(content(0.1))
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(content(1.))
                    .child("Follow your Google Calendar"),
            )
            .child(hint(
                "In Google Calendar on the web: Settings › your calendar › Integrate calendar › «Secret address in iCal format». Copy it and paste it here. It works like a password, so MonoBOT keeps it in your system's keychain. Its events show here read only and refresh every 15 minutes; MonoBOT never changes that calendar.",
            ))
            .when_some(status, |d, status| {
                d.child(
                    div()
                        .text_size(px(12.))
                        .text_color(content(0.7))
                        .child(status),
                )
            })
            .when(!state.error.is_empty(), |d| {
                d.child(alert(format!("Last update failed: {}", state.error)))
            })
            .child(labelled(
                if state.connected {
                    "Replace with another address"
                } else {
                    "Secret iCal address"
                },
                &self.feed_input,
                window,
                cx,
            ))
            .when_some(self.feed_error.clone(), |d, error| d.child(alert(error)))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .when(state.connected, |d| {
                        d.child(
                            text_button("agenda-feed-refresh", "Update now", 0.7, !self.feed_busy)
                                .on_click(cx.listener(|this, _, _, cx| this.refresh_feed(cx))),
                        )
                        .child(
                            text_button("agenda-feed-remove", "Stop following", 0.7, !self.feed_busy)
                                .on_click(cx.listener(|this, _, _, cx| this.save_feed(true, cx))),
                        )
                    })
                    .child(
                        text_button("agenda-feed-close", "Close", 0.6, !self.feed_busy).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.feed_open = false;
                                this.feed_error = None;
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        primary(
                            "agenda-feed-save",
                            if self.feed_busy { "Fetching…" } else { "Follow calendar" },
                            !self.feed_busy,
                        )
                        .debug_selector(|| "agenda-feed-save".into())
                        .on_click(cx.listener(|this, _, _, cx| this.save_feed(false, cx))),
                    ),
            )
            .into_any_element()
    }

    /// The Agenda page: the bar, what was said last, the open panels, the grid and the day.
    pub(super) fn agenda_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let days = group_days(&self.events);
        let mut column = div()
            .w_full()
            .max_w(px(1100.))
            .px(px(20.))
            .py(px(16.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(self.calendar_bar(cx));
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
        if self.feed_open {
            column = column.child(self.feed_panel(window, cx));
        }
        if self.form_open {
            column = column.child(self.event_form(window, cx));
        }
        column = column
            .child(self.calendar_grid(&days, cx))
            .child(self.day_list(&days, cx));
        div()
            .id("agenda-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(div().w_full().flex().justify_center().child(column))
            .into_any_element()
    }
}
