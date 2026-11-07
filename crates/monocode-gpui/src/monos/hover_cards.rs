//! Hover cards in the agents' rail and the News (MonoCardPreview.tsx's
//! `useMonoCardPreview` and BotNewsList.tsx's `useNewsSummary`): a row's card
//! opens beside it, the pointer may cross the gap into the card, and leaving
//! both closes it a moment later. The cards reuse the popover's entrance, so
//! they fade and slide in from the row as quickly as the desktop's.
use super::*;
use crate::popover::Side;
use gpui::{FontWeight, Task};
use monocode_core::{
    connectors,
    mono::{agents as core_agents, bot_tools, bots, groups::roster, news},
};

/// How long the pointer may be away from both row and card before it closes.
const CLOSE_MS: u64 = 120;
/// How long the pointer rests on a report before its summary shows.
const SUMMARY_OPEN_MS: u64 = 250;
const SUMMARY_MAX: usize = 220;
const LIST_MAX: usize = 4;

/// One hover card's state: the row it shows for, where that row is, and the
/// card's own bounds so the row's leave does not close a card being entered.
pub(super) struct Hover {
    pub(super) shown: Option<String>,
    anchor: AnchorSlot,
    pending: Option<String>,
    surface: AnchorSlot,
    timer: Option<Task<()>>,
}
impl Default for Hover {
    fn default() -> Self {
        Self {
            shown: None,
            anchor: AnchorSlot::default(),
            pending: None,
            surface: AnchorSlot::default(),
            timer: None,
        }
    }
}

type Pick = fn(&mut Monos) -> &mut Hover;

impl Hover {
    pub(super) fn close(&mut self) -> bool {
        self.timer = None;
        self.pending = None;
        self.shown.take().is_some()
    }

    /// The pointer came onto a row. With a card already open the card moves
    /// to it at once; otherwise it opens after `open_after`.
    fn enter(
        this: &mut Monos,
        pick: Pick,
        key: String,
        anchor: AnchorSlot,
        open_after: u64,
        cx: &mut Context<Monos>,
    ) {
        let hover = pick(this);
        hover.timer = None;
        hover.pending = None;
        if hover.shown.is_some() || open_after == 0 {
            if hover.shown.as_deref() != Some(key.as_str()) {
                hover.shown = Some(key);
                hover.anchor = anchor;
                cx.notify();
            }
            return;
        }
        hover.pending = Some(key.clone());
        hover.timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(open_after))
                .await;
            let _ = this.update(cx, |this, cx| {
                let hover = pick(this);
                hover.timer = None;
                hover.pending = None;
                hover.shown = Some(key);
                hover.anchor = anchor;
                cx.notify();
            });
        }));
    }

    /// The pointer left a row. A leave can arrive after the next row's
    /// enter, so only the row shown (or about to be) reacts to it.
    fn leave(this: &mut Monos, pick: Pick, key: &str, window: &Window, cx: &mut Context<Monos>) {
        let hover = pick(this);
        if hover.pending.as_deref() == Some(key) {
            hover.pending = None;
            hover.timer = None;
            return;
        }
        if hover.shown.as_deref() == Some(key) {
            Self::schedule_close(this, pick, window, cx);
        }
    }

    /// The pointer moved onto or off the card itself.
    fn card_hover(
        this: &mut Monos,
        pick: Pick,
        hovered: bool,
        window: &Window,
        cx: &mut Context<Monos>,
    ) {
        if hovered {
            pick(this).timer = None;
        } else {
            Self::schedule_close(this, pick, window, cx);
        }
    }

    fn schedule_close(this: &mut Monos, pick: Pick, window: &Window, cx: &mut Context<Monos>) {
        let hover = pick(this);
        // During the entrance the card can already cover the gap beside its
        // row: the row's leave must not undo the card's enter.
        if hover
            .surface
            .get()
            .is_some_and(|bounds| bounds.contains(&window.mouse_position()))
        {
            hover.timer = None;
            return;
        }
        hover.timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(CLOSE_MS))
                .await;
            let _ = this.update(cx, |this, cx| {
                if pick(this).close() {
                    cx.notify();
                }
            });
        }));
    }
}

