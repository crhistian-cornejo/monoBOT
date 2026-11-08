//! The Images bot's side panel: its Gallery (every picture its chat holds,
//! in a masonry wall that keeps each picture's shape, with the brief, style,
//! format and date on hover) and its Styles (the preset looks and formats,
//! which go into the draft as `/screenprint`, `/portrait` like the slash
//! menu's). The pictures and presets are core's (monocode_core::images).
use super::*;
use crate::image_lightbox;
use gpui::{CursorStyle, ElementId, FontWeight, ObjectFit, img};
use monocode_core::images::{
    gallery::{self, Filter, Item},
    styles::{self, FORMATS, Orientation, STYLES},
};

/// Narrowest a gallery column gets before the wall drops one.
const COLUMN_MIN: f32 = 150.;

impl Monos {
    /// The bot's pictures, read from its chat again only when the chat changed.
    fn gallery_items(&mut self, mono: &Mono, cx: &mut Context<Self>) -> Vec<Item> {
        let key = match self.chats.get(&mono.id) {
            Some(chat) => {
                let rows = chat.read(cx).mono_rows(cx);
                let text: usize = rows.iter().map(|row| row.text.len()).sum();
                (mono.id.clone(), rows.len(), text)
            }
            None => (mono.id.clone(), 0, 0),
        };
        if self.gallery_items.as_ref().is_none_or(|(at, _)| *at != key) {
            let items = self
                .chats
                .get(&mono.id)
                .map(|chat| gallery::from_rows(chat.read(cx).mono_rows(cx)))
                .unwrap_or_default();
            self.gallery_items = Some((key, items));
        }
        self.gallery_items
            .as_ref()
            .map(|(_, items)| items.clone())
            .unwrap_or_default()
    }

    /// Hands a preset to the bot's draft, as the slash menu would.
    fn pick_preset(
        &mut self,
        mono: &str,
        id: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(chat) = self.chats.get(mono).cloned() {
            chat.update(cx, |chat, cx| chat.mono_pick_preset(id, window, cx));
            cx.notify();
        }
    }

    pub(super) fn gallery_view(&mut self, mono: &Mono, cx: &mut Context<Self>) -> AnyElement {
        let items = self.gallery_items(mono, cx);
        if items.is_empty() {
            return panels::empty_state(
                "image-01",
                &format!(
                    "Every picture {} makes lands here, newest first. Ask for one, or pick a look in Styles.",
                    mono.label()
                ),
            );
        }
        let filter = self.gallery_filter;
        let count = |orientation| {
            items
                .iter()
                .filter(|item| item.orientation() == orientation)
                .count()
        };
        let chips = [
            (Filter::All, "All", items.len()),
            (
                Filter::Only(Orientation::Portrait),
                "Portrait",
                count(Orientation::Portrait),
            ),
            (
                Filter::Only(Orientation::Landscape),
                "Landscape",
                count(Orientation::Landscape),
            ),
            (
                Filter::Only(Orientation::Square),
                "Square",
                count(Orientation::Square),
            ),
        ];
        let shown: Vec<&Item> = items.iter().filter(|item| filter.keeps(item)).collect();
        let width = self
            .owner
            .upgrade()
            .map(|owner| owner.read(cx).right_panel_width())
            .unwrap_or(crate::right_panel::WIDTH_DEFAULT);
        let columns = ((width - 24.) / COLUMN_MIN).floor().clamp(2., 4.) as usize;
        let wall = gallery::masonry(&shown, columns);
        let chat = self.chats.get(&mono.id).cloned();
        let now = chrono::Local::now();
        let label = mono.label();
        div()
            .id("image-gallery")
            .debug_selector(|| "image-gallery".into())
            .role(Role::Region)
            .aria_label(format!("{label}'s gallery"))
            .size_full()
            .overflow_y_scroll()
            .px_3()
            .pt_2()
            .pb_4()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .id("gallery-filters")
                    .role(Role::TabList)
                    .aria_label("Show")
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .children(
                        chips
                            .into_iter()
                            .filter(|(chip, _, n)| *chip == Filter::All || *n > 0)
                            .map(|(chip, name, n)| {
                                filter_chip(chip, name, n, chip == filter).on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.gallery_filter = chip;
                                        cx.notify();
                                    },
                                ))
                            }),
                    ),
            )
            .child(if shown.is_empty() {
                panels::empty("No pictures in this shape yet.")
            } else {
                div()
                    .flex()
                    .items_start()
                    .gap_2()
                    .children(wall.into_iter().map(|column| {
                        div().flex_1().min_w_0().flex().flex_col().gap_2().children(
                            column
                                .into_iter()
                                .map(|index| tile(shown[index], chat.clone(), now, cx)),
                        )
                    }))
                    .into_any_element()
            })
            .into_any_element()
    }

    pub(super) fn styles_view(&mut self, mono: &Mono, cx: &mut Context<Self>) -> AnyElement {
        let picked = self
            .chats
            .get(&mono.id)
            .map(|chat| chat.read(cx).mono_draft_words(cx))
            .unwrap_or_default();
        let is_picked = |id: &str| picked.iter().any(|word| word == id);
        // Each look's newest picture, as its thumbnail.
        let items = self.gallery_items(mono, cx);
        let latest = |id: &str| {
            items
                .iter()
                .find(|item| item.style.as_deref() == Some(id) && !item.making())
                .and_then(|item| item.path.clone())
        };
        let id = mono.id.clone();
        div()
            .id("image-styles")
            .debug_selector(|| "image-styles".into())
            .size_full()
            .overflow_y_scroll()
            .px_3()
            .pt_2()
            .pb_4()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .px_1()
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(content(0.5))
                    .child(format!(
                        "Pick a format and a look: they go into your message to {} as slash commands, like typing / in the chat.",
                        mono.label()
                    )),
            )
            .child(section_label("Format"))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1p5()
                    .children(FORMATS.iter().map(|format| {
                        let id = id.clone();
                        format_chip(format, is_picked(format.id)).on_click(cx.listener(
                            move |this, _, window, cx| this.pick_preset(&id, format.id, window, cx),
                        ))
                    })),
            )
            .child(div().pt_2().child(section_label("Style")))
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_2()
                    .children(STYLES.iter().map(|style| {
                        let id = id.clone();
                        style_card(style, latest(style.id), is_picked(style.id)).on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.pick_preset(&id, style.id, window, cx)
                            }),
                        )
                    })),
            )
            .into_any_element()
    }
}

