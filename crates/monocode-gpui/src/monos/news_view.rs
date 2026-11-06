//! The Principal's News and Bots tabs (BotNewsList.tsx, BotDelegations.tsx):
//! the agents waiting on the user, every report the bots posted on their
//! own (newest first, filtered by bot), and the bots it handed work to with
//! what each was asked last. The feed and delegations are core's.
use super::panels::empty_state;
use super::*;
use gpui::{ClipboardItem, FontWeight};
use monocode_core::mono::{delegations, news, outcomes::ReportStatus};

/// How tall a report shows before "Show more".
const CLAMP: f32 = 140.;

/// "14:05" today, "Nov 3" before (when).
fn when(at: u64) -> String {
    let Some(date) = chrono::DateTime::from_timestamp_millis(at as i64) else {
        return String::new();
    };
    let date = date.with_timezone(&chrono::Local);
    if date.date_naive() == chrono::Local::now().date_naive() {
        date.format("%H:%M").to_string()
    } else {
        date.format("%b %-d").to_string()
    }
}

/// The report's badge colours (STATUS_STYLE).
fn status_colors(status: ReportStatus) -> (gpui::Hsla, gpui::Hsla) {
    match status {
        ReportStatus::Done => (
            rgb(0x10b981).into(),
            gpui::Hsla::from(rgb(0x10b981)).opacity(0.1),
        ),
        ReportStatus::Blocked => (
            rgb(0xf87171).into(),
            gpui::Hsla::from(rgb(0xf87171)).opacity(0.1),
        ),
        _ => (accent_color(), accent_color().opacity(0.1)),
    }
}