fn mono_preview(this: &mut Monos) -> &mut Hover {
    &mut this.mono_preview
}
fn news_summary(this: &mut Monos) -> &mut Hover {
    &mut this.news.summary
}

/// A few names and how many more, for one line of a card.
fn short_list(names: &[String]) -> String {
    let shown = names
        .iter()
        .take(LIST_MAX)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > LIST_MAX {
        format!("{shown} +{}", names.len() - LIST_MAX)
    } else {
        shown
    }
}

/// `~/…` for a folder under home (prettyCwd).
fn pretty_path(path: &str) -> String {
    std::env::var("HOME")
        .ok()
        .filter(|home| !home.is_empty())
        .and_then(|home| {
            path.strip_prefix(home.as_str()).and_then(|rest| {
                if rest.is_empty() {
                    Some("~".to_owned())
                } else {
                    rest.starts_with('/').then(|| format!("~{rest}"))
                }
            })
        })
        .unwrap_or_else(|| path.to_owned())
}

/// What a bot can use, grouped as its settings group it (botToolLines).
fn bot_tool_lines(mono: &Mono) -> Vec<(&'static str, String)> {
    let tools = bot_tools::tools(
        mono.bot_role.as_deref().unwrap_or("assistant"),
        mono.bot_tools.as_ref(),
    );
    let accounts: Vec<String> = bot_tools::SERVICES
        .iter()
        .filter(|service| tools.services.iter().any(|s| s == *service))
        .map(|service| core_agents::service_label(service).0.to_owned())
        .collect();
    let access: Vec<String> = bot_tools::ACCESS
        .iter()
        .filter(|access| tools.access.iter().any(|a| a == *access))
        .map(|access| core_agents::access_label(access).0.to_owned())
        .collect();
    let connectors: Vec<String> = connectors::CONNECTORS
        .iter()
        .filter(|c| {
            tools
                .connectors
                .as_ref()
                .is_some_and(|list| list.iter().any(|id| id == c.id))
        })
        .map(|c| c.name.to_owned())
        .collect();
    let folders: Vec<String> = tools.folders.iter().map(|f| pretty_path(&f.path)).collect();
    [
        ("Accounts", accounts),
        ("Can", access),
        ("Connectors", connectors),
        ("Skills", tools.skills.unwrap_or_default()),
        ("Folders", folders),
    ]
    .into_iter()
    .filter(|(_, names)| !names.is_empty())
    .map(|(label, names)| (label, short_list(&names)))
    .collect()
}

/// PreviewDetail: a label column and its value, as a session's card has.
fn detail(label: &'static str, value: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .gap_2()
        .text_size(px(11.))
        .line_height(px(16.))
        .child(
            div()
                .w(px(72.))
                .flex_shrink_0()
                .text_color(content(0.45))
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_color(content(0.8))
                .child(value),
        )
}

impl Monos {
    /// Hover listener for an agent row in the rail.
    pub(super) fn mono_row_hover(
        &mut self,
        id: &str,
        anchor: AnchorSlot,
        hovered: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        // Dragging a row to reorder it is not reading about it.
        if self.skip_click && self.drag.is_some() {
            return;
        }
        if hovered {
            Hover::enter(self, mono_preview, id.to_owned(), anchor, 0, cx);
        } else {
            Hover::leave(self, mono_preview, id, window, cx);
        }
    }

    /// Closes the agent card, e.g. once a row's menu or a drag takes over.
    pub(super) fn close_mono_preview(&mut self, cx: &mut Context<Self>) {
        if self.mono_preview.close() {
            cx.notify();
        }
    }

