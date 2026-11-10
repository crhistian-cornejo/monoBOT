//! The live item's mark, drawn into pixels so the macOS menu bar (an image
//! the system tints) and the Windows tray (an icon) show the same picture:
//! the provider's logo (its SVG, the one the app shows), a ring that fills
//! clockwise from the top, the three dots of the work mark, or a note. The
//! tray has no room for rows, so it can carry a small number inside the ring.
use monocode_core::menu_bar::Mark;
use monocode_core::provider::Kind;

/// How strongly the empty part of a ring shows.
const TRACK: f32 = 0.32;
/// Samples per pixel side, for smooth edges.
const SAMPLES: u32 = 4;

/// Square pixels, row by row from the top, four bytes each: red, green,
/// blue and alpha, straight (not premultiplied).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Pixels {
    pub size: u32,
    pub rgba: Vec<u8>,
}

impl Pixels {
    #[cfg(test)]
    pub(crate) fn alpha(&self, x: u32, y: u32) -> u8 {
        self.rgba[((y * self.size + x) * 4 + 3) as usize]
    }
}

/// Digits 0 to 9 on a 3 × 5 grid, one row per three bits, top first.
const DIGITS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b010, 0b010, 0b010],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

/// The ring's outer and inner radii, in units of the side.
fn ring_radii(size: f32) -> (f32, f32) {
    let outer = 0.5 - 0.5 / size;
    let thickness = (0.14f32).max(1.6 / size);
    (outer, outer - thickness)
}

/// How much of the mark covers the point (x, y), both 0 to 1 across.
fn cover(mark: Mark, size: f32, x: f32, y: f32) -> f32 {
    match mark {
        Mark::Logo(_) => 0.,
        Mark::Ring(fraction) => {
            let (dx, dy) = (x - 0.5, y - 0.5);
            let distance = (dx * dx + dy * dy).sqrt();
            let (outer, inner) = ring_radii(size);
            if distance > outer || distance < inner {
                return 0.;
            }
            // Clockwise from the top, 0 to 1.
            let turn = (dx.atan2(-dy) / std::f32::consts::TAU).rem_euclid(1.);
            if turn <= fraction.clamp(0., 1.) {
                1.
            } else {
                TRACK
            }
        }
        Mark::Dots => {
            let radius = 0.11;
            let on = [0.2, 0.5, 0.8]
                .iter()
                .any(|cx| (x - cx).powi(2) + (y - 0.5).powi(2) <= radius * radius);
            if on { 1. } else { 0. }
        }
        Mark::Note => {
            let head =
                (x - 0.36).powi(2) / 0.19f32.powi(2) + (y - 0.72).powi(2) / 0.14f32.powi(2) <= 1.;
            let stem = (0.46..=0.55).contains(&x) && (0.14..=0.72).contains(&y);
            let rise = (x - 0.55) * 0.45;
            let flag = (0.55..=0.8).contains(&x) && (0.14 + rise..=0.3 + rise).contains(&y);
            if head || stem || flag { 1. } else { 0. }
        }
    }
}

/// Whether the number is drawn inside the ring at this size, and its scale:
/// pixels per grid cell.
fn figure_scale(size: u32, figure: u32) -> Option<(u32, Vec<usize>)> {
    let digits: Vec<usize> = figure
        .to_string()
        .bytes()
        .map(|b| (b - b'0') as usize)
        .collect();
    let (_, inner) = ring_radii(size as f32);
    // The digits must fit in the square inside the hole.
    let room = (inner * 2. * size as f32) * 0.8;
    let width = digits.len() as u32 * 4 - 1;
    let scale = (room / width as f32).min(room / 5.).floor() as u32;
    (scale >= 1).then_some((scale, digits))
}

