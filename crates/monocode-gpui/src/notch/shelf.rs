//! The open notch's shelf: three columns of widgets the user picks, orders
//! and widens, each reading what the app already keeps. The pencil turns it
//! into its arranging: move, narrow, widen or take out each widget, and add
//! what still fits. Each change is saved when it is made.
use super::*;
use monocode_core::notch::{Activity, Widget};

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
                .map(|index| self.cell(&shelf, index, width, cx))
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
            _ => div().into_any_element(),
        };
        let editing = self.editing;
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
            .when(editing, |cell| cell.border_1().border_color(ink(0.18)))
            .child(label(model::widget_name(&id)))
            .child(div().flex_1().min_h_0().flex().flex_col().child(content))
            .when(editing, |cell| {
                cell.child(self.arranging(&widget, index, shelf.len(), cx))
            })
            .into_any_element()
    }

    /// The arranging's tools over one widget.
    fn arranging(
        &self,
        widget: &Widget,
        index: usize,
        count: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
                            "shift" => look.shift(&id, by),
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
            .top(px(6.))
            .right(px(6.))
            .flex()
            .gap(px(3.))
            .child(
                tool(key("earlier"), "Move earlier", "chevron-left", index > 0)
                    .when(index > 0, |tool| tool.on_click(action(-1, "shift"))),
            )
            .child(
                tool(
                    key("later"),
                    "Move later",
                    "chevron-right",
                    index + 1 < count,
                )
                .when(index + 1 < count, |tool| tool.on_click(action(1, "shift"))),
            )
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
        for id in addable {
            row = row.child(
                pill(
                    gpui::SharedString::from(format!("notch-add-{id}")),
                    &format!("+ {}", model::widget_name(id)),
                    false,
                )
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
        row.into_any_element()
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
        let shown = 3;
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
        let Some(usage) = self.usage else {
            return quiet("Measuring…").into_any_element();
        };
        let cpu = usage
            .cpu_percent
            .map_or_else(|| "—".to_owned(), |cpu| format!("{cpu:.0}%"));
        let memory = match usage.memory_pressure {
            Some(1) => "Normal",
            Some(2) => "Tight",
            Some(4) => "Critical",
            _ => "—",
        };
        let stat = |name: &'static str, value: String| {
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(18.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(ink(0.92))
                        .child(value),
                )
                .child(div().text_size(px(10.5)).text_color(ink(0.45)).child(name))
        };
        div()
            .flex_1()
            .flex()
            .items_end()
            .gap(px(18.))
            .child(stat("CPU", cpu))
            .when(span >= 2, |row| {
                row.child(stat("Memory", memory.to_owned()))
            })
            .when(span < 2, |row| {
                row.child(
                    div()
                        .text_size(px(10.5))
                        .text_color(ink(0.45))
                        .child(memory),
                )
            })
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
                .child(now.format("%H:%M").to_string()),
        )
        .child(
            div()
                .text_size(px(10.5))
                .text_color(ink(0.5))
                .child(now.format("%a %e %b").to_string()),
        )
        .into_any_element()
}
