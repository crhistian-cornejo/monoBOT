//! The Swarm tab: a lead's orchestration board beside its chat, with Board
//! (every post, live), Members (who coordinates, who works, who stands by)
//! and Graph (who talked to whom, replayable) sub-tabs. It only draws
//! `monocode_core::orchestration::board`; the shell hands it the board of the
//! chat in view whenever that changes, so nothing here reads disk or runs.
mod graph;

use crate::{
    input::{self, Input},
    monos::avatar,
    right_panel::{DIFF_ADD, stroke, tabular},
    theme::*,
};
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    HighlightStyle, Hsla, ListAlignment, ListState, Role, SharedString, StyledText, Subscription,
    Task, WeakEntity, Window, div, list, prelude::*, px, rgb,
};
use monocode_core::orchestration::board::{self, Board, Kind, Member, Post, Section, State};
use std::{collections::HashMap, sync::Arc};

pub(crate) use graph::Timeline;

/// Asks the shell to open a member's chat.
pub(crate) struct OpenMember(pub i64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SwarmTab {
    #[default]
    Board,
    Members,
    Graph,
}
impl SwarmTab {
    fn label(self) -> &'static str {
        match self {
            Self::Board => "Board",
            Self::Members => "Members",
            Self::Graph => "Graph",
        }
    }
}