fn filter_chip(
    filter: Filter,
    name: &'static str,
    count: usize,
    selected: bool,
) -> gpui::Stateful<gpui::Div> {
    let id = match filter {
        Filter::All => "gallery-all",
        Filter::Only(Orientation::Portrait) => "gallery-portrait",
        Filter::Only(Orientation::Landscape) => "gallery-landscape",
        Filter::Only(Orientation::Square) => "gallery-square",
    };
    div()
        .id(id)
        .debug_selector(move || id.into())
        .role(Role::Tab)
        .aria_label(format!("{name}, {count}"))
        .aria_toggled(if selected {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .cursor_pointer()
        .flex()
        .items_center()
        .gap_1()
        .h(px(24.))
        .px_2()
        .rounded_md()
        .text_size(px(12.))
        .text_color(content(if selected { 0.95 } else { 0.6 }))
        .when(selected, |d| d.bg(content(0.09)))
        .when(!selected, |d| d.hover(|s| s.bg(content(0.05))))
        .child(name)
        .child(
            div()
                .text_color(content(if selected { 0.6 } else { 0.4 }))
                .child(count.to_string()),
        )
}

/// One picture of the wall: its own shape, opening full screen on click, with
/// its brief, look, format and date on hover, and quick actions.
fn tile(
    item: &Item,
    chat: Option<Entity<Conversation>>,
    now: chrono::DateTime<chrono::Local>,
    cx: &mut Context<Monos>,
) -> AnyElement {
    let ratio = styles::ratio(item.aspect.as_deref());
    let group: SharedString = format!("gallery-tile-{}", item.key).into();
    let title = item.title();
    let meta = [
        item.style_name().map(str::to_owned),
        Some(item.format_label()),
        item.created_at.and_then(|at| gallery::date_label(at, now)),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    let base = div()
        .id(ElementId::Name(format!("gallery-{}", item.key).into()))
        .group(group.clone())
        .relative()
        .overflow_hidden()
        .rounded_lg()
        .border_1()
        .border_color(content(0.08))
        .bg(content(0.04))
        .w_full()
        .aspect_ratio(ratio);
    let Some(path) = item.path.clone() else {
        return base
            .role(Role::Status)
            .aria_label("Making a picture")
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(11.))
            .text_color(content(0.55))
            .child("Creating…")
            .into_any_element();
    };
    let open = image_lightbox::Open {
        source: image_lightbox::Source::Path(PathBuf::from(&path)),
        alt: item.prompt.clone().into(),
    };
    let missing = title.clone();
    let reveal = PathBuf::from(&path);
    let edit = PathBuf::from(&path);
    let style = item.style.clone();
    base.debug_selector(|| "gallery-tile".into())
        .role(Role::Button)
        .aria_label(format!("{title}. {meta}"))
        .cursor(CursorStyle::PointingHand)
        .on_click(move |_, window, cx| window.dispatch_action(Box::new(open.clone()), cx))
        .child(
            img(PathBuf::from(&path))
                .size_full()
                .object_fit(ObjectFit::Cover)
                .with_fallback(move || {
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p(px(8.))
                        .text_size(px(11.))
                        .text_color(content(0.5))
                        .child(format!("Could not open {missing}"))
                        .into_any_element()
                }),
        )
        // The hover card: what it is, its look, shape and day.
        .child(
            div()
                .absolute()
                .left(px(6.))
                .right(px(6.))
                .bottom(px(6.))
                .opacity(0.)
                .group_hover(group.clone(), |s| s.opacity(1.))
                .rounded_md()
                .border_1()
                .border_color(content(0.1))
                .bg(background_base().opacity(0.92))
                .px_2()
                .py_1p5()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(content(0.9))
                        .line_clamp(2)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .line_height(px(14.))
                        .text_color(content(0.55))
                        .truncate()
                        .child(meta),
                ),
        )
        // Quick actions: change it in the chat, reuse its look, find the file.
        .child(
            div()
                .absolute()
                .top(px(6.))
                .right(px(6.))
                .opacity(0.)
                .group_hover(group, |s| s.opacity(1.))
                .flex()
                .gap(px(2.))
                .p(px(2.))
                .rounded_md()
                .border_1()
                .border_color(content(0.1))
                .bg(background_base().opacity(0.92))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .when_some(chat.clone(), |d, chat| {
                    d.child(
                        icon_button("gallery-edit", "Change in chat", "pencil-edit").on_click(
                            move |_, window, cx| {
                                cx.stop_propagation();
                                let edit = edit.clone();
                                chat.update(cx, |chat, cx| {
                                    chat.mono_attach_picture(edit, window, cx)
                                });
                            },
                        ),
                    )
                })
                .when_some(chat.zip(style), |d, (chat, style)| {
                    d.when_some(styles::style(&style), |d, style| {
                        d.child(
                            icon_button("gallery-style", "Use this look", "palette").on_click(
                                cx.listener(move |_, _, window, cx| {
                                    cx.stop_propagation();
                                    chat.update(cx, |chat, cx| {
                                        chat.mono_pick_preset(style.id, window, cx)
                                    });
                                }),
                            ),
                        )
                    })
                })
                .child(
                    icon_button("gallery-reveal", "Reveal in Finder", "folder-open").on_click(
                        move |_, _, cx| {
                            cx.stop_propagation();
                            cx.reveal_path(&reveal);
                        },
                    ),
                ),
        )
        .into_any_element()
}

/// A format: its shape drawn small, its name and ratio.
fn format_chip(format: &'static styles::Format, picked: bool) -> gpui::Stateful<gpui::Div> {
    let ratio = styles::ratio(Some(format.aspect));
    // The glyph fits a 14px box.
    let (w, h) = if ratio >= 1. {
        (14., 14. / ratio)
    } else {
        (14. * ratio, 14.)
    };
    div()
        .id(ElementId::Name(format!("format-{}", format.id).into()))
        .debug_selector(move || format!("image-format-{}", format.id))
        .role(Role::Button)
        .aria_label(format!(
            "{}, {}: {}",
            format.name, format.aspect, format.use_for
        ))
        .tooltip(text_tip(format.use_for))
        .cursor_pointer()
        .flex()
        .items_center()
        .gap_1p5()
        .h(px(28.))
        .px_2()
        .rounded_md()
        .border_1()
        .border_color(if picked { accent_color() } else { content(0.1) })
        .when(picked, |d| d.bg(accent_color().opacity(0.1)))
        .hover(|s| s.bg(content(0.05)))
        .child(
            div()
                .size(px(14.))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .w(px(w))
                        .h(px(h))
                        .rounded(px(2.))
                        .border_1()
                        .border_color(content(if picked { 0.8 } else { 0.5 })),
                ),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(content(0.85))
                .child(format.name),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(content(0.45))
                .child(format.aspect),
        )
}

