//! The open notch's shelf: three columns of widgets the user picks, orders
//! and widens, each reading what the app already keeps. The pencil turns it
//! into its arranging: move, narrow, widen or take out each widget, and add
//! what still fits. Each change is saved when it is made.
use super::*;
use monocode_core::notch::{Activity, Widget};

/// How far one press of the catalog's arrows moves it.
const CATALOG_STEP: f32 = 150.;

/// How long a meter's bar takes to reach a new reading.
const METER_GLIDE: f32 = 0.45;

/// A meter's bar in the accent: it glides from the reading before (`from`)
/// to this one (`level`, 0 to 100), which came at `at`. At once under
/// reduced motion.
fn meter_bar(from: Option<f32>, level: Option<f32>, at: Instant) -> gpui::Div {
    let to = level.unwrap_or(0.).clamp(0., 100.) / 100.;
    let from = from.map_or(to, |from| from.clamp(0., 100.) / 100.);
    div().h(px(3.)).bg(ink(0.12)).child(
        canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| {
                let t = if cx.reduce_motion() {
                    1.
                } else {
                    (at.elapsed().as_secs_f32() / METER_GLIDE).clamp(0., 1.)
                };
                let eased = 1. - (1. - t).powi(3);
                let shown = from + (to - from) * eased;
                window.paint_quad(gpui::fill(
                    Bounds::new(
                        bounds.origin,
                        gpui::size(bounds.size.width * shown, bounds.size.height),
                    ),
                    accent_color(),
                ));
                if t < 1. {
                    window.request_animation_frame();
                }
            },
        )
        .size_full(),
    )
}

/// The last minute of a number (each 0 to 100, oldest first) as a faint
/// area in the accent, the newest reading at the right edge.
fn graph(past: Vec<f32>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            if past.len() < 2 {
                return;
            }
            let step = bounds.size.width / (model::stats::HISTORY - 1) as f32;
            let first = model::stats::HISTORY - past.len();
            let line: Vec<_> = past
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    point(
                        bounds.left() + step * (first + index) as f32,
                        bounds.bottom() - bounds.size.height * (value.clamp(0., 100.) / 100.),
                    )
                })
                .collect();
            let accent = accent_color();
            let mut area = gpui::PathBuilder::fill();
            let mut outline = line.clone();
            outline.push(point(bounds.right(), bounds.bottom()));
            outline.push(point(line[0].x, bounds.bottom()));
            area.add_polygon(&outline, true);
            if let Ok(path) = area.build() {
                window.paint_path(path, accent.opacity(0.1));
            }
            let mut stroke = gpui::PathBuilder::stroke(px(1.));
            stroke.add_polygon(&line, false);
            if let Ok(path) = stroke.build() {
                window.paint_path(path, accent.opacity(0.45));
            }
        },
    )
    .size_full()
}

/// One of the arranging's small square buttons over a widget.
fn tool(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    path: &'static str,
    enabled: bool,
) -> gpui::Stateful<gpui::Div> {
    let id = id.into();
    let selector = format!("{id}");
    div()
        .id(id)
        .debug_selector(move || selector.clone())
        .role(Role::Button)
        .aria_label(label)
        .size(px(18.))
        .flex()
        .items_center()
        .justify_center()
        .bg(ink(0.12))
        .when(enabled, |tool| {
            tool.cursor_pointer().hover(|style| style.bg(ink(0.22)))
        })
        .when(!enabled, |tool| tool.opacity(0.35))
        .child(icon(path, 10.).text_color(ink(0.9)))
}

fn label(text: &'static str) -> gpui::Div {
    div()
        .text_size(px(9.5))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(ink(0.45))
        .child(text.to_uppercase())
}

fn quiet(text: &'static str) -> gpui::Div {
    div()
        .flex_1()
        .flex()
        .items_center()
        .text_size(px(11.5))
        .text_color(ink(0.4))
        .child(text)
}

