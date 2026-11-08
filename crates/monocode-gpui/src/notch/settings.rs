//! «Notch settings», opened from the notch itself: a sidebar of pages
//! (General, Notices, Widgets, Look) beside the page's choices. Each choice
//! is saved as it is made and the notch reshapes at once.
use super::*;

/// The settings' pages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Page {
    #[default]
    General,
    Notices,
    Widgets,
    Look,
}

impl Page {
    const ALL: [Page; 4] = [Page::General, Page::Notices, Page::Widgets, Page::Look];

    fn name(self) -> &'static str {
        match self {
            Page::General => "General",
            Page::Notices => "Notices",
            Page::Widgets => "Widgets",
            Page::Look => "Look",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Page::General => "settings",
            Page::Notices => "bell",
            Page::Widgets => "dashboard",
            Page::Look => "palette",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Page::General => "notch-page-general",
            Page::Notices => "notch-page-notices",
            Page::Widgets => "notch-page-widgets",
            Page::Look => "notch-page-look",
        }
    }
}

/// A row: the setting's name and a hint, and its choices on the right.
fn row(name: &'static str, hint: &'static str, choices: impl IntoElement) -> gpui::Div {
    div()
        .min_h(px(34.))
        .py(px(3.))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().text_size(px(12.)).text_color(ink(0.88)).child(name))
                .when(!hint.is_empty(), |label| {
                    label.child(div().text_size(px(10.5)).text_color(ink(0.4)).child(hint))
                }),
        )
        .child(div().flex_shrink_0().child(choices))
}

/// A segmented choice; the chosen one is lit in the accent.
fn segment(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    chosen: bool,
) -> gpui::Stateful<gpui::Div> {
    let id = id.into();
    let selector = format!("{id}");
    div()
        .id(id)
        .debug_selector(move || selector.clone())
        .role(Role::RadioButton)
        .aria_label(label)
        .h(px(22.))
        .min_w(px(22.))
        .px(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(11.5))
        .cursor_pointer()
        .when(chosen, |segment| {
            segment.bg(accent_color()).text_color(white(1.))
        })
        .when(!chosen, |segment| {
            segment
                .bg(ink(0.07))
                .text_color(ink(0.65))
                .hover(|style| style.bg(ink(0.14)).text_color(ink(0.9)))
        })
        .child(label)
}

fn segments(children: impl IntoIterator<Item = gpui::Stateful<gpui::Div>>) -> gpui::Div {
    div().flex().gap(px(1.)).children(children)
}

/// A click that changes the look.
fn choose(
    cx: &mut Context<Notch>,
    change: impl Fn(&mut Look) + 'static,
) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static {
    cx.listener(move |this: &mut Notch, _: &gpui::ClickEvent, _, cx| this.change_look(&change, cx))
}

/// An on and off switch as two segments.
fn toggle(
    cx: &mut Context<Notch>,
    id: &'static str,
    on: bool,
    set: fn(&mut Look, bool),
) -> gpui::Div {
    segments([
        segment(gpui::SharedString::from(format!("{id}-on")), "On", on)
            .on_click(choose(cx, move |look| set(look, true))),
        segment(gpui::SharedString::from(format!("{id}-off")), "Off", !on)
            .on_click(choose(cx, move |look| set(look, false))),
    ])
}