/// A look: its newest picture or its palette, its name and a line on it.
fn style_card(
    style: &'static styles::Style,
    thumbnail: Option<String>,
    picked: bool,
) -> gpui::Stateful<gpui::Div> {
    // The palette is the style's own data, not the app's theme.
    let swatch = div().size_full().flex().children(
        style
            .swatch
            .map(|color| div().flex_1().h_full().bg(rgb(color))),
    );
    div()
        .id(ElementId::Name(format!("style-{}", style.id).into()))
        .debug_selector(move || format!("image-style-{}", style.id))
        .role(Role::Button)
        .aria_label(format!("{}: {}", style.name, style.blurb))
        .aria_toggled(if picked {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .cursor_pointer()
        .flex()
        .flex_col()
        .min_w_0()
        .overflow_hidden()
        .rounded_lg()
        .border_1()
        .border_color(if picked {
            accent_color()
        } else {
            content(0.08)
        })
        .when(picked, |d| d.bg(accent_color().opacity(0.08)))
        .hover(|s| s.bg(content(0.04)))
        .child(
            div()
                .w_full()
                .aspect_ratio(16. / 9.)
                .overflow_hidden()
                .bg(content(0.04))
                .child(match thumbnail {
                    Some(path) => img(PathBuf::from(path))
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .into_any_element(),
                    None => swatch.into_any_element(),
                }),
        )
        .child(
            div()
                .px_2()
                .py_1p5()
                .min_w_0()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .min_w_0()
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(content(0.9))
                        .child(div().min_w_0().truncate().child(style.name)),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .line_height(px(15.))
                        .text_color(content(0.5))
                        .truncate()
                        .child(format!("/{} · {}", style.id, style.blurb)),
                ),
        )
}