    /// The agent card beside its row, when that row is in `roster`.
    pub(super) fn mono_preview_card(
        &self,
        roster: &[Mono],
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let id = self.mono_preview.shown.as_deref()?;
        let mono = roster.iter().find(|m| m.id == id)?;
        let state = self
            .chats
            .get(id)
            .map(|chat| chat.read(cx).mono_status().to_owned())
            .unwrap_or_else(|| "idle".into());
        Some(
            popover(
                "mono-preview",
                Anchor::Slot(self.mono_preview.anchor.clone()),
            )
            .side(Side::Right)
            .gap(8.)
            .solid(rail_color())
            .width(260.)
            .max_height(440.)
            .track(self.mono_preview.surface.clone())
            .child(
                div()
                    .id("mono-preview-content")
                    .debug_selector(|| "mono-preview".into())
                    .role(Role::Tooltip)
                    .max_h(px(438.))
                    .overflow_y_scroll()
                    .on_hover(cx.listener(|this, hovered, window, cx| {
                        Hover::card_hover(this, mono_preview, *hovered, window, cx)
                    }))
                    .child(self.mono_preview_body(mono, &state)),
            )
            .into_any_element(),
        )
    }

    /// MonoCardPreview's body: what it is, its state, what it is for, and
    /// who is in it or what it can use.
    fn mono_preview_body(&self, mono: &Mono, state: &str) -> AnyElement {
        let group = mono.group.as_ref();
        let kind = if let Some(group) = group {
            let n = group.members.len();
            format!("Group · {n} {}", if n == 1 { "member" } else { "members" })
        } else if mono.is_bot() {
            if mono.is_principal() {
                "Orchestrator".into()
            } else {
                let role = mono.bot_role.as_deref().unwrap_or("assistant");
                let name = bots::role(role)
                    .or_else(|| bots::role("assistant"))
                    .map_or("Assistant", |r| r.name);
                format!("{name} bot")
            }
        } else if mono.kind.as_deref() == Some("workstream") {
            "Workstream".into()
        } else {
            "Mono".into()
        };
        // monoSpecialty: its own line, else its role's for a bot.
        let about = match group {
            Some(group) => group.goal.clone(),
            None => mono
                .specialty
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .or_else(|| {
                    mono.is_bot().then(|| {
                        bots::role(mono.bot_role.as_deref().unwrap_or("assistant"))
                            .map(|r| r.specialty.to_owned())
                            .unwrap_or_default()
                    })
                }),
        }
        .filter(|s| !s.trim().is_empty());
        let (status, status_color) = match state {
            "needs-you" => ("Needs you", accent_color()),
            "working" => ("Working", content(0.7)),
            _ => ("Idle", content(0.4)),
        };
        let mut details = div()
            .mt_2()
            .pt_2()
            .border_t_1()
            .border_color(content(0.08))
            .flex()
            .flex_col()
            .gap(px(6.));
        if let Some(group) = group {
            // groupLead: the Principal speaks for the group.
            let member = |id: &String| self.roster.get(id);
            let lead = group
                .members
                .iter()
                .filter_map(member)
                .find(|m| m.is_principal())
                .or_else(|| self.roster.monos.iter().find(|m| m.is_principal()));
            let members: Vec<String> = group
                .members
                .iter()
                .filter_map(member)
                .filter(|m| lead.is_none_or(|lead| lead.id != m.id))
                .map(Mono::label)
                .collect();
            if let Some(lead) = lead {
                details = details.child(detail("Leads", lead.label()));
            }
            details = details.child(detail(
                "Members",
                if members.is_empty() {
                    "Only the lead yet".to_owned()
                } else {
                    short_list(&members)
                },
            ));
            if !group.folders.is_empty() {
                let folders: Vec<String> =
                    group.folders.iter().map(|f| pretty_path(&f.path)).collect();
                details = details.child(detail("Folders", short_list(&folders)));
            }
            if !group.context.is_empty() {
                let n = group.context.len();
                details = details.child(detail(
                    "Context",
                    if n == 1 {
                        "1 file".to_owned()
                    } else {
                        format!("{n} files")
                    },
                ));
            }
            let waiting = group.requests.len();
            if waiting > 0 {
                details = details.child(detail(
                    "Waiting",
                    div().text_color(accent_color()).child(if waiting == 1 {
                        "1 access request".to_owned()
                    } else {
                        format!("{waiting} access requests")
                    }),
                ));
            }
        } else if mono.is_bot() {
            details = details.children(
                bot_tool_lines(mono)
                    .into_iter()
                    .map(|(label, value)| detail(label, value)),
            );
        } else {
            let names: Vec<String> = mono
                .projects
                .iter()
                .map(|p| roster::project_name(p))
                .collect();
            details = details.child(detail(
                "Projects",
                if names.is_empty() {
                    "No projects yet".to_owned()
                } else {
                    monocode_core::mono::groups::brief::projects_phrase(&names)
                },
            ));
        }
        div()
            .px_3()
            .py_2p5()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .text_size(px(11.))
                            .text_color(content(0.55))
                            .child(if mono.is_group() {
                                let (shown, more) = self.group_stack_members(mono);
                                avatar::group_avatar_stack(&shown, state, more, 16., false)
                            } else {
                                div()
                                    .size(px(14.))
                                    .flex_shrink_0()
                                    .child(avatar::avatar(
                                        &mono.mascot,
                                        mono.color,
                                        state,
                                        14.,
                                        true,
                                    ))
                                    .into_any_element()
                            })
                            .child(div().min_w_0().truncate().child(kind)),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(11.))
                            .text_color(status_color)
                            .child(status),
                    ),
            )
            .child(
                div()
                    .mt_1p5()
                    .text_size(px(13.))
                    .line_height(px(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(content(1.))
                    .child(mono.label()),
            )
            .children(about.map(|about| {
                div()
                    .mt_0p5()
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(content(0.65))
                    .line_clamp(3)
                    .child(about)
            }))
            .child(details)
            .into_any_element()
    }

    /// Hover listener for a report's header in the News.
    pub(super) fn news_header_hover(
        &mut self,
        id: &str,
        anchor: AnchorSlot,
        hovered: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if hovered {
            Hover::enter(
                self,
                news_summary,
                id.to_owned(),
                anchor,
                SUMMARY_OPEN_MS,
                cx,
            );
        } else {
            Hover::leave(self, news_summary, id, window, cx);
        }
    }

    /// useNewsSummary's card: who posted it, from what and when, and the
    /// report as one plain paragraph.
    pub(super) fn news_summary_card(
        &self,
        items: &[news::Item],
        when: impl Fn(u64) -> String,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let id = self.news.summary.shown.as_deref()?;
        let item = items.iter().find(|item| item.id == id)?;
        let text = news::preview(&item.text, SUMMARY_MAX);
        Some(
            popover(
                "news-summary",
                Anchor::Slot(self.news.summary.anchor.clone()),
            )
            .side(Side::Left)
            .gap(8.)
            .solid(rail_color())
            .width(280.)
            .track(self.news.summary.surface.clone())
            .child(
                div()
                    .id("news-summary-content")
                    .debug_selector(|| "news-summary".into())
                    .role(Role::Tooltip)
                    .px_3()
                    .py_2p5()
                    .on_hover(cx.listener(|this, hovered, window, cx| {
                        Hover::card_hover(this, news_summary, *hovered, window, cx)
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .text_size(px(11.))
                            .text_color(content(0.55))
                            .child(self.face(&item.mono_id, 14., cx))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .child(format!("{} · {}", item.bot_name, item.source)),
                            )
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
                            .mt_1p5()
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .text_color(content(0.8))
                            .child(if text.is_empty() {
                                "No text".to_owned()
                            } else {
                                text
                            }),
                    ),
            )
            .into_any_element(),
        )
    }
}