/// liveAgents' elapsed label, short: seconds, then minutes, then hours.
fn elapsed(started_at: Option<u64>) -> String {
    let Some(started_at) = started_at else {
        return String::new();
    };
    let seconds = now_ms().saturating_sub(started_at) / 1000;
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m", seconds / 60),
        _ => format!("{}h {}m", seconds / 3600, seconds / 60 % 60),
    }
}

impl Notch {
    pub(super) fn shelf_view(&mut self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let shelf = self.look.shelf.clone();
        let rows = model::layout(&shelf);
        let editing = self.editing;
        let mut column = div()
            .debug_selector(|| "notch-shelf".into())
            .flex()
            .flex_col()
            .gap(px(model::CELL_GAP));
        for row in rows {
            let cells: Vec<AnyElement> = row
                .into_iter()
                .map(|index| {
                    let reveal = self.reveal(index, cx);
                    div()
                        .opacity(reveal)
                        .mt(px((1. - reveal) * 4.))
                        .child(self.cell(&shelf, index, width, cx))
                        .into_any_element()
                })
                .collect();
            column = column.child(div().flex().gap(px(model::CELL_GAP)).children(cells));
        }
        if shelf.is_empty() && !editing {
            column = column.child(
                div()
                    .h(px(model::CELL_HEIGHT))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(11.5))
                    .text_color(ink(0.45))
                    .child("No widgets. Use the pencil to add some."),
            );
        }
        if editing {
            column = column.child(self.catalog(cx));
        }
        column.into_any_element()
    }

    fn cell(
        &mut self,
        shelf: &[Widget],
        index: usize,
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let widget = shelf[index].clone();
        let id = widget.id.clone();
        let content = match id.as_str() {
            "agents" => self.agents_widget(widget.span, cx),
            "recent" => self.recent_widget(widget.span, cx),
            "today" => self.today_widget(),
            "asks" => self.asks_widget(widget.span, cx),
            "call" => self.call_widget(widget.span, cx),
            "clock" => clock_widget(),
            "system" => self.system_widget(widget.span),
            "new" => self.new_widget(cx),
            "calendar" => super::media::calendar_widget(widget.span),
            _ => div().into_any_element(),
        };
        let editing = self.editing;
        let name = model::widget_name(&id);
        div()
            .id(gpui::SharedString::from(format!("notch-widget-{id}")))
            .debug_selector({
                let id = id.clone();
                move || format!("notch-widget-{id}")
            })
            .relative()
            .w(px(model::cell_width(width, widget.span)))
            .h(px(model::CELL_HEIGHT))
            .flex_shrink_0()
            .p(px(10.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .bg(ink(0.07))
            .when(editing, |cell| {
                // Arranging: pick a widget up and drop it on another to put
                // it there; the one under it lights up.
                let dragged = WidgetDrag {
                    id: id.clone(),
                    name,
                };
                cell.border_1()
                    .border_color(ink(0.18))
                    .cursor(gpui::CursorStyle::OpenHand)
                    .on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
                    .drag_over::<WidgetDrag>(|style, _, _, _| {
                        style.border_color(accent_color()).bg(ink(0.12))
                    })
                    .on_drop(cx.listener(move |this, dragged: &WidgetDrag, _, cx| {
                        let id = dragged.id.clone();
                        this.change_look(
                            |look| {
                                look.move_to(&id, index);
                            },
                            cx,
                        );
                    }))
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .when(editing, |row| {
                        row.child(icon("grip-vertical", 10.).text_color(ink(0.45)))
                    })
                    .child(label(name)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .when(editing, |content| content.opacity(0.45))
                    .child(content),
            )
            .when(editing, |cell| cell.child(self.arranging(&widget, cx)))
            .into_any_element()
    }

    /// The arranging's tools over one widget, in its corner: narrower,
    /// wider and remove. Moving is a drag.
    fn arranging(&self, widget: &Widget, cx: &mut Context<Self>) -> AnyElement {
        let id = widget.id.clone();
        let widths = model::widths(&id).unwrap_or(&[1]);
        let narrower = widths.first().is_some_and(|first| *first < widget.span);
        let wider = widths.last().is_some_and(|last| *last > widget.span) && {
            let mut look = self.look.clone();
            look.resize(&id, 1)
        };
        let action = |by: i32, kind: &'static str| {
            let id = id.clone();
            cx.listener(move |this: &mut Notch, _: &gpui::ClickEvent, _, cx| {
                let id = id.clone();
                this.change_look(
                    |look| {
                        match kind {
                            "resize" => look.resize(&id, by),
                            _ => {
                                look.remove(&id);
                                true
                            }
                        };
                    },
                    cx,
                );
            })
        };
        let key = |name: &str| gpui::SharedString::from(format!("notch-{name}-{}", widget.id));
        div()
            .absolute()
            .bottom(px(6.))
            .right(px(6.))
            .flex()
            .gap(px(3.))
            .child(
                tool(key("narrower"), "Narrower", "minus-sign", narrower)
                    .when(narrower, |tool| tool.on_click(action(-1, "resize"))),
            )
            .child(
                tool(key("wider"), "Wider", "plus", wider)
                    .when(wider, |tool| tool.on_click(action(1, "resize"))),
            )
            .child(tool(key("remove"), "Remove", "x", true).on_click(action(0, "remove")))
            .into_any_element()
    }

    /// The widgets that still fit, to add.
    fn catalog(&self, cx: &mut Context<Self>) -> AnyElement {
        let addable = self.look.addable();
        let mut row = div()
            .debug_selector(|| "notch-catalog".into())
            .h(px(CATALOG_HEIGHT))
            .flex()
            .items_center()
            .gap(px(6.));
        if addable.is_empty() {
            row = row.child(div().text_size(px(11.)).text_color(ink(0.45)).child(
                if self.look.shelf.len() == model::WIDGETS.len() {
                    "Every widget is on the shelf."
                } else {
                    "The shelf is full. Narrow or remove a widget, or add a row in settings."
                },
            ));
        }
        if addable.is_empty() {
            return row.into_any_element();
        }
        // More widgets than the row holds: it scrolls (the wheel moves it
        // too) and an arrow at each end steps through it.
        let room = self.look.panel_width() - 2. * model::PANEL_PAD;
        let needed: f32 = addable
            .iter()
            .map(|id| (model::widget_name(id).chars().count() + 2) as f32 * 6. + 18. + 6.)
            .sum();
        let overflows = needed > room - 12.;
        let at = -f32::from(self.catalog_scroll.offset().x);
        let end = f32::from(self.catalog_scroll.max_offset().x);
        let step = |by: f32| {
            cx.listener(move |this: &mut Notch, _: &gpui::ClickEvent, _, cx| {
                let handle = &this.catalog_scroll;
                let end = f32::from(handle.max_offset().x).max(0.);
                let to = (-f32::from(handle.offset().x) + by).clamp(0., end);
                handle.set_offset(point(px(-to), px(0.)));
                cx.notify();
            })
        };
        let (back, forward) = (at > 0.5, end <= 0. || at < end - 0.5);
        let mut pills = div()
            .id("notch-catalog-scroll")
            .debug_selector(|| "notch-catalog-scroll".into())
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .gap(px(6.))
            .overflow_x_scroll()
            .track_scroll(&self.catalog_scroll);
        for id in addable {
            pills = pills.child(
                pill(
                    gpui::SharedString::from(format!("notch-add-{id}")),
                    &format!("+ {}", model::widget_name(id)),
                    false,
                )
                .flex_shrink_0()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.change_look(
                        |look| {
                            look.add(id);
                        },
                        cx,
                    )
                })),
            );
        }
        row.when(overflows, |row| {
            row.child(
                tool(
                    "notch-catalog-back",
                    "Earlier widgets",
                    "chevron-left",
                    back,
                )
                .size(px(22.))
                .flex_shrink_0()
                .when(back, |tool| tool.on_click(step(-CATALOG_STEP))),
            )
        })
        .child(pills)
        .when(overflows, |row| {
            row.child(
                tool(
                    "notch-catalog-forward",
                    "More widgets",
                    "chevron-right",
                    forward,
                )
                .size(px(22.))
                .flex_shrink_0()
                .when(forward, |tool| tool.on_click(step(CATALOG_STEP))),
            )
        })
        .into_any_element()
    }

    fn agents_widget(&self, span: u32, cx: &mut Context<Self>) -> AnyElement {
        let mut activities: Vec<&Activity> = self.feed().activities.iter().collect();
        // What works first, the longest running on top; then what finished.
        activities.sort_by_key(|a| (a.done, a.started_at.unwrap_or(u64::MAX)));
        if activities.is_empty() {
            // Nothing runs: offer to pick up where the user left off.
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(ink(0.5))
                        .child("Nothing running"),
                )
                .children(self.feed().recent.first().cloned().map(|recent| {
                    let id = recent.id;
                    super::pages::session_row(
                        ("notch-agents-last", id as u64),
                        provider_glyph(&recent.provider, 13.),
                        &recent.title,
                        "Continue".into(),
                        accent_color(),
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.emit(NotchEvent::OpenSession(id), cx)
                        }),
                    )
                }))
                .into_any_element();
        }
        // The card holds three rows: with more agents, "+N more" takes the third.
        let shown = if activities.len() > 3 { 2 } else { 3 };
        let more = activities.len().saturating_sub(shown);
        div()
            .flex()
            .flex_col()
            .gap(px(3.))
            .children(activities.into_iter().take(shown).map(|activity| {
                let id = activity.id;
                div()
                    .id(("notch-agent", id as u64))
                    .h(px(18.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .cursor_pointer()
                    .hover(|style| style.bg(ink(0.06)))
                    .on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.emit(NotchEvent::OpenSession(id), cx)
                        }),
                    )
                    .child(
                        div()
                            .size(px(14.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(activity_mark(activity, cx)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(11.5))
                            .text_color(ink(0.9))
                            .child(activity.title.clone()),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .max_w(px(if span >= 3 { 140. } else { 76. }))
                            .truncate()
                            .text_size(px(11.))
                            .text_color(if activity.needs_you {
                                accent_color()
                            } else {
                                ink(0.5)
                            })
                            .child(if activity.needs_you {
                                "Needs you".to_owned()
                            } else {
                                activity.step.clone()
                            }),
                    )
                    .when(span >= 3 && !activity.done, |row| {
                        row.child(
                            div()
                                .flex_shrink_0()
                                .text_size(px(10.5))
                                .text_color(ink(0.4))
                                .child(elapsed(activity.started_at)),
                        )
                    })
            }))
            .when(more > 0, |list| {
                list.child(
                    div()
                        .h(px(18.))
                        .pl(px(20.))
                        .flex()
                        .items_center()
                        .text_size(px(10.5))
                        .text_color(ink(0.4))
                        .child(format!("+{more} more")),
                )
            })
            .into_any_element()
    }

    fn asks_widget(&self, span: u32, cx: &mut Context<Self>) -> AnyElement {
        let asks = &self.feed().asks;
        let Some(ask) = asks.first().cloned() else {
            return quiet("Nothing needs you").into_any_element();
        };
        let more = asks.len() - 1;
        let open = ask.session;
        let actions = if ask.approval && span >= 2 {
            let (allow, deny) = (ask.clone(), ask.clone());
            div()
                .flex()
                .gap(px(6.))
                .child(
                    pill("notch-shelf-deny", "Deny", false).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.emit(
                                NotchEvent::Decide {
                                    session: deny.session,
                                    request: deny.request.clone(),
                                    decision: Decision::Deny,
                                },
                                cx,
                            )
                        },
                    )),
                )
                .child(
                    pill("notch-shelf-allow", "Allow", true).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.emit(
                                NotchEvent::Decide {
                                    session: allow.session,
                                    request: allow.request.clone(),
                                    decision: Decision::Allow,
                                },
                                cx,
                            )
                        },
                    )),
                )
        } else {
            div().child(pill("notch-shelf-open-ask", "Open", true).on_click(
                cx.listener(move |this, _, _, cx| this.emit(NotchEvent::OpenSession(open), cx)),
            ))
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(provider_glyph(&ask.provider, 12.))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(11.5))
                            .text_color(ink(0.9))
                            .child(ask.title.clone()),
                    ),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.))
                    .text_color(ink(0.55))
                    .child(ask.label.clone()),
            )
            .child(
                div()
                    .mt_auto()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(ink(0.4))
                            .when(more > 0, |more_label| {
                                more_label.child(format!("+{more} more"))
                            }),
                    )
                    .child(actions),
            )
            .into_any_element()
    }

    fn call_widget(&self, span: u32, cx: &mut Context<Self>) -> AnyElement {
        let Some(call) = self.feed().call.clone() else {
            return quiet("No call").into_any_element();
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(icon("mic-01", 12.).text_color(accent_color()))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(11.5))
                            .text_color(ink(0.9))
                            .child(call.group),
                    ),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.))
                    .text_color(ink(0.55))
                    .child(call.status),
            )
            .when(span >= 2, |view| {
                view.child(
                    div().mt_auto().flex().justify_end().child(
                        pill("notch-hang-up", "Hang up", false).on_click(
                            cx.listener(|this, _, _, cx| this.emit(NotchEvent::HangUp, cx)),
                        ),
                    ),
                )
            })
            .into_any_element()
    }

    fn system_widget(&self, span: u32) -> AnyElement {
        let Some(stats) = self.usage else {
            return quiet("Measuring…").into_any_element();
        };
        let percent = |value: Option<f32>| {
            value.map_or_else(|| "—".to_owned(), |value| format!("{value:.0}%"))
        };
        // The memory in use where the system counts it; its pressure where
        // it only says that (macOS).
        let memory = match (stats.memory, stats.pressure) {
            (Some(memory), _) => percent(Some(memory.percent())),
            (None, Some(1)) => "Normal".to_owned(),
            (None, Some(2)) => "Tight".to_owned(),
            (None, Some(4)) => "Critical".to_owned(),
            _ => "—".to_owned(),
        };
        let share = |stats: &model::stats::Stats| stats.memory.map(|memory| memory.percent());
        // Each number with the one before it (its bar glides from there) and
        // its last minute.
        let (before, at) = match self.usage_before {
            Some((before, at)) => (Some(before), at),
            None => (None, Instant::now()),
        };
        let history = &self.usage_history;
        let mut meters = vec![
            (
                "CPU",
                percent(stats.cpu),
                stats.cpu,
                before.and_then(|before| before.cpu),
                &history.cpu,
            ),
            (
                "RAM",
                memory,
                share(&stats),
                before.as_ref().and_then(share),
                &history.memory,
            ),
        ];
        // The graphics card where the system gives its load.
        if stats.gpu.is_some() || cfg!(target_os = "windows") {
            meters.push((
                "GPU",
                percent(stats.gpu),
                stats.gpu,
                before.and_then(|before| before.gpu),
                &history.gpu,
            ));
        }
        let bar = |from: Option<f32>, level: Option<f32>| meter_bar(from, level, at);
        if span < 2 {
            // One column: a row each, its bar between the name and the number.
            return div()
                .debug_selector(|| "notch-system".into())
                .flex_1()
                .flex()
                .flex_col()
                .justify_end()
                .gap(px(5.))
                .children(meters.into_iter().map(|(name, value, level, from, _)| {
                    div()
                        .debug_selector(move || format!("notch-system-{}", name.to_lowercase()))
                        .h(px(14.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .w(px(24.))
                                .flex_shrink_0()
                                .text_size(px(10.))
                                .text_color(ink(0.45))
                                .child(name),
                        )
                        .child(bar(from, level).flex_1())
                        .child(
                            div()
                                .min_w(px(28.))
                                .flex_shrink_0()
                                .flex()
                                .justify_end()
                                .text_size(px(11.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(ink(0.92))
                                .child(value),
                        )
                }))
                .into_any_element();
        }
        let detail = stats.memory.map(|memory| memory.label());
        div()
            .debug_selector(|| "notch-system".into())
            .flex_1()
            .flex()
            .gap(px(14.))
            .children(meters.into_iter().map(|(name, value, level, from, past)| {
                let caption = match (&detail, name) {
                    (Some(detail), "RAM") => format!("RAM {detail}"),
                    _ => name.to_owned(),
                };
                div()
                    .debug_selector(move || format!("notch-system-{}", name.to_lowercase()))
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .justify_end()
                    .gap(px(2.))
                    // Its last minute, drawn behind the number.
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .child(graph(past.iter().copied().collect())),
                    )
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(ink(0.92))
                            .child(value),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(10.))
                            .text_color(ink(0.45))
                            .child(caption),
                    )
                    .child(bar(from, level))
            }))
            .into_any_element()
    }

    fn new_widget(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("notch-new-chat")
            .debug_selector(|| "notch-new-chat".into())
            .role(Role::Button)
            .aria_label("New chat")
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .cursor_pointer()
            .hover(|style| style.bg(ink(0.06)))
            .on_click(cx.listener(|this, _, _, cx| this.emit(NotchEvent::NewChat, cx)))
            .child(icon("plus", 16.).text_color(ink(0.85)))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(ink(0.6))
                    .child("Quick composer"),
            )
            .into_any_element()
    }
}