/// Draws a mark `size` pixels square in `color`, with `figure` inside the
/// ring when there is room for it.
pub(crate) fn draw(mark: Mark, size: u32, figure: Option<u32>, color: [u8; 3]) -> Pixels {
    if let Mark::Logo(kind) = mark {
        return logo(kind, size, color).unwrap_or_else(|| draw(Mark::Dots, size, None, color));
    }
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let side = size as f32;
    let figure = match mark {
        Mark::Ring(_) => figure.and_then(|figure| figure_scale(size, figure)),
        _ => None,
    };
    let glyph = |px: u32, py: u32| -> bool {
        let Some((scale, digits)) = &figure else {
            return false;
        };
        let width = (digits.len() as u32 * 4 - 1) * scale;
        let height = 5 * scale;
        let left = (size - width) / 2;
        let top = (size - height) / 2;
        if px < left || py < top || px >= left + width || py >= top + height {
            return false;
        }
        let (gx, gy) = ((px - left) / scale, (py - top) / scale);
        let (digit, column) = ((gx / 4) as usize, gx % 4);
        column < 3 && DIGITS[digits[digit]][gy as usize] & (0b100 >> column) != 0
    };
    for py in 0..size {
        for px in 0..size {
            let alpha = if glyph(px, py) {
                1.
            } else {
                let mut sum = 0.;
                for sy in 0..SAMPLES {
                    for sx in 0..SAMPLES {
                        let x = (px as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / side;
                        let y = (py as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / side;
                        sum += cover(mark, side, x, y);
                    }
                }
                sum / (SAMPLES * SAMPLES) as f32
            };
            let at = ((py * size + px) * 4) as usize;
            rgba[at..at + 3].copy_from_slice(&color);
            rgba[at + 3] = (alpha.clamp(0., 1.) * 255.).round() as u8;
        }
    }
    Pixels { size, rgba }
}

/// The provider's logo as a silhouette in `color`: the menu bar shows
/// marks in one color, as the system's own do.
fn logo(kind: Kind, size: u32, color: [u8; 3]) -> Option<Pixels> {
    use gpui::AssetSource;
    use resvg::{tiny_skia, usvg};
    let path = match kind {
        Kind::Codex => "codex-glyph.svg",
        other => crate::theme::provider_logo(other.id()),
    };
    let data = crate::assets::Assets.load(path).ok()??;
    let tree = usvg::Tree::from_data(&data, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
    let (width, height) = (tree.size().width(), tree.size().height());
    let scale = size as f32 / width.max(height);
    let offset = |side: f32| (size as f32 - side * scale) / 2.;
    let transform =
        tiny_skia::Transform::from_row(scale, 0., 0., scale, offset(width), offset(height));
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let rgba = pixmap
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|px| [color[0], color[1], color[2], px[3]])
        .collect();
    Some(Pixels { size, rgba })
}

#[cfg(test)]
mod tests {
    use super::*;

    const INK: [u8; 3] = [0, 0, 0];

    #[test]
    fn a_ring_fills_clockwise_from_the_top() {
        let quarter = draw(Mark::Ring(0.25), 32, None, INK);
        // Top right of the ring is filled, bottom left only shows the track.
        let filled = quarter.alpha(26, 9);
        let track = quarter.alpha(5, 22);
        assert!(filled > 200, "{filled}");
        assert!((60..120).contains(&track), "{track}");
        // The hole and the corners stay clear.
        assert_eq!(quarter.alpha(16, 16), 0);
        assert_eq!(quarter.alpha(0, 0), 0);
    }

    #[test]
    fn the_tray_draws_the_number_inside_the_ring_when_it_fits() {
        let plain = draw(Mark::Ring(0.5), 16, None, INK);
        let figure = draw(Mark::Ring(0.5), 16, Some(72), INK);
        assert_eq!(plain.alpha(8, 8), 0);
        assert_ne!(plain, figure);
        // "7" starts with a full top row.
        assert_eq!(figure.alpha(5, 5), 255);
        // Its second row has only the right cell.
        assert_eq!(figure.alpha(5, 6), 0);
        assert_eq!(figure.alpha(6, 6), 255);
        // Three digits do not fit in a 16-pixel hole: the ring stays plain.
        assert_eq!(
            draw(Mark::Ring(1.), 16, Some(100), INK),
            draw(Mark::Ring(1.), 16, None, INK)
        );
        assert!(figure_scale(32, 100).is_some());
    }

    #[test]
    fn a_logo_is_its_svg_as_a_silhouette() {
        for kind in [
            Kind::Claude,
            Kind::Codex,
            Kind::Devin,
            Kind::OpenCode,
            Kind::Kimi,
        ] {
            let logo = draw(Mark::Logo(kind), 32, None, [9, 9, 9]);
            let inked = logo
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|px| px[3] > 128)
                .count();
            assert!(inked > 60, "{kind:?} draws its logo ({inked} pixels)");
            assert!(
                logo.rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|px| px[..3] == [9, 9, 9])
            );
        }
    }

    #[test]
    fn dots_and_note_are_drawn_in_the_given_color() {
        let dots = draw(Mark::Dots, 18, None, [255, 255, 255]);
        assert_eq!(dots.alpha(9, 9), 255);
        assert_eq!(&dots.rgba[0..3], &[255, 255, 255]);
        let note = draw(Mark::Note, 18, Some(5), INK);
        assert!(note.alpha(6, 13) > 200, "the note's head");
        assert_eq!(note.alpha(2, 2), 0);
    }
}
