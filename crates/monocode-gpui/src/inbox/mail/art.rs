//! A picture drawn in glyphs, in the user's accent color, as in Echo (the desktop app's
//! MailArt): it appears once the picture is read, then nothing moves. Decoration only.
use super::*;
use gpui::{Bounds, Pixels, TextRun, canvas, point};
use std::{cell::RefCell, rc::Rc, sync::OnceLock};

/// The desktop app's own picture, so the two draw the same art.
const PICTURE: &[u8] = include_bytes!("../../../../../src/assets/mail/mail-art.webp");
/// A cell of the art, in pixels: a monospaced glyph is about half as wide as it is tall.
const CELL_WIDTH: f32 = 5.;
const CELL_HEIGHT: f32 = 8.;
/// Steps of brightness the glyphs are drawn in.
const STEPS: usize = 6;

static DECODED: OnceLock<Option<image::RgbaImage>> = OnceLock::new();

fn decoded() -> Option<&'static image::RgbaImage> {
    DECODED
        .get_or_init(|| {
            image::load_from_memory_with_format(PICTURE, image::ImageFormat::WebP)
                .ok()
                .map(|picture| picture.into_rgba8())
        })
        .as_ref()
}

/// Reads the picture once, off the UI thread, then draws the mail again.
pub(super) fn warm(cx: &mut Context<Mailbox>) {
    if DECODED.get().is_some() {
        return;
    }
    let task = cx.background_executor().spawn(async {
        decoded();
    });
    cx.spawn(async move |this, cx| {
        task.await;
        let _ = this.update(cx, |_, cx| cx.notify());
    })
    .detach();
}

/// A row of the art: its glyphs, and each run's brightness step (0: blank).
struct Row {
    text: SharedString,
    runs: Vec<(usize, usize)>,
}

/// The art at one size, in columns and rows.
type Drawn = ((usize, usize), Rc<Vec<Row>>);

thread_local! {
    /// The art at the last size it was drawn, which it keeps until the panel resizes.
    static DRAWN: RefCell<Option<Drawn>> = const { RefCell::new(None) };
}

fn rows(cols: usize, rows: usize) -> Option<Rc<Vec<Row>>> {
    if let Some(kept) = DRAWN.with(|drawn| {
        drawn
            .borrow()
            .as_ref()
            .filter(|(size, _)| *size == (cols, rows))
            .map(|(_, rows)| rows.clone())
    }) {
        return Some(kept);
    }
    let picture = DECODED.get()?.as_ref()?;
    let (x, y, width, height) = model::cover_crop(
        picture.width(),
        picture.height(),
        cols as f32 * CELL_WIDTH,
        rows as f32 * CELL_HEIGHT,
    );
    let crop = image::imageops::crop_imm(picture, x, y, width.max(1), height.max(1)).to_image();
    let small = image::imageops::resize(
        &crop,
        cols as u32,
        rows as u32,
        image::imageops::FilterType::Triangle,
    );
    let cells = model::ascii_cells(small.as_raw(), cols, rows);
    let drawn: Vec<Row> = cells
        .chunks(cols)
        .map(|line| {
            let mut text = String::with_capacity(cols);
            let mut runs: Vec<(usize, usize)> = Vec::new();
            for &(glyph, light) in line {
                let step = if glyph == 0 {
                    0
                } else {
                    ((light * STEPS as f32).ceil() as usize).clamp(1, STEPS)
                };
                text.push(model::GLYPHS[glyph] as char);
                match runs.last_mut() {
                    Some((len, last)) if *last == step => *len += 1,
                    _ => runs.push((1, step)),
                }
            }
            Row {
                text: text.into(),
                runs,
            }
        })
        .collect();
    let drawn = Rc::new(drawn);
    DRAWN.with(|kept| *kept.borrow_mut() = Some(((cols, rows), drawn.clone())));
    Some(drawn)
}

fn paint(bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let cols = (f32::from(bounds.size.width) / CELL_WIDTH).ceil() as usize;
    let count = (f32::from(bounds.size.height) / CELL_HEIGHT).ceil() as usize;
    if cols == 0 || count == 0 {
        return;
    }
    let Some(drawn) = rows(cols, count) else {
        return;
    };
    let font = gpui::font(crate::fonts::mono());
    let accent = accent_color();
    for (index, row) in drawn.iter().enumerate() {
        let runs: Vec<TextRun> = row
            .runs
            .iter()
            .map(|&(len, step)| TextRun {
                len,
                font: font.clone(),
                color: if step == 0 {
                    accent.opacity(0.)
                } else {
                    accent.opacity(0.2 + 0.8 * step as f32 / STEPS as f32)
                },
                background_color: None,
                underline: None,
                strikethrough: None,
            })
            .collect();
        let line = window.text_system().shape_line(
            row.text.clone(),
            px(CELL_HEIGHT),
            &runs,
            Some(px(CELL_WIDTH)),
        );
        let _ = line.paint(
            point(bounds.left(), bounds.top() + px(index as f32 * CELL_HEIGHT)),
            px(CELL_HEIGHT),
            gpui::TextAlign::Left,
            None,
            window,
            cx,
        );
    }
}

pub(super) fn art(children: Option<AnyElement>) -> AnyElement {
    div()
        .relative()
        .size_full()
        .min_h_0()
        .overflow_hidden()
        .bg(accent_color().opacity(0.05))
        .child(
            canvas(
                |_, _, _| {},
                |bounds, _, window, cx| paint(bounds, window, cx),
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(gpui::relative(1. / 3.))
                .bg(gpui::linear_gradient(
                    0.,
                    gpui::linear_color_stop(background_base(), 0.),
                    gpui::linear_color_stop(background_base().opacity(0.), 1.),
                )),
        )
        .when_some(children, |d, children| {
            d.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .p(px(24.))
                    .child(children),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_desktop_apps_picture_becomes_rows_of_glyphs() {
        assert!(super::decoded().is_some(), "the picture decodes");
        let rows = super::rows(40, 20).unwrap();
        assert_eq!(rows.len(), 20);
        assert!(
            rows.iter().all(|row| row.text.len() == 40
                && row.runs.iter().map(|(len, _)| len).sum::<usize>() == 40)
        );
        assert!(
            rows.iter()
                .any(|row| row.runs.iter().any(|(_, step)| *step > 0))
        );
    }
}
