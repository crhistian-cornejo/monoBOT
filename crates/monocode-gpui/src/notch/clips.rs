//! The open notch's Clipboard page: what was copied lately, a click from
//! being copied again. The list is `monocode_core::notch::clipboard`, kept
//! in memory only; the system's clipboard is watched off the interface
//! thread (`platform::watch_clipboard`). Each row wears its kind's mark
//! (code, link, path, email, a color's swatch, a picture's thumbnail) and
//! says so in a word; the row just copied says "Copied" for a moment.
//!
//! Above the list a card shows one copy large: the one the pointer is on,
//! or the newest. A picture is drawn as big as the card allows with its size
//! beside it; code keeps its lines and indentation in the code font; text
//! wraps; a link leads with where it goes; a color shows itself and how it
//! is written. Beside it, a few facts and what can be done with it: copy it
//! again, open a link, show a path in its folder, write to an address.
use super::*;
use monocode_core::notch::clipboard::{self as clipboard, Clip, Kind, LARGEST, LARGEST_PICTURE};
use std::sync::Arc;

/// The page's own header: its caption and Clear.
const CLIPS_HEADER: f32 = 28.;
/// The page with nothing copied yet.
const CLIPS_EMPTY: f32 = 64.;
/// How long a row says it was copied.
const COPIED_FOR: Duration = Duration::from_millis(1400);
/// The card that shows one copy large, and the gap under it.
const CARD: f32 = 118.;
const CARD_GAP: f32 = 6.;
/// The room a picture, or a color, has in the card.
const PICTURE: (f32, f32) = (150., 98.);
/// The facts and actions beside the preview.
const BESIDE: f32 = 128.;
/// The lines of code, or of text, the card shows.
const LINES: usize = 6;

/// A kept picture's bytes and the image drawn from them. GPUI keys its
/// texture by the image, so each picture is decoded once.
pub(super) type Thumbnails = Vec<(Arc<[u8]>, Arc<gpui::Image>)>;

impl Notch {
    /// Whether the Clipboard page is offered: the look asks for it and this
    /// system's clipboard can be watched.
    pub(super) fn keeps_clips(&self) -> bool {
        self.look.clipboard && platform::CLIPBOARD
    }

    /// Follows the look: watches the clipboard while the history is on,
    /// and forgets everything when it is turned off.
    pub(super) fn watch_clipboard(&mut self, cx: &mut Context<Self>) {
        if !self.keeps_clips() {
            self.clips_task = None;
            self.clips.clear();
            self.thumbnails.clear();
            if self.tab == Tab::Clips {
                self.tab = Tab::Home;
            }
            return;
        }
        let (sender, copies) = async_channel::unbounded();
        if !platform::watch_clipboard(sender, LARGEST, LARGEST_PICTURE) {
            return;
        }
        // Dropping the task drops the receiver, which ends the watch.
        self.clips_task = Some(cx.spawn(async move |this, cx| {
            while let Ok(copy) = copies.recv().await {
                let kept = this.update(cx, |this, cx| match copy {
                    platform::Copied::Text(text) => this.copied(&text, cx),
                    platform::Copied::Picture(bmp, width, height) => {
                        this.copied_picture(bmp, width, height, cx)
                    }
                });
                if kept.is_err() {
                    break;
                }
            }
        }));
    }

    /// Text was copied.
    pub(crate) fn copied(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.keeps_clips() && self.clips.push(text, now_ms()) {
            // What was just copied is what the card shows.
            self.clip_shown = None;
            if self.open {
                cx.notify();
            }
        }
    }

    /// A picture was copied (a `platform::PICTURE` file and its size).
    pub(crate) fn copied_picture(
        &mut self,
        bmp: Vec<u8>,
        width: u32,
        height: u32,
        cx: &mut Context<Self>,
    ) {
        if self.keeps_clips() && self.clips.push_image(bmp, width, height, now_ms()) {
            self.clip_shown = None;
            if self.open {
                cx.notify();
            }
        }
    }

