//! Pictures in replies: the ones a provider made (`GeneratedImage.tsx`) and
//! the local images or videos a reply points at (`DocumentImage.tsx`,
//! `LocalMedia.tsx`). Web images are never fetched; their alt text stands in.
use super::attachments::Attachment;
use super::*;
use gpui::AnyElement;
use monocode_core::images::GeneratedImage;
use monocode_core::transcript::media::{self, Kind};
use std::path::PathBuf;

fn alt_text(text: String) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .italic()
        .text_size(px(13.))
        .line_height(px(20.))
        .text_color(content(0.45))
        .child(text)
}

impl TranscriptView {
    pub(super) fn reply_image(
        &mut self,
        message_start: usize,
        image: &GeneratedImage,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let alt = image
            .alt
            .clone()
            .map(|alt| alt.trim().to_owned())
            .filter(|alt| !alt.is_empty());
        let file = Attachment::image_file(PathBuf::from(&image.path), image.name.clone());
        let tile = self.image_tile(&file, &format!("generated-{message_start}"), 0, true, cx);
        div()
            .id(("generated-image", message_start))
            .debug_selector(move || format!("generated-image-{message_start}"))
            .role(Role::Group)
            .aria_label(alt.clone().unwrap_or_else(|| "Generated image".into()))
            .w_full()
            .min_w_0()
            .py_2()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(tile)
            .when_some(alt, |d, alt| {
                d.child(
                    div()
                        .max_w(px(attachments::PHOTO_SIDE * 2.))
                        .text_size(px(12.))
                        .line_height(px(18.))
                        .text_color(content(0.45))
                        .child(alt),
                )
            })
            .into_any_element()
    }

    /// `![alt](src)`, or a lone link to a local image or video.
    pub(super) fn markdown_media(
        &mut self,
        key: (usize, usize),
        src: &str,
        alt: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let home = monocode_core::accounts::home();
        let found = media::resolve(src, self.file_link_base.as_deref(), home.as_deref());
        let name = |path: &PathBuf| {
            if alt.trim().is_empty() {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| src.to_owned())
            } else {
                alt.trim().to_owned()
            }
        };
        match found {
            Some(media::Media {
                kind: Kind::Image,
                path,
            }) => {
                let file = Attachment::image_file(path.clone(), name(&path));
                let tile =
                    self.image_tile(&file, &format!("md-image-{}-{}", key.0, key.1), 0, true, cx);
                div()
                    .debug_selector(move || format!("md-image-{}-{}", key.0, key.1))
                    .w_full()
                    .min_w_0()
                    .child(tile)
                    .into_any_element()
            }
            Some(media::Media {
                kind: Kind::Video,
                path,
            }) => {
                let label = name(&path);
                let open = path.to_string_lossy().into_owned();
                div()
                    .id(SharedString::from(format!("md-video-{}-{}", key.0, key.1)))
                    .debug_selector(move || format!("md-video-{}-{}", key.0, key.1))
                    .role(Role::Button)
                    .aria_label(format!("Play {label}"))
                    .tooltip(chrome_tip(format!("Play {label}"), None))
                    .max_w_full()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(content(0.1))
                    .bg(content(0.04))
                    .cursor_pointer()
                    .hover(|s| s.bg(content(0.08)))
                    .child(icon("play", 14.).text_color(content(0.6)))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .text_color(content(0.8))
                            .child(label),
                    )
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        window.dispatch_action(
                            Box::new(crate::file_editor::OpenPath(open.clone())),
                            cx,
                        );
                    })
                    .into_any_element()
            }
            // A web image or an unknown file: its words stand in for it.
            None => alt_text(if alt.trim().is_empty() {
                src.to_owned()
            } else {
                alt.trim().to_owned()
            })
            .into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn row(role: &str, text: &str) -> Row {
        Row {
            role: role.into(),
            text: text.into(),
            first: true,
            sent_at: None,
        }
    }

    fn png(path: &std::path::Path) {
        image::RgbaImage::from_pixel(4, 2, image::Rgba([0, 128, 255, 255]))
            .save(path)
            .unwrap();
    }

    #[gpui::test]
    fn replies_show_local_images_videos_and_alt_text(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let dir = std::env::temp_dir().join(format!("monocode-media-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        png(&dir.join("shot.png"));
        let base = dir.to_string_lossy().into_owned();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = TranscriptView::new();
                    view.file_link_base = Some(base.clone());
                    view.patch(
                        0,
                        vec![
                            row("user", "Show me"),
                            row(
                                "assistant",
                                "Here it is:\n\n![The screen](shot.png)\n\n[clip](./demo.mp4)\n\n![Badge](https://example.com/badge.svg)",
                            ),
                        ],
                        cx,
                    );
                    view
                })
            })
            .unwrap()
        });
        let view = window.root(cx).unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        view.read_with(visual, |view, _| {
            let parts: Vec<_> = view
                .rows
                .iter()
                .filter_map(|row| match &row.body {
                    Body::Markdown(md) => match &md.part {
                        Part::Image { src, alt } => Some((src.clone(), alt.clone())),
                        _ => None,
                    },
                    _ => None,
                })
                .collect();
            assert_eq!(
                parts,
                vec![
                    ("shot.png".to_string(), "The screen".to_string()),
                    ("./demo.mp4".to_string(), "clip".to_string()),
                    (
                        "https://example.com/badge.svg".to_string(),
                        "Badge".to_string()
                    ),
                ]
            );
        });
        let image = visual
            .debug_bounds("md-image-1-17")
            .expect("the local image is drawn");
        assert!(image.size.height > px(20.), "{image:?}");
        assert!(visual.debug_bounds("md-video-1-18").is_some());
        // The picture opens full screen.
        let tile = visual.debug_bounds("transcript-image-0").expect("tile");
        let opened = std::rc::Rc::new(std::cell::Cell::new(false));
        let seen = opened.clone();
        visual.update(|window, cx| {
            let _ = window;
            cx.on_action(move |_: &crate::image_lightbox::Open, _| seen.set(true));
        });
        visual.simulate_click(tile.center(), Default::default());
        assert!(opened.get());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[gpui::test]
    fn generated_images_are_drawn_with_their_prompt(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let dir = std::env::temp_dir().join(format!("monocode-generated-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fox.png");
        png(&path);
        let image = GeneratedImage {
            path: path.to_string_lossy().into_owned(),
            name: "fox.png".into(),
            mime_type: "image/png".into(),
            size: 1,
            alt: Some("A fox in the snow".into()),
        };
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = TranscriptView::new();
                    view.patch(
                        0,
                        vec![
                            row("user", "Draw a fox"),
                            row(
                                monocode_core::images::REPLY_ROLE,
                                &serde_json::to_string(&image).unwrap(),
                            ),
                            row("assistant", "Here is your fox."),
                        ],
                        cx,
                    );
                    view
                })
            })
            .unwrap()
        });
        let view = window.root(cx).unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        view.read_with(visual, |view, _| {
            assert!(
                view.rows
                    .iter()
                    .any(|row| matches!(&row.body, Body::Image(shown) if **shown == image))
            );
        });
        let bounds = visual
            .debug_bounds("generated-image-1")
            .expect("the picture is drawn");
        assert!(bounds.size.height > px(40.));
        let _ = std::fs::remove_dir_all(dir);
    }
}