impl Notch {
    fn recent_widget(&self, span: u32, cx: &mut Context<Self>) -> AnyElement {
        let recent = &self.feed().recent;
        if recent.is_empty() {
            return quiet("No chats yet").into_any_element();
        }
        let now = now_ms();
        div()
            .flex()
            .flex_col()
            .children(
                recent
                    .iter()
                    .take(if span >= 3 { 3 } else { 2 })
                    .map(|recent| {
                        let id = recent.id;
                        super::pages::session_row(
                            ("notch-shelf-recent", id as u64),
                            provider_glyph(&recent.provider, 13.),
                            &recent.title,
                            model::ago(recent.updated_at, now),
                            ink(0.4),
                        )
                        .h(px(22.))
                        .px(px(2.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.emit(NotchEvent::OpenSession(id), cx)
                        }))
                    }),
            )
            .into_any_element()
    }

    fn today_widget(&self) -> AnyElement {
        let Some(next) = self.feed().upcoming.first() else {
            return quiet("No reminders").into_any_element();
        };
        let due = chrono::DateTime::from_timestamp_millis(next.due_at as i64)
            .map(|at| at.with_timezone(&chrono::Local).format("%H:%M").to_string())
            .unwrap_or_default();
        let more = self.feed().upcoming.len() - 1;
        div()
            .flex_1()
            .flex()
            .flex_col()
            .justify_end()
            .child(
                div()
                    .text_size(px(20.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(ink(0.95))
                    .child(due),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.))
                    .text_color(ink(0.6))
                    .child(next.title.clone()),
            )
            .when(more > 0, |view| {
                view.child(
                    div()
                        .text_size(px(10.5))
                        .text_color(ink(0.4))
                        .child(format!("+{more} more")),
                )
            })
            .into_any_element()
    }
}

fn clock_widget() -> AnyElement {
    let now = chrono::Local::now();
    div()
        .flex_1()
        .flex()
        .flex_col()
        .justify_end()
        .child(
            div()
                .text_size(px(22.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(ink(0.95))
                // The minute rolls over instead of jumping.
                .child(crate::text_morph::text_morph(
                    "notch-clock",
                    now.format("%H:%M").to_string(),
                )),
        )
        .child(
            div()
                .text_size(px(10.5))
                .text_color(ink(0.5))
                .child(crate::text_morph::text_morph(
                    "notch-clock-date",
                    now.format("%a %e %b").to_string(),
                )),
        )
        .into_any_element()
}

/// A widget picked up while arranging, and the card that follows the
/// pointer while it is carried.
#[derive(Clone)]
pub(super) struct WidgetDrag {
    id: String,
    name: &'static str,
}

impl Render for WidgetDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(28.))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(6.))
            .bg(black(0.92))
            .border_1()
            .border_color(accent_color())
            .text_size(px(11.5))
            .text_color(ink(0.95))
            .cursor(gpui::CursorStyle::ClosedHand)
            .child(icon("grip-vertical", 10.).text_color(ink(0.6)))
            .child(self.name)
    }
}