    /// Copies a kept clip again; it becomes the newest and says so for a
    /// moment.
    fn copy_clip(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(clip) = self.clips.clips().get(index).cloned() else {
            return;
        };
        let item = match &clip.image {
            Some(bytes) => gpui::ClipboardItem::new_image(&gpui::Image::from_bytes(
                platform::PICTURE,
                bytes.to_vec(),
            )),
            None => gpui::ClipboardItem::new_string(clip.text.clone()),
        };
        cx.write_to_clipboard(item);
        // It becomes the newest, and the card follows it there.
        self.clips.raise(index, now_ms());
        self.clip_shown = None;
        self.said_copied(cx);
    }

    /// Copies `text` as one way of writing what a clip holds (a color's
    /// `rgb()`); the clipboard's watch adds it to the list by itself.
    fn copy_text(&mut self, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        self.said_copied(cx);
    }

    /// The newest row and the card say "Copied" for a moment.
    fn said_copied(&mut self, cx: &mut Context<Self>) {
        self.clip_copied = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FOR).await;
            let _ = this.update(cx, |this, cx| {
                this.clip_copied = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn clips_height(&self) -> f32 {
        match self.clips.clips().len() {
            0 => CLIPS_EMPTY,
            count => (CLIPS_HEADER + CARD + CARD_GAP + ROW * count as f32 + 4.).min(AGENTS_MAX),
        }
    }

    /// Which copy the card shows: the one last pointed at while it is still
    /// kept, or the newest.
    pub(super) fn shown_clip(&self) -> Option<usize> {
        let clips = self.clips.clips();
        self.clip_shown
            .and_then(|at| clips.iter().position(|clip| clip.at == at))
            .or((!clips.is_empty()).then_some(0))
    }

    /// The copy itself, as large as the card allows.
    fn clip_preview(&mut self, clip: &Clip) -> AnyElement {
        let text = clip.text.trim();
        let lead = |words: String| {
            div()
                .truncate()
                .text_size(px(13.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(ink(0.95))
                .child(words)
        };
        let under = |words: String, lines: usize| {
            div()
                .text_size(px(11.))
                .line_height(px(15.))
                .line_clamp(lines)
                .text_color(ink(0.5))
                .child(words)
        };
        let column = || div().min_w_0().flex_1().flex().flex_col().gap(px(4.));
        match (clip.kind, &clip.image) {
            (Kind::Image(..), Some(bytes)) => div()
                .w(px(PICTURE.0))
                .h(px(PICTURE.1))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(ink(0.05))
                .child(
                    gpui::img(self.thumbnail(bytes))
                        .max_w(px(PICTURE.0))
                        .max_h(px(PICTURE.1))
                        .object_fit(gpui::ObjectFit::Contain),
                )
                .into_any_element(),
            // The copied color itself: the one place a color is not the
            // theme's.
            (Kind::Color(color), _) => div()
                .w(px(PICTURE.1))
                .h(px(PICTURE.1))
                .flex_shrink_0()
                .border_1()
                .border_color(ink(0.2))
                .bg(gpui::rgb(color))
                .into_any_element(),
            (Kind::Code, _) => {
                let (lines, more) = clip.code_lines(LINES);
                column()
                    .gap(px(0.))
                    .font_family(crate::fonts::mono())
                    .text_size(px(10.5))
                    .line_height(px(15.))
                    .text_color(ink(0.86))
                    .children(lines.into_iter().map(|line| {
                        // A blank line keeps its height.
                        div().h(px(15.)).truncate().child(line)
                    }))
                    .when(more > 0, |code| {
                        code.child(div().text_color(ink(0.4)).child(format!(
                            "+{more} more {}",
                            if more == 1 { "line" } else { "lines" }
                        )))
                    })
                    .into_any_element()
            }
            (Kind::Link, _) => {
                let host = clipboard::host(text).unwrap_or(text).to_owned();
                // What follows the host: the page it leads to.
                let rest = text
                    .split_once("://")
                    .and_then(|(_, rest)| rest.split_once('/'))
                    .map(|(_, page)| format!("/{page}"))
                    .unwrap_or_default();
                column()
                    .child(lead(host))
                    .when(!rest.is_empty(), |link| link.child(under(rest, 4)))
                    .into_any_element()
            }
            (Kind::Path, _) => {
                let (name, folder) = clipboard::path_parts(text);
                column()
                    .child(lead(name.to_owned()))
                    .when(!folder.is_empty(), |path| {
                        path.child(under(folder.to_owned(), 3))
                    })
                    .into_any_element()
            }
            (Kind::Email, _) => column().child(lead(text.to_owned())).into_any_element(),
            // Text, and a picture whose bytes were not kept.
            _ => column()
                .child(
                    div()
                        .text_size(px(11.5))
                        .line_height(px(15.5))
                        .line_clamp(LINES)
                        .text_color(ink(0.86))
                        // Enough for the lines shown; the rest is in the
                        // clipboard, not on screen.
                        .child(text.chars().take(600).collect::<String>()),
                )
                .into_any_element(),
        }
    }

    /// The card: the copy large, and beside it what it is and what can be
    /// done with it.
    fn clip_card(&mut self, index: usize, clip: &Clip, cx: &mut Context<Self>) -> AnyElement {
        let just_copied = index == 0 && self.clip_copied.is_some();
        let preview = self.clip_preview(clip);
        let text = clip.text.trim().to_owned();
        let mut facts = div().flex().flex_col().gap(px(1.)).child(
            div()
                .text_size(px(11.5))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(ink(0.92))
                .child(clip.kind.label()),
        );
        for fact in clip.facts() {
            facts = facts.child(
                div()
                    .truncate()
                    .text_size(px(10.5))
                    .line_height(px(14.))
                    .text_color(ink(0.5))
                    .child(fact),
            );
        }
        let mut actions = div().flex().flex_wrap().gap(px(4.));
        match clip.kind {
            // A color is copied the way it will be written.
            Kind::Color(color) => {
                for (at, written) in clipboard::color_formats(color).into_iter().enumerate() {
                    let copy = written.clone();
                    let id = gpui::SharedString::from(format!("notch-clip-format-{at}"));
                    actions = actions.child(pill(id, &written, at == 0).on_click(
                        cx.listener(move |this, _, _, cx| this.copy_text(copy.clone(), cx)),
                    ));
                }
            }
            kind => {
                actions = actions.child(
                    pill(
                        "notch-clip-copy",
                        if just_copied { "Copied" } else { "Copy" },
                        true,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.copy_clip(index, cx))),
                );
                let more = match kind {
                    Kind::Link => Some(("notch-clip-open", "Open", text.clone())),
                    Kind::Email => Some(("notch-clip-write", "Write", format!("mailto:{text}"))),
                    _ => None,
                };
                if let Some((id, label, url)) = more {
                    actions = actions
                        .child(pill(id, label, false).on_click(move |_, _, cx| cx.open_url(&url)));
                }
                if kind == Kind::Path {
                    let path = std::path::PathBuf::from(&text);
                    actions = actions.child(
                        pill("notch-clip-show", "Show", false)
                            .on_click(move |_, _, cx| cx.reveal_path(&path)),
                    );
                }
            }
        }
        div()
            .debug_selector(|| "notch-clip-card".into())
            .h(px(CARD))
            .mb(px(CARD_GAP))
            .flex_shrink_0()
            .p(px(10.))
            .flex()
            .gap(px(12.))
            .overflow_hidden()
            .bg(ink(0.07))
            .child(preview)
            .child(
                div()
                    // Beside a picture or a color the facts take the room
                    // left; beside words they keep to their column.
                    .map(|beside| match clip.kind {
                        Kind::Image(..) | Kind::Color(_) => beside.min_w_0().flex_1(),
                        _ => beside.w(px(BESIDE)).flex_shrink_0(),
                    })
                    .flex()
                    .flex_col()
                    .justify_between()
                    .gap(px(6.))
                    .child(facts)
                    .child(actions),
            )
            .into_any_element()
    }

    /// A picture's image, decoded the first time its row is drawn. Pictures
    /// no longer kept are let go.
    fn thumbnail(&mut self, bytes: &Arc<[u8]>) -> Arc<gpui::Image> {
        if let Some((_, image)) = self
            .thumbnails
            .iter()
            .find(|(kept, _)| Arc::ptr_eq(kept, bytes))
        {
            return image.clone();
        }
        let image = Arc::new(gpui::Image::from_bytes(platform::PICTURE, bytes.to_vec()));
        self.thumbnails.push((bytes.clone(), image.clone()));
        image
    }

    fn clip_mark(&mut self, clip: &Clip) -> AnyElement {
        let glyph = |path: &'static str| icon(path, 12.).text_color(ink(0.55)).into_any_element();
        match (clip.kind, &clip.image) {
            (Kind::Image(..), Some(bytes)) => gpui::img(self.thumbnail(bytes))
                .size(px(14.))
                .object_fit(gpui::ObjectFit::Cover)
                .into_any_element(),
            (Kind::Image(..), None) => glyph("image-01"),
            (Kind::Text, _) => glyph("text-quote"),
            (Kind::Link, _) => glyph("external-link"),
            (Kind::Code, _) => glyph("terminal"),
            (Kind::Path, _) => glyph("folder-01"),
            (Kind::Email, _) => glyph("mail"),
            // The copied color itself: the one place a color is not the
            // theme's.
            (Kind::Color(color), _) => div()
                .size(px(10.))
                .border_1()
                .border_color(ink(0.25))
                .bg(gpui::rgb(color))
                .into_any_element(),
        }
    }

    pub(super) fn clips_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let clips = self.clips.clips().to_vec();
        self.thumbnails.retain(|(bytes, _)| {
            clips.iter().any(|clip| {
                clip.image
                    .as_ref()
                    .is_some_and(|kept| Arc::ptr_eq(kept, bytes))
            })
        });
        let shown = self.shown_clip();
        let card = shown.and_then(|index| {
            let clip = clips.get(index)?.clone();
            Some(self.clip_card(index, &clip, cx))
        });
        let page = div()
            .debug_selector(|| "notch-clips-page".into())
            .h(px(self.clips_height()))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(CLIPS_HEADER))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(pages::caption("Copied lately").h_auto().pt_0())
                    .when(!clips.is_empty(), |header| {
                        header.child(pill("notch-clips-clear", "Clear", false).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.clips.clear();
                                this.clip_copied = None;
                                this.clip_shown = None;
                                cx.notify();
                            }),
                        ))
                    }),
            )
            .children(card);
        // The rows scroll under the card, which stays where it is.
        let mut list = div()
            .id("notch-clips-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col();
        if clips.is_empty() {
            list = list.child(
                div()
                    .px(px(6.))
                    .text_size(px(11.5))
                    .text_color(ink(0.4))
                    .child("Nothing copied yet. What you copy shows here, in memory only."),
            );
        }
        let now = now_ms();
        for (index, clip) in clips.into_iter().enumerate() {
            let mark = self.clip_mark(&clip);
            let at = clip.at;
            // A copied row is always the newest.
            let just_copied = index == 0 && self.clip_copied.is_some();
            let (detail, color) = if just_copied {
                ("Copied".to_owned(), accent_color())
            } else {
                (
                    format!("{} · {}", clip.kind.label(), model::ago(clip.at, now)),
                    ink(0.4),
                )
            };
            let remove = div()
                .id(("notch-clip-remove", index))
                .debug_selector(move || format!("notch-clip-remove-{index}"))
                .role(Role::Button)
                .aria_label("Remove from the history")
                .size(px(18.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .hover(|style| style.bg(ink(0.14)))
                .child(icon("cancel-01", 10.).text_color(ink(0.45)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.clips.remove(index);
                    this.clip_copied = None;
                    cx.notify();
                }));
            list = list.child(
                // The label is told apart by the copy, not by its place: a
                // new copy slides the rows down without each one's label
                // turning into the next one's.
                pages::session_row_keyed(
                    ("notch-clip", index),
                    ("notch-clip-detail", clip.at),
                    mark,
                    &clip.preview(),
                    detail,
                    color,
                )
                .debug_selector(move || format!("notch-clip-{index}"))
                .role(Role::Button)
                .aria_label(format!(
                    "Copy this {} again",
                    clip.kind.label().to_lowercase()
                ))
                // The row the card shows is marked, and pointing at another
                // shows that one: no click needed to look.
                .when(shown == Some(index) && !just_copied, |row| {
                    row.bg(ink(0.05))
                })
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    if this.clip_shown != Some(at) {
                        this.clip_shown = Some(at);
                        cx.notify();
                    }
                }))
                .when(just_copied, |row| {
                    row.bg(ink(0.07)).child(
                        div()
                            .debug_selector(|| "notch-clip-copied".into())
                            .flex_shrink_0()
                            .child(icon("check", 11.).text_color(accent_color())),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| this.copy_clip(index, cx)))
                .child(remove),
            );
        }
        page.child(list).into_any_element()
    }
}
