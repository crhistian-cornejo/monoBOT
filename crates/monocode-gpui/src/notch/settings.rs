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
    /// The sign-in for the player's "Playing next".
    Music,
}

impl Page {
    const ALL: [Page; 5] = [
        Page::General,
        Page::Notices,
        Page::Widgets,
        Page::Look,
        Page::Music,
    ];

    fn name(self) -> &'static str {
        match self {
            Page::General => "General",
            Page::Notices => "Notices",
            Page::Widgets => "Widgets",
            Page::Look => "Look",
            Page::Music => "Music",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Page::General => "settings",
            Page::Notices => "bell",
            Page::Widgets => "dashboard",
            Page::Look => "palette",
            Page::Music => "queue-list",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Page::General => "notch-page-general",
            Page::Notices => "notch-page-notices",
            Page::Widgets => "notch-page-widgets",
            Page::Look => "notch-page-look",
            Page::Music => "notch-page-music",
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

/// One choice of a segmented control (`controls`' look): the chosen one
/// sits on a raised plate in the sunken track `segments` draws.
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
        .aria_selected(chosen)
        .h(px(22.))
        .min_w(px(26.))
        .px(px(9.))
        // Scaled by the notch's corner choice like the rest of it.
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(11.5))
        .cursor_pointer()
        .when(chosen, |segment| {
            segment.bg(ink(0.2)).text_color(ink(1.)).shadow(vec![
                gpui::BoxShadow::new(px(0.), px(1.), black(0.35)).blur_radius(px(2.)),
            ])
        })
        .when(!chosen, |segment| {
            segment
                .text_color(ink(0.6))
                .hover(|style| style.text_color(ink(0.92)))
        })
        .child(label)
}

/// The sunken track around a row of [`segment`]s.
fn segments(children: impl IntoIterator<Item = gpui::Stateful<gpui::Div>>) -> gpui::Div {
    div()
        .p(px(2.))
        .flex()
        .gap(px(2.))
        .rounded(px(8.))
        .bg(ink(0.07))
        .children(children)
}

/// A click that changes the look.
fn choose(
    cx: &mut Context<Notch>,
    change: impl Fn(&mut Look) + 'static,
) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static {
    cx.listener(move |this: &mut Notch, _: &gpui::ClickEvent, _, cx| this.change_look(&change, cx))
}

/// The system's switch for a setting that is on or off.
fn toggle(
    cx: &mut Context<Notch>,
    label: &'static str,
    id: &'static str,
    on: bool,
    set: fn(&mut Look, bool),
) -> gpui::Stateful<gpui::Div> {
    controls::switch(id, label, on).on_click(choose(cx, move |look| set(look, !on)))
}

impl Notch {
    pub(super) fn settings_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        // A source list, as a macOS settings window's sidebar: rounded rows,
        // the page shown on the accent.
        let mut sidebar = div()
            .w(px(112.))
            .flex_shrink_0()
            .mr(px(12.))
            .p(px(4.))
            .flex()
            .flex_col()
            .gap(px(1.))
            .rounded(px(controls::CARD_RADIUS))
            .bg(ink(0.04));
        for page in Page::ALL {
            let chosen = self.settings_page == page;
            sidebar = sidebar.child(
                div()
                    .id(page.id())
                    .debug_selector(move || page.id().into())
                    .role(Role::Tab)
                    .aria_label(page.name())
                    .aria_selected(chosen)
                    .h(px(26.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .rounded(px(controls::ROW_RADIUS))
                    .cursor_pointer()
                    .when(chosen, |item| item.bg(accent_color()))
                    .when(!chosen, |item| {
                        item.hover(|style| style.bg(ink(0.07)))
                            .active(|style| style.bg(ink(0.12)))
                    })
                    .child(icon(page.icon(), 13.).text_color(if chosen {
                        white(1.)
                    } else {
                        ink(0.55)
                    }))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(if chosen { white(1.) } else { ink(0.75) })
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
            Page::Music => self.music_page(cx),
        };
        div()
            .debug_selector(|| "notch-settings-page".into())
            .h(px(SETTINGS_HEIGHT))
            .pt(px(6.))
            .flex()
            // The source list keeps to its rows instead of reaching the
            // island's rounded foot.
            .items_start()
            .child(sidebar)
            .child(
                div()
                    .id("notch-settings-body")
                    .self_stretch()
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
            .when(platform::CLIPBOARD, |page| {
                page.child(row(
                    "Clipboard history",
                    "Text you copy, kept in memory only",
                    toggle(
                        cx,
                        "Clipboard history",
                        "notch-clipboard",
                        look.clipboard,
                        |look, on| look.clipboard = on,
                    ),
                ))
            })
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
                "Face at rest",
                "Eyes that blink and follow the pointer",
                toggle(cx, "Face at rest", "notch-face", look.face, |look, on| {
                    look.face = on
                }),
            ))
            .child(row(
                "Edge line",
                "A hairline while it shows something",
                toggle(cx, "Edge line", "notch-edge", look.edge, |look, on| {
                    look.edge = on
                }),
            ))
            .child(row(
                "Volume and brightness",
                if cfg!(target_os = "macos") {
                    "The notch answers the keys (needs Accessibility)"
                } else {
                    "The notch answers the volume keys"
                },
                segments([
                    segment("notch-keys-notch", "Notch", look.system_keys)
                        .on_click(choose(cx, |look| look.system_keys = true)),
                    segment("notch-keys-system", "System", !look.system_keys)
                        .on_click(choose(cx, |look| look.system_keys = false)),
                ]),
            ))
            .child(row(
                "Count at rest",
                "How many agents work, by the camera",
                toggle(
                    cx,
                    "Count at rest",
                    "notch-count",
                    look.show_count,
                    |look, on| look.show_count = on,
                ),
            ))
            .child(row(
                "Corners",
                "How round it opens",
                segments(crate::theme::CORNERS.iter().map(|&(id, label, _)| {
                    segment(
                        gpui::SharedString::from(format!("notch-corners-{id}")),
                        label,
                        look.corners == id,
                    )
                    .on_click(choose(cx, move |look| look.corners = id.into()))
                })),
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