pub(crate) struct SwarmPanel {
    lead: Option<i64>,
    board: Arc<Board>,
    tab: SwarmTab,
    query: Entity<Input>,
    member_query: Entity<Input>,
    /// Hide what the run posts by itself; show agents' own posts only.
    agents_only: bool,
    /// Indices into the board's posts that the Board tab lists.
    rows: Arc<Vec<usize>>,
    list: ListState,
    /// Per lead, the newest post the user has had on screen.
    seen: HashMap<i64, u64>,
    /// Posts from this sequence number on sit under «New».
    divider: Option<u64>,
    shown: bool,
    pub(crate) timeline: Timeline,
    /// Redraws Members once a second while someone's time is running.
    clock: Option<Task<()>>,
    /// What the header says when nobody works: a run is Finished, a
    /// group of agents only Idle.
    rest_label: &'static str,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<OpenMember> for SwarmPanel {}
impl Focusable for SwarmPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn field(label: &'static str, cx: &mut Context<Input>) -> Input {
    let mut input = Input::field(label, label, content(0.35), cx);
    input.set_font_size(12.);
    input
}

/// A member's colour, as its pixel mascot wears it.
pub(crate) fn member_color(ix: usize) -> u32 {
    monocode_core::mono::COLORS_MONO[ix % monocode_core::mono::COLORS_MONO.len()]
}

/// The green of a running member's dot and the Live mark.
pub(crate) fn live_color() -> Hsla {
    rgb(DIFF_ADD).into()
}

fn member_avatar(ix: usize, member: &Member, side: f32) -> AnyElement {
    let (mascot, _) = Board::look(ix);
    let status = if member.state == State::Working {
        "working"
    } else {
        "idle"
    };
    div()
        .relative()
        .flex_shrink_0()
        .size(px(side + 8.))
        .rounded(px(8.))
        .bg(content(0.06))
        .border_1()
        .border_color(content(0.08))
        .flex()
        .items_center()
        .justify_center()
        .child(avatar::avatar(
            mascot,
            member_color(ix),
            status,
            side,
            !member.state.active(),
        ))
        .when(member.state.active(), |d| {
            d.child(
                div()
                    .absolute()
                    .right(px(-3.))
                    .bottom(px(-3.))
                    .size(px(10.))
                    .rounded_full()
                    .border_2()
                    .border_color(pane_fill())
                    .bg(live_color()),
            )
        })
        .into_any_element()
}

/// Text with its `code` runs tinted, wrapping like prose.
pub(crate) fn rich_text(text: &str) -> StyledText {
    let mut plain = String::with_capacity(text.len());
    let mut highlights = Vec::new();
    for (code, part) in board::segments(text) {
        let start = plain.len();
        plain.push_str(part);
        if code {
            highlights.push((
                start..plain.len(),
                HighlightStyle {
                    background_color: Some(content(0.08)),
                    color: Some(content(0.95)),
                    ..Default::default()
                },
            ));
        }
    }
    StyledText::new(plain).with_highlights(highlights)
}

/// A post's header: `@from → @to …`, with its number and time at the end.
fn post_header(post: &Post, emphasis: bool) -> AnyElement {
    let recipients = if post.all {
        std::iter::once("@all".to_owned())
            .chain(post.to.iter().map(|t| format!("@{t}")))
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        post.to
            .iter()
            .map(|t| format!("@{t}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    div()
        .min_w_0()
        .flex()
        .items_center()
        .gap_1p5()
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(12.5))
                .font_weight(if emphasis {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(content(0.95))
                .child(format!("@{}", post.from)),
        )
        .when(!recipients.is_empty(), |d| {
            d.child(
                div()
                    .flex_shrink_0()
                    .text_size(px(11.))
                    .text_color(content(0.35))
                    .child("→"),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.))
                    .text_color(content(0.5))
                    .child(recipients),
            )
        })
        .child(div().flex_1())
        .child(
            div()
                .flex_shrink_0()
                .font_family(crate::fonts::mono())
                .font_features(tabular())
                .text_size(px(10.5))
                .text_color(content(0.4))
                .child(format!("#{} · {}", post.seq, board::clock(post.at))),
        )
        .into_any_element()
}

impl SwarmPanel {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| field("Search", cx));
        let member_query = cx.new(|cx| field("Search members", cx));
        let subscriptions = vec![
            cx.subscribe(&query, |this, _, _: &input::Changed, cx| {
                this.refilter(cx);
            }),
            cx.subscribe(&member_query, |_, _, _: &input::Changed, cx| cx.notify()),
        ];
        Self {
            lead: None,
            board: Arc::new(Board::default()),
            tab: SwarmTab::Board,
            query,
            member_query,
            agents_only: false,
            rows: Arc::new(Vec::new()),
            list: ListState::new(0, ListAlignment::Top, px(400.)),
            seen: HashMap::new(),
            divider: None,
            shown: false,
            timeline: Timeline::default(),
            clock: None,
            rest_label: "Finished",
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    /// For a team that rests between requests rather than finishing.
    pub(crate) fn set_rest_label(&mut self, label: &'static str) {
        self.rest_label = label;
    }

    #[cfg(test)]
    pub(crate) fn lead(&self) -> Option<i64> {
        self.lead
    }
    #[cfg(test)]
    pub(crate) fn board(&self) -> &Board {
        &self.board
    }
    #[cfg(test)]
    pub(crate) fn tab(&self) -> SwarmTab {
        self.tab
    }

    /// Show `lead`'s board. A new board for the same lead appends its posts
    /// and keeps the reader's place.
    pub(crate) fn set_board(&mut self, lead: i64, board: Arc<Board>, cx: &mut Context<Self>) {
        let same = self.lead == Some(lead);
        if same && Arc::ptr_eq(&self.board, &board) {
            return;
        }
        if !same {
            if let Some(old) = self.lead {
                let latest = self.latest();
                if self.shown && self.tab == SwarmTab::Board {
                    self.seen.insert(old, latest);
                }
            }
            self.lead = Some(lead);
            self.timeline = Timeline::default();
            self.divider = None;
        }
        let latest_before = if same { self.latest() } else { 0 };
        self.board = board;
        self.refilter(cx);
        if !same {
            self.mark_seen();
        } else if self.shown && self.tab == SwarmTab::Board {
            self.seen.insert(lead, self.latest());
        }
        let latest = self.latest();
        self.timeline
            .follow(self.board.posts.len(), latest > latest_before && same, cx);
        cx.notify();
    }

    /// Forget the board when the chat in view has none.
    pub(crate) fn clear(&mut self, cx: &mut Context<Self>) {
        if self.lead.is_none() {
            return;
        }
        if let Some(old) = self.lead.take()
            && self.shown
            && self.tab == SwarmTab::Board
        {
            self.seen.insert(old, self.latest());
        }
        self.board = Arc::new(Board::default());
        self.timeline = Timeline::default();
        self.refilter(cx);
        cx.notify();
    }

    fn latest(&self) -> u64 {
        self.board.posts.last().map_or(0, |p| p.seq)
    }

    /// The reader now sees the board: posts since the last visit go under
    /// «New».
    fn mark_seen(&mut self) {
        let Some(lead) = self.lead else {
            return;
        };
        if !(self.shown && self.tab == SwarmTab::Board) {
            return;
        }
        let seen = self.seen.get(&lead).copied().unwrap_or(0);
        self.divider = self
            .board
            .posts
            .iter()
            .find(|p| p.seq > seen)
            .filter(|_| seen > 0)
            .map(|p| p.seq);
        self.seen.insert(lead, self.latest());
    }

    pub(crate) fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        if self.shown == shown {
            return;
        }
        self.shown = shown;
        if shown {
            self.mark_seen();
        } else {
            self.timeline.pause();
            self.clock = None;
        }
        cx.notify();
    }

    pub(crate) fn select(&mut self, tab: SwarmTab, cx: &mut Context<Self>) {
        if self.tab == tab {
            return;
        }
        if self.tab == SwarmTab::Graph {
            self.timeline.pause();
        }
        self.tab = tab;
        self.mark_seen();
        cx.notify();
    }

    /// The posts the Board tab lists, after the search and filter.
    fn refilter(&mut self, cx: &mut Context<Self>) {
        let query = self.query.read(cx).content.trim().to_owned();
        let rows: Vec<usize> = self
            .board
            .posts
            .iter()
            .enumerate()
            .filter(|(_, p)| !self.agents_only || p.kind == Kind::Message)
            .filter(|(_, p)| Board::matches(p, &query))
            .map(|(ix, _)| ix)
            .collect();
        let old = self.rows.clone();
        // New posts at the end splice in; anything else starts over.
        let appended = !old.is_empty()
            && rows.len() >= old.len()
            && old.iter().zip(&rows).all(|(a, b)| a == b);
        if appended {
            if rows.len() > old.len() {
                self.list
                    .splice(old.len()..old.len(), rows.len() - old.len());
            }
        } else if rows != *old {
            self.list.reset(rows.len());
            self.list.set_follow_mode(gpui::FollowMode::Tail);
            if !rows.is_empty() {
                self.list.scroll_to_end();
            }
        }
        self.rows = Arc::new(rows);
        cx.notify();
    }

    fn open_member(&mut self, handle: &str, cx: &mut Context<Self>) {
        if let Some(session) = self.board.member(handle).and_then(|m| m.session) {
            cx.emit(OpenMember(session));
        }
    }

    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        let live = self.board.live;
        div()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_b_1()
            .border_color(stroke())
            .child(
                div()
                    .px_3()
                    .pt_2p5()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(content(0.95))
                            .child(if self.board.title.is_empty() {
                                "Swarm".to_owned()
                            } else {
                                self.board.title.clone()
                            }),
                    )
                    .child(
                        div()
                            .id("swarm-live")
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_size(px(11.5))
                            .text_color(content(0.6))
                            .child(div().size(px(6.)).rounded_full().bg(if live {
                                live_color()
                            } else {
                                content(0.3)
                            }))
                            .child(if live { "Live" } else { self.rest_label }),
                    ),
            )
            .child(
                div()
                    .id("swarm-tabs")
                    .debug_selector(|| "swarm-tabs".into())
                    .role(Role::TabList)
                    .aria_label("Swarm")
                    .px_3()
                    .pt_1p5()
                    .flex()
                    .items_end()
                    .gap_4()
                    .children(
                        [SwarmTab::Board, SwarmTab::Members, SwarmTab::Graph].map(|tab| {
                            let active = self.tab == tab;
                            let count = match tab {
                                SwarmTab::Board => Some(self.board.posts.len()),
                                SwarmTab::Members => Some(self.board.members.len()),
                                SwarmTab::Graph => None,
                            };
                            div()
                                .id(SharedString::from(format!("swarm-tab-{}", tab.label())))
                                .debug_selector(move || format!("swarm-tab-{}", tab.label()))
                                .role(Role::Tab)
                                .aria_label(tab.label())
                                .aria_selected(active)
                                .cursor_pointer()
                                .pb_1p5()
                                .flex()
                                .items_center()
                                .gap_1()
                                .border_b_2()
                                .border_color(if active {
                                    content(0.9)
                                } else {
                                    gpui::transparent_black()
                                })
                                .text_size(px(12.5))
                                .text_color(content(if active { 0.95 } else { 0.5 }))
                                .when(!active, |d| d.hover(|s| s.text_color(content(0.85))))
                                .child(tab.label())
                                .children(count.map(|n| {
                                    div()
                                        .text_size(px(10.5))
                                        .font_features(tabular())
                                        .text_color(content(0.4))
                                        .child(n.to_string())
                                }))
                                .on_click(cx.listener(move |this, _, _, cx| this.select(tab, cx)))
                        }),
                    ),
            )
            .into_any_element()
    }

    fn search_row(
        &self,
        query: &Entity<Input>,
        filter: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex_shrink_0()
            .px_3()
            .py_2()
            .flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(content(0.1))
                    .bg(content(0.05))
                    .px_2()
                    .child(icon("search", 12.).text_color(content(0.4)))
                    .child(div().min_w_0().flex_1().py(px(5.)).child(query.clone())),
            )
            .when(filter, |d| {
                let on = self.agents_only;
                d.child(
                    icon_button(
                        "swarm-filter",
                        if on {
                            "Show every post"
                        } else {
                            "Show agents' own posts only"
                        },
                        "filter",
                    )
                    .when(on, |d| d.bg(content(0.1)).text_color(content(1.)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.agents_only = !this.agents_only;
                        this.refilter(cx);
                    })),
                )
            })
            .into_any_element()
    }

    fn board_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let entity = cx.entity().downgrade();
        let board = self.board.clone();
        let rows = self.rows.clone();
        let divider = self.divider;
        let body = if self.board.posts.is_empty() {
            empty(
                "No posts yet. The coordinator's assignments and the agents' messages appear here as they happen.",
            )
        } else if rows.is_empty() {
            empty("No posts match.")
        } else {
            list(self.list.clone(), move |ix, _, _| {
                let Some(post) = rows.get(ix).and_then(|i| board.posts.get(*i)) else {
                    return div().into_any_element();
                };
                let new = divider.is_some_and(|d| post.seq == d);
                post_row(&board, post, new, entity.clone())
            })
            .size_full()
            .into_any_element()
        };
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(self.search_row(&self.query, true, cx))
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }

    /// Keep Members' times moving while it is on screen and someone works.
    fn run_clock(&mut self, cx: &mut Context<Self>) {
        let wanted = self.shown
            && self.tab == SwarmTab::Members
            && self.board.members.iter().any(|m| m.active_since.is_some());
        if !wanted {
            self.clock = None;
            return;
        }
        if self.clock.is_some() {
            return;
        }
        self.clock = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }
        }));
    }

    fn members_tab(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.run_clock(cx);
        let now = monocode_core::time::now_ms();
        let query = self
            .member_query
            .read(cx)
            .content
            .trim()
            .trim_start_matches('@')
            .to_lowercase();
        let mut members: Vec<(usize, &Member)> = self
            .board
            .members
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                query.is_empty()
                    || m.handle.to_lowercase().contains(&query)
                    || m.assignment.to_lowercase().contains(&query)
                    || m.model
                        .as_deref()
                        .is_some_and(|x| x.to_lowercase().contains(&query))
            })
            .collect();
        members.sort_by_key(|(ix, m)| (m.section(), *ix));
        let mut children: Vec<AnyElement> = Vec::new();
        let mut section = None;
        for (ix, member) in members {
            let this_section = member.section();
            if section != Some(this_section) {
                if this_section != Section::Coordinator {
                    let count = self
                        .board
                        .members
                        .iter()
                        .filter(|m| m.section() == this_section)
                        .count();
                    children.push(
                        div()
                            .px_3()
                            .pt_3()
                            .pb_1()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .border_t_1()
                            .border_color(stroke())
                            .text_size(px(10.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(content(0.45))
                            .child(this_section.label().to_uppercase())
                            .child(
                                div()
                                    .font_features(tabular())
                                    .text_color(content(0.35))
                                    .child(count.to_string()),
                            )
                            .into_any_element(),
                    );
                }
                section = Some(this_section);
            }
            children.push(self.member_row(ix, member, now, cx));
        }
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(self.search_row(&self.member_query, false, cx))
            .child(
                div()
                    .id("swarm-members")
                    .debug_selector(|| "swarm-members".into())
                    .role(Role::List)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .pb_2()
                    .children(children),
            )
            .into_any_element()
    }

    fn member_row(
        &self,
        ix: usize,
        member: &Member,
        now: u64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let handle = member.handle.clone();
        let state = if member.coordinator() {
            Section::Coordinator.label().to_owned()
        } else {
            member.state.label().to_owned()
        };
        let model = member
            .model
            .clone()
            .unwrap_or_else(|| member.provider.clone());
        let worked = member.worked(now);
        let meta = [
            (!model.is_empty()).then_some(model),
            (member.latest_seq > 0).then(|| format!("#{}", member.latest_seq)),
            (worked >= 1_000).then(|| board::duration(worked)),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
        let opens = member.session.is_some();
        div()
            .id(SharedString::from(format!("swarm-member-{handle}")))
            .debug_selector({
                let handle = handle.clone();
                move || format!("swarm-member-{handle}")
            })
            .role(Role::ListItem)
            .aria_label(format!("@{handle}, {state}"))
            .mx_1p5()
            .px_1p5()
            .py_2()
            .rounded(px(8.))
            .flex()
            .gap_2p5()
            .when(opens, |d| {
                d.cursor_pointer()
                    .hover(|s| s.bg(content(0.04)))
                    .on_click(cx.listener(move |this, _, _, cx| this.open_member(&handle, cx)))
            })
            .child(member_avatar(ix, member, 22.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(content(0.95))
                                    .child(format!("@{}", member.handle)),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(px(11.5))
                                    .text_color(match member.state {
                                        _ if member.coordinator() => content(0.5),
                                        State::Working => accent_color(),
                                        State::NeedsInput => {
                                            rgb(crate::right_panel::AMBER_400).into()
                                        }
                                        State::Failed => rgb(crate::right_panel::RED_400).into(),
                                        _ => content(0.45),
                                    })
                                    .child(state),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_family(crate::fonts::mono())
                                    .text_size(px(10.5))
                                    .text_color(content(0.4))
                                    .child(meta),
                            ),
                    )
                    .when(!member.assignment.is_empty(), |d| {
                        d.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(px(11.5))
                                .text_color(content(0.45))
                                .child(member.assignment.clone()),
                        )
                    })
                    .when(!member.latest.is_empty(), |d| {
                        d.child(
                            div()
                                .min_w_0()
                                .text_size(px(12.))
                                .line_height(px(17.))
                                .text_color(content(0.8))
                                .line_clamp(2)
                                .child(rich_text(&member.latest)),
                        )
                    }),
            )
            .into_any_element()
    }
}