impl Notch {
    pub(super) fn settings_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let mut sidebar = div()
            .w(px(108.))
            .flex_shrink_0()
            .pr(px(8.))
            .mr(px(10.))
            .border_r_1()
            .border_color(ink(0.08))
            .flex()
            .flex_col()
            .gap(px(2.));
        for page in Page::ALL {
            let chosen = self.settings_page == page;
            sidebar = sidebar.child(
                div()
                    .id(page.id())
                    .debug_selector(move || page.id().into())
                    .role(Role::Tab)
                    .aria_label(page.name())
                    .h(px(28.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .when(chosen, |item| item.bg(ink(0.1)))
                    .when(!chosen, |item| item.hover(|style| style.bg(ink(0.05))))
                    .child(icon(page.icon(), 13.).text_color(ink(if chosen { 0.95 } else { 0.5 })))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(ink(if chosen { 0.95 } else { 0.6 }))
                            .child(page.name()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings_page = page;
                        cx.notify();
                    })),
            );
        }
        let page = match self.settings_page {
            Page::General => self.general_page(cx),
            Page::Notices => self.notices_page(cx),
            Page::Widgets => self.widgets_page(cx),
            Page::Look => self.look_page(cx),
        };
        div()
            .debug_selector(|| "notch-settings-page".into())
            .h(px(SETTINGS_HEIGHT))
            .pt(px(6.))
            .flex()
            .child(sidebar)
            .child(
                div()
                    .id("notch-settings-body")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .child(page),
            )
            .into_any_element()
    }

    fn general_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let look = self.look.clone();
        div()
            .flex()
            .flex_col()
            .child(row(
                "Shape",
                if cfg!(target_os = "macos") {
                    "Around the camera, or floating"
                } else {
                    "On the top edge, or floating"
                },
                segments([
                    segment("notch-style-notch", "Notch", !look.island())
                        .on_click(choose(cx, |look| look.style = "notch".into())),
                    segment("notch-style-island", "Island", look.island())
                        .on_click(choose(cx, |look| look.style = "island".into())),
                ]),
            ))
            .child(row(
                "Opens on",
                "Resting the pointer, or a click",
                segments([
                    segment("notch-opens-hover", "Hover", !look.opens_on_click())
                        .on_click(choose(cx, |look| look.opens_on = "hover".into())),
                    segment("notch-opens-click", "Click", look.opens_on_click())
                        .on_click(choose(cx, |look| look.opens_on = "click".into())),
                ]),
            ))
            .child(row(
                "Panel width",
                "",
                segments([
                    segment("notch-width-compact", "S", look.width == "compact")
                        .on_click(choose(cx, |look| look.width = "compact".into())),
                    segment("notch-width-regular", "M", look.width == "regular")
                        .on_click(choose(cx, |look| look.width = "regular".into())),
                    segment("notch-width-wide", "L", look.width == "wide")
                        .on_click(choose(cx, |look| look.width = "wide".into())),
                ]),
            ))
            .into_any_element()
    }

    fn notices_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let look = self.look.clone();
        div()
            .flex()
            .flex_col()
            .child(row(
                "Finished turns",
                "A notice, or only the count",
                segments([
                    segment("notch-finished-on", "Notice", look.finished_notices)
                        .on_click(choose(cx, |look| look.finished_notices = true)),
                    segment("notch-finished-off", "Count", !look.finished_notices)
                        .on_click(choose(cx, |look| look.finished_notices = false)),
                ]),
            ))
            .child(row(
                "Notices stay",
                "Approvals stay until answered",
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        segment("notch-notice-shorter", "−", false).on_click(choose(cx, |look| {
                            look.notice_seconds = look.notice_seconds.saturating_sub(1)
                        })),
                    )
                    .child(
                        div()
                            .debug_selector(|| "notch-notice-seconds".into())
                            .w(px(34.))
                            .flex()
                            .justify_center()
                            .text_size(px(11.5))
                            .text_color(ink(0.9))
                            .child(format!("{} s", look.notice_seconds)),
                    )
                    .child(
                        segment("notch-notice-longer", "+", false)
                            .on_click(choose(cx, |look| look.notice_seconds += 1)),
                    ),
            ))
            .into_any_element()
    }

    fn look_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let look = self.look.clone();
        div()
            .flex()
            .flex_col()
            .child(row(
                "Edge line",
                "A hairline while it shows something",
                toggle(cx, "notch-edge", look.edge, |look, on| look.edge = on),
            ))
            .child(row(
                "Count at rest",
                "How many agents work, by the camera",
                toggle(cx, "notch-count", look.show_count, |look, on| {
                    look.show_count = on
                }),
            ))
            .into_any_element()
    }

    /// Every widget: on the shelf or not, its width and its place.
    fn widgets_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let look = self.look.clone();
        let mut list = div().flex().flex_col().child(row(
            "Rows",
            "",
            segments((1..=model::MAX_ROWS).map(|rows| {
                segment(
                    gpui::SharedString::from(format!("notch-rows-{rows}")),
                    ["1", "2", "3"][rows as usize - 1],
                    look.shelf_rows == rows,
                )
                .on_click(choose(cx, move |look| look.set_rows(rows)))
            })),
        ));
        let addable = look.addable();
        for (id, widths) in model::WIDGETS {
            let placed = look.shelf.iter().position(|widget| widget.id == id);
            let span = placed.map(|at| look.shelf[at].span);
            let can_add = addable.contains(&id);
            let mut controls = div().flex().items_center().gap(px(6.));
            if let Some(at) = placed {
                controls = controls
                    .child(segments(widths.iter().map(|width| {
                        let width = *width;
                        segment(
                            gpui::SharedString::from(format!("notch-span-{id}-{width}")),
                            ["1", "2", "3"][width as usize - 1],
                            span == Some(width),
                        )
                        .on_click(choose(cx, move |look| {
                            if let Some(widget) = look.shelf.iter().find(|w| w.id == id).cloned() {
                                let by = if width > widget.span { 1 } else { -1 };
                                while look.shelf.iter().any(|w| w.id == id && w.span != width)
                                    && look.resize(id, by)
                                {}
                            }
                        }))
                    })))
                    .child(
                        segment(
                            gpui::SharedString::from(format!("notch-up-{id}")),
                            "↑",
                            false,
                        )
                        .when(at == 0, |up| up.opacity(0.35))
                        .on_click(choose(cx, move |look| {
                            look.shift(id, -1);
                        })),
                    );
            }
            controls = controls.child(
                segment(
                    gpui::SharedString::from(format!("notch-show-{id}")),
                    if placed.is_some() { "Shown" } else { "Hidden" },
                    placed.is_some(),
                )
                .when(placed.is_none() && !can_add, |show| show.opacity(0.35))
                .on_click(choose(cx, move |look| {
                    if look.shelf.iter().any(|widget| widget.id == id) {
                        look.remove(id);
                    } else {
                        look.add(id);
                    }
                })),
            );
            list = list.child(row(model::widget_name(id), "", controls));
        }
        list.into_any_element()
    }
}
