//! The open notch's Clipboard page: what was copied lately, a click from
//! being copied again. The list is `monocode_core::notch::clipboard`, kept
//! in memory only; the system's clipboard is watched off the interface
//! thread (`platform::watch_clipboard`). Each row wears its kind's mark
//! (code, link, path, email, a color's swatch, a picture's thumbnail) and
//! says so in a word; the row just copied says "Copied" for a moment.
use super::*;
use monocode_core::notch::clipboard::{Clip, Kind, LARGEST, LARGEST_PICTURE};
use std::sync::Arc;

/// The page's own header: its caption and Clear.
const CLIPS_HEADER: f32 = 28.;
/// The page with nothing copied yet.
const CLIPS_EMPTY: f32 = 64.;
/// How long a row says it was copied.
const COPIED_FOR: Duration = Duration::from_millis(1400);

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
        if self.keeps_clips() && self.clips.push(text, now_ms()) && self.open {
            cx.notify();
        }
    }

    /// A picture was copied (a BMP file and its size).
    pub(crate) fn copied_picture(
        &mut self,
        bmp: Vec<u8>,
        width: u32,
        height: u32,
        cx: &mut Context<Self>,
    ) {
        if self.keeps_clips() && self.clips.push_image(bmp, width, height, now_ms()) && self.open {
            cx.notify();
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
                gpui::ImageFormat::Bmp,
                bytes.to_vec(),
            )),
            None => gpui::ClipboardItem::new_string(clip.text.clone()),
        };
        cx.write_to_clipboard(item);
        self.clips.raise(index, now_ms());
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
            count => (CLIPS_HEADER + ROW * count as f32 + 4.).min(AGENTS_MAX),
        }
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
        let image = Arc::new(gpui::Image::from_bytes(
            gpui::ImageFormat::Bmp,
            bytes.to_vec(),
        ));
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
        let mut list = div()
            .id("notch-clips-page")
            .debug_selector(|| "notch-clips-page".into())
            .h(px(self.clips_height()))
            .overflow_y_scroll()
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
                                cx.notify();
                            }),
                        ))
                    }),
            );
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
                pages::session_row(("notch-clip", index), mark, &clip.preview(), detail, color)
                    .debug_selector(move || format!("notch-clip-{index}"))
                    .role(Role::Button)
                    .aria_label(format!(
                        "Copy this {} again",
                        clip.kind.label().to_lowercase()
                    ))
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
        list.into_any_element()
    }
}