fn empty(text: &'static str) -> AnyElement {
    div()
        .size_full()
        .p_4()
        .flex()
        .justify_center()
        .text_size(px(12.))
        .text_color(content(0.45))
        .child(text)
        .into_any_element()
}

/// One post of the Board tab, after the «New» divider when it is the first
/// unseen one.
fn post_row(board: &Board, post: &Post, new: bool, panel: WeakEntity<SwarmPanel>) -> AnyElement {
    let ix = board
        .members
        .iter()
        .position(|m| m.handle == post.from)
        .unwrap_or(0);
    let member = board.members.get(ix).cloned().unwrap_or_default();
    let from = post.from.clone();
    let automatic = post.kind != Kind::Message;
    div()
        .flex()
        .flex_col()
        .when(new, |d| {
            d.child(
                div()
                    .px_3()
                    .pt_1()
                    .pb_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(11.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(accent_color())
                    .child("New")
                    .child(div().flex_1().h(px(1.)).bg(accent_color().opacity(0.5))),
            )
        })
        .child(
            div()
                .id(SharedString::from(format!("swarm-post-{}", post.seq)))
                .debug_selector({
                    let seq = post.seq;
                    move || format!("swarm-post-{seq}")
                })
                .role(Role::ListItem)
                .mx_1p5()
                .px_1p5()
                .py_2()
                .rounded(px(8.))
                .flex()
                .gap_2p5()
                .when(new, |d| d.bg(accent_color().opacity(0.06)))
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "swarm-post-avatar-{}",
                            post.seq
                        )))
                        .when(member.session.is_some(), |d| {
                            d.cursor_pointer().on_click(move |_, _, cx| {
                                let _ = panel.update(cx, |this, cx| this.open_member(&from, cx));
                            })
                        })
                        .child(member_avatar(ix, &member, 18.)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(post_header(post, !automatic))
                        .child(
                            div()
                                .min_w_0()
                                .text_size(px(12.5))
                                .line_height(px(18.))
                                .text_color(content(if automatic { 0.7 } else { 0.88 }))
                                .child(rich_text(&post.text)),
                        ),
                ),
        )
        .into_any_element()
}

impl Render for SwarmPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.tab != SwarmTab::Members {
            self.clock = None;
        }
        let body = match self.tab {
            SwarmTab::Board => self.board_tab(cx),
            SwarmTab::Members => self.members_tab(cx),
            SwarmTab::Graph => self.graph_tab(window, cx),
        };
        div()
            .id("swarm-panel")
            .debug_selector(|| "swarm-panel".into())
            .track_focus(&self.focus)
            .role(Role::Group)
            .aria_label("Swarm")
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .child(self.header(cx))
            .child(body)
    }
}

#[cfg(test)]
mod tests;