impl Monos {
    fn status_of(&self, id: &str, cx: &App) -> &'static str {
        match self.chats.get(id).map(|chat| chat.read(cx).mono_status()) {
            Some("working") => "working",
            Some("needs-you") => "needs-you",
            _ => "idle",
        }
    }

    /// A bot's face, as the rail draws it.
    fn face(&self, id: &str, side: f32, cx: &App) -> AnyElement {
        match self.roster.get(id) {
            Some(mono) => {
                avatar::avatar(&mono.mascot, mono.color, self.status_of(id, cx), side, true)
            }
            None => div().size(px(side)).into_any_element(),
        }
    }

    /// News: who waits on the user, then the reports.
    pub(super) fn news_list(&mut self, cx: &mut Context<Self>) -> AnyElement {
        // Reading the feed marks it seen.
        if self.has_unseen_news() {
            self.mark_news_seen(cx);
        }
        let waiting: Vec<(String, String)> = self
            .roster
            .monos
            .iter()
            .filter(|m| m.is_bot() || m.is_coding())
            .filter(|m| self.status_of(&m.id, cx) == "needs-you")
            .map(|m| (m.id.clone(), m.label()))
            .collect();
        let feed = self.news_feed().clone();
        if feed.items.is_empty() && waiting.is_empty() {
            return empty_state(
                "bell",
                "When an agent waits for your approval, or a bot's habit has something to tell you, like a digest, a budget alert or a finished report, it shows up here.",
            );
        }
        let mut list = div()
            .id("bot-news")
            .debug_selector(|| "bot-news".into())
            .size_full()
            .overflow_y_scroll()
            .px_2()
            .py_2()
            .flex()
            .flex_col()
            .gap_3();
        if !waiting.is_empty() {
            list = list.child(
                div()
                    .child(
                        div()
                            .px_2()
                            .pb_1()
                            .text_size(px(11.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(accent_color())
                            .child("NEEDS YOU"),
                    )
                    .children(waiting.into_iter().enumerate().map(|(ix, (id, name))| {
                        div()
                            .id(("news-waiting", ix))
                            .role(Role::Button)
                            .aria_label(format!("Open {name}"))
                            .flex()
                            .items_center()
                            .gap_2()
                            .rounded_md()
                            .px_2()
                            .py(px(6.))
                            .hover(|s| s.bg(content(0.05)))
                            .child(div().size(px(6.)).rounded_full().bg(accent_color()))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(content(0.85))
                                    .child(name),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(content(0.55))
                                    .child("Waiting for your input"),
                            )
                            .on_click(
                                cx.listener(move |this, _, window, cx| this.open(&id, window, cx)),
                            )
                    })),
            );
        }
        if feed.items.is_empty() {
            return list.into_any_element();
        }
        let only = feed.filter(self.news.only.as_deref()).map(str::to_owned);
        if feed.shows_filters() {
            let all = div()
                .id("news-filter-all")
                .role(Role::Button)
                .aria_label(format!("All · {}", feed.items.len()))
                .aria_selected(only.is_none())
                .h(px(24.))
                .px(px(10.))
                .flex()
                .flex_shrink_0()
                .items_center()
                .gap(px(6.))
                .rounded_full()
                .border_1()
                .text_size(px(12.))
                .map(|d| filter_look(d, only.is_none()))
                .child("All")
                .child(
                    div()
                        .text_color(content(0.45))
                        .child(feed.items.len().to_string()),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.news.only = None;
                    cx.notify();
                }));
            list = list.child(
                div()
                    .id("news-filters")
                    .role(Role::Toolbar)
                    .aria_label("Show news from")
                    .flex()
                    .gap_1()
                    .overflow_x_scroll()
                    .child(all)
                    .children(feed.senders().into_iter().enumerate().map(|(ix, sender)| {
                        let active = only.as_deref() == Some(sender.mono_id.as_str());
                        let id = sender.mono_id.clone();
                        div()
                            .id(("news-filter", ix))
                            .role(Role::Button)
                            .aria_label(format!("{} · {}", sender.name, sender.count))
                            .aria_selected(active)
                            .h(px(24.))
                            .pl_1()
                            .pr_2()
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap(px(6.))
                            .rounded_full()
                            .border_1()
                            .text_size(px(12.))
                            .map(|d| filter_look(d, active))
                            .child(self.face(&sender.mono_id, 14., cx))
                            .child(sender.count.to_string())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.news.only = (!active).then(|| id.clone());
                                cx.notify();
                            }))
                    })),
            );
        }
        list = list.children(feed.shown(only.as_deref()).into_iter().map(|item| {
            let open = self.news.expanded.contains(&item.id);
            let needs_you = item.status == Some(ReportStatus::NeedsAttention);
            let bot = item.mono_id.clone();
            let chat = item.mono_id.clone();
            let toggle = item.id.clone();
            let text = item.text.clone();
            let group: SharedString = format!("news-item-{}", item.id).into();
            let mut body = div().w_full().min_w_0().flex().flex_col();
            for (ix, row) in crate::markdown_model::parse_full(&item.text)
                .blocks
                .iter()
                .flat_map(|b| crate::markdown::rows(&b.block))
                .enumerate()
            {
                body = body.child(div().w_full().min_w_0().flex_shrink_0().child(
                    crate::markdown::body(
                        &row,
                        format!("news-{}-{ix}", item.id).into(),
                        crate::markdown::Tone::Normal,
                        None,
                        std::time::Instant::now(),
                    ),
                ));
            }
            let long = item.text.lines().count() > 6 || item.text.len() > 600;
            div()
                .id(SharedString::from(format!("news-{}", item.id)))
                .debug_selector(|| "bot-news-item".into())
                .group(group.clone())
                .rounded_lg()
                .border_1()
                .border_color(content(0.08))
                .bg(content(0.02))
                .px_3()
                .pt(px(10.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(12.))
                        .text_color(content(0.5))
                        .child(
                            div()
                                .id(SharedString::from(format!("news-bot-{}", item.id)))
                                .role(Role::Button)
                                .aria_label(format!("Open {}", item.bot_name))
                                .flex()
                                .min_w_0()
                                .items_center()
                                .gap_2()
                                .hover(|s| s.text_color(content(0.8)))
                                .child(
                                    div()
                                        .relative()
                                        .flex_shrink_0()
                                        .child(self.face(&item.mono_id, 18., cx))
                                        .when(!item.seen, |d| {
                                            d.child(
                                                div()
                                                    .absolute()
                                                    .left(px(-4.))
                                                    .top(px(-2.))
                                                    .size(px(6.))
                                                    .rounded_full()
                                                    .bg(accent_color()),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(content(0.85))
                                        .child(item.bot_name.clone()),
                                )
                                .child(div().min_w_0().truncate().child(item.source.clone()))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open(&bot, window, cx)
                                })),
                        )
                        .children(item.status.and_then(|status| {
                            news::status_label(status).map(|label| {
                                let (fg, bg) = status_colors(status);
                                div()
                                    .flex_shrink_0()
                                    .rounded_sm()
                                    .px(px(6.))
                                    .bg(bg)
                                    .text_color(fg)
                                    .text_size(px(11.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(label)
                            })
                        }))
                        .child(
                            div()
                                .ml_auto()
                                .flex_shrink_0()
                                .text_color(content(0.4))
                                .child(when(item.at)),
                        ),
                )
                .child(
                    div()
                        .pl(px(26.))
                        .pt(px(6.))
                        .text_size(px(13.))
                        .line_height(px(20.))
                        .child(
                            div()
                                .w_full()
                                .when(long && !open, |d| d.max_h(px(CLAMP)).overflow_hidden())
                                .child(body),
                        )
                        .when(long, |d| {
                            d.child(
                                div()
                                    .id(SharedString::from(format!("news-more-{}", item.id)))
                                    .role(Role::Button)
                                    .aria_expanded(open)
                                    .pt_1()
                                    .text_size(px(12.))
                                    .text_color(content(0.55))
                                    .hover(|s| s.text_color(content(1.)))
                                    .child(if open { "Show less" } else { "Show more" })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !this.news.expanded.remove(&toggle) {
                                            this.news.expanded.insert(toggle.clone());
                                        }
                                        cx.notify();
                                    })),
                            )
                        }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .pb(px(6.))
                        .pl(px(20.))
                        .pt(px(2.))
                        .child(
                            div()
                                .id(SharedString::from(format!("news-open-{}", item.id)))
                                .role(Role::Button)
                                .aria_label(item.open_label())
                                .h(px(24.))
                                .px(px(6.))
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .rounded_md()
                                .text_size(px(12.))
                                .text_color(if needs_you {
                                    accent_color()
                                } else {
                                    content(0.55)
                                })
                                .hover(|s| s.bg(content(0.08)))
                                .child(icon("message-square", 14.))
                                .child(item.open_label())
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open(&chat, window, cx)
                                })),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("news-copy-{}", item.id)))
                                .role(Role::Button)
                                .aria_label("Copy report")
                                .size(px(24.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_md()
                                .text_color(content(0.55))
                                .opacity(0.)
                                .group_hover(group, |s| s.opacity(1.))
                                .hover(|s| s.bg(content(0.08)))
                                .child(icon("response-copy", 14.))
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
                                }),
                        ),
                )
        }));
        list.into_any_element()
    }

    /// Bots: one row per bot the Principal asked, what it asked last and
    /// what that bot is doing now; a row opens the bot's chat.
    pub(super) fn delegations_list(&mut self, mono: &Mono, cx: &mut Context<Self>) -> AnyElement {
        let all = self.delegations().clone();
        let rows: Vec<(delegations::Delegation, Mono)> =
            delegations::latest_by_agent(&all, &mono.id)
                .into_iter()
                .filter_map(|entry| Some((entry.clone(), self.roster.get(&entry.to)?.clone())))
                .collect();
        if rows.is_empty() {
            return empty_state(
                "users",
                &format!(
                    "When {} hands part of a job to one of your bots, the bot shows up here with what it was asked and what it is doing.",
                    mono.label()
                ),
            );
        }
        div()
            .id("bot-delegations")
            .debug_selector(|| "bot-delegations".into())
            .size_full()
            .overflow_y_scroll()
            .px_2()
            .py_2()
            .flex()
            .flex_col()
            .gap(px(1.))
            .children(rows.into_iter().enumerate().map(|(ix, (entry, bot))| {
                let status = self.status_of(&bot.id, cx);
                let (label, tone) = match status {
                    "needs-you" => ("Needs you", accent_color()),
                    "working" => ("Working", content(0.6)),
                    _ => ("Idle", content(0.4)),
                };
                let id = bot.id.clone();
                div()
                    .id(("bot-delegation", ix))
                    .role(Role::Button)
                    .aria_label(format!("Open {}", bot.label()))
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .rounded_md()
                    .px_2()
                    .py_2()
                    .hover(|s| s.bg(content(0.05)))
                    .child(div().mt(px(2.)).flex_shrink_0().child(avatar::avatar(
                        &bot.mascot,
                        bot.color,
                        status,
                        16.,
                        true,
                    )))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .gap(px(2.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_size(px(12.))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(content(0.85))
                                            .child(bot.label()),
                                    )
                                    .child(div().min_w_0().truncate().text_color(tone).child(label))
                                    .child(
                                        div()
                                            .ml_auto()
                                            .flex_shrink_0()
                                            .text_color(content(0.4))
                                            .child(when(entry.at)),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(21.))
                                    .text_color(content(0.7))
                                    .line_clamp(2)
                                    .child(entry.message.clone()),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| this.open(&id, window, cx)))
            }))
            .into_any_element()
    }
}

/// FilterChip's two looks.
fn filter_look(d: gpui::Stateful<gpui::Div>, active: bool) -> gpui::Stateful<gpui::Div> {
    if active {
        d.border_color(content(0.2))
            .bg(content(0.08))
            .text_color(content(1.))
    } else {
        d.border_color(content(0.08))
            .text_color(content(0.55))
            .hover(|s| s.bg(content(0.05)).text_color(content(1.)))
    }
}
