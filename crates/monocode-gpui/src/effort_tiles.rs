//! The shimmer behind a strong reasoning effort in a model menu: Max, Ultra,
//! Ultracode and Ultrathink. While the row is hovered or highlighted, a grid
//! of tiles fills with the effort's colour from the row's centre outwards,
//! the row's edge pulses three times, and the whole layer fades out after
//! five seconds. Under reduced motion the layer is drawn once, at rest.
//!
//! It only paints: the grid is a single `canvas`, so a row is not 160
//! elements a frame.
use crate::theme::*;
use gpui::{
    Animation, AnimationExt, AnyElement, Bounds, Hsla, Window, canvas, div, point, prelude::*, px,
    size,
};
use std::time::Duration;

/// Tiles across and down the row.
const COLUMNS: usize = 32;
const ROWS: usize = 5;
/// How long the layer stays before it clears.
const SPAN_MS: f32 = 5000.;
/// One tile's fill, played three times, each tile starting later the
/// farther it is from the centre.
const FILL_MS: f32 = 1400.;
const FILL_PLAYS: f32 = 3.;
const STAGGER_MS: f32 = 420.;
/// The layer fades in as the row is entered.
const FADE_IN_MS: f32 = 160.;
/// Share of the span the layer holds before it fades out.
const HOLD: f32 = 0.92;
/// Width over which the layer dissolves into the row's left and right edges.
const FEATHER_X: f32 = 28.;
/// A filled tile's strongest alpha: the label stays readable on it.
const PEAK: f32 = 0.6;
/// Where the layer rests under reduced motion.
const REST_LAYER: f32 = 0.72;
const REST_FILL: f32 = 0.35;
const REST_EDGE: f32 = 0.42;

/// An effort that gets the shimmer, and the colour it wears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tone {
    Max,
    Ultra,
    Ultracode,
    Ultrathink,
}

impl Tone {
    /// The tone of a catalog effort id, when it has one. Every provider's
    /// effort menu asks, so Codex's Ultra and Claude's Max look the same.
    pub(crate) fn of(effort: &str) -> Option<Self> {
        match effort {
            "max" => Some(Self::Max),
            "ultra" => Some(Self::Ultra),
            "ultracode" => Some(Self::Ultracode),
            "ultrathink" => Some(Self::Ultrathink),
            _ => None,
        }
    }
    pub(crate) fn color(self) -> Hsla {
        gpui::rgb(match self {
            Self::Max => TONE_MAX,
            Self::Ultra => TONE_ULTRA,
            Self::Ultracode => TONE_ULTRACODE,
            Self::Ultrathink => TONE_ULTRATHINK,
        })
        .into()
    }
}

/// How far a tile is from the grid's centre: 0 in the middle, about 1.4 in
/// a corner.
fn distance(column: usize, row: usize) -> f32 {
    let centre_x = (COLUMNS as f32 - 1.) / 2.;
    let centre_y = (ROWS as f32 - 1.) / 2.;
    (((column as f32 - centre_x) / centre_x).powi(2) + ((row as f32 - centre_y) / centre_y).powi(2))
        .sqrt()
}

/// Whether a tile takes part, on a fixed scatter so every row looks alike.
fn takes_part(index: usize) -> bool {
    (index * 73 + index * index * 19 + 23) % 101 < 65
}

/// A value along keyframes of (position in 0..=1, value), straight between
/// them.
fn along(keys: &[(f32, f32)], at: f32) -> f32 {
    let at = at.clamp(0., 1.);
    for pair in keys.windows(2) {
        let ((from, a), (to, b)) = (pair[0], pair[1]);
        if at <= to {
            return a + (b - a) * ((at - from) / (to - from).max(f32::EPSILON)).clamp(0., 1.);
        }
    }
    keys.last().map_or(0., |&(_, value)| value)
}

/// How filled a tile is, 0 to 1, `ms` into the span: nothing before its
/// turn, then it lights, dims and clears, three times.
fn fill(distance: f32, ms: f32) -> f32 {
    let local = ms - distance * STAGGER_MS;
    if !(0. ..FILL_MS * FILL_PLAYS).contains(&local) {
        return 0.;
    }
    along(
        &[(0., 0.), (0.26, 1.), (0.52, 0.18), (1., 0.)],
        (local % FILL_MS) / FILL_MS,
    )
}

/// The row's edge glow, 0 to 1: three pulses, then it is gone.
fn edge(progress: f32) -> f32 {
    along(
        &[
            (0., 0.),
            (0.12, 0.),
            (0.19, 1.),
            (0.26, 0.),
            (0.40, 0.),
            (0.47, 1.),
            (0.54, 0.),
            (0.68, 0.),
            (0.75, 1.),
            (0.82, 0.),
            (1., 0.),
        ],
        progress,
    )
}

/// How visible the whole layer is: it fades in, holds, then clears.
fn layer(ms: f32) -> f32 {
    let into = (ms / FADE_IN_MS).clamp(0., 1.);
    into * along(&[(0., 1.), (HOLD, 1.), (1., 0.)], ms / SPAN_MS)
}

/// How much of a tile at (`x`, `y`), both 0 to 1 across the row, shows: the
/// layer dissolves into the row's four sides instead of stopping at them.
fn feather(x: f32, y: f32, width: f32) -> f32 {
    let across = (width / FEATHER_X).max(2.);
    let sideways = (x * across).min((1. - x) * across).clamp(0., 1.);
    let down = if y < 0.35 {
        y / 0.35
    } else if y > 0.65 {
        (1. - y) / 0.35
    } else {
        1.
    };
    sideways * down.clamp(0., 1.)
}

/// Where in its play the layer is.
#[derive(Clone, Copy)]
enum Moment {
    /// 0 to 1 through the span.
    Playing(f32),
    /// Reduced motion: the layer at rest.
    Resting,
}

fn paint(bounds: Bounds<gpui::Pixels>, color: Hsla, moment: Moment, window: &mut Window) {
    let width: f32 = bounds.size.width.into();
    let height: f32 = bounds.size.height.into();
    if width < 1. || height < 1. {
        return;
    }
    let (layer, ms, glow) = match moment {
        Moment::Playing(progress) => {
            let ms = progress * SPAN_MS;
            (layer(ms), Some(ms), edge(progress) * 0.55 * 0.85)
        }
        Moment::Resting => (REST_LAYER, None, REST_EDGE * 0.55),
    };
    if layer <= 0.003 {
        return;
    }
    let (cell_w, cell_h) = (width / COLUMNS as f32, height / ROWS as f32);
    for row in 0..ROWS {
        for column in 0..COLUMNS {
            let index = row * COLUMNS + column;
            let visible = layer
                * feather(
                    (column as f32 + 0.5) / COLUMNS as f32,
                    (row as f32 + 0.5) / ROWS as f32,
                    width,
                );
            if visible <= 0.003 {
                continue;
            }
            let origin = bounds.origin + point(px(column as f32 * cell_w), px(row as f32 * cell_h));
            let filled = if !takes_part(index) {
                0.
            } else if let Some(ms) = ms {
                fill(distance(column, row), ms) * PEAK
            } else {
                REST_FILL
            };
            if filled > 0.003 {
                window.paint_quad(gpui::fill(
                    Bounds::new(origin, size(px(cell_w), px(cell_h))),
                    color.opacity(filled * visible),
                ));
            }
            // The tile's own lower and right edge, so the grid reads even
            // where nothing is lit.
            let line = color.opacity(0.32 * visible);
            window.paint_quad(gpui::fill(
                Bounds::new(
                    origin + point(px(cell_w - 1.), px(0.)),
                    size(px(1.), px(cell_h)),
                ),
                line,
            ));
            window.paint_quad(gpui::fill(
                Bounds::new(
                    origin + point(px(0.), px(cell_h - 1.)),
                    size(px(cell_w), px(1.)),
                ),
                line,
            ));
        }
    }
    if glow > 0.003 {
        window.paint_quad(gpui::quad(
            bounds,
            px(8.),
            gpui::transparent_black(),
            px(2.),
            color.opacity(glow),
            gpui::BorderStyle::Solid,
        ));
    }
}

fn grid(color: Hsla, moment: Moment) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| paint(bounds, color, moment, window),
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// The shimmer layer for one row: put it first in the row, which must be
/// `relative`. `key` ties the play to the row, so it starts again each time
/// the row is entered after having been left.
pub(crate) fn tiles(key: impl Into<gpui::ElementId>, color: Hsla, reduced: bool) -> AnyElement {
    let layer = div().absolute().top_0().left_0().size_full();
    if reduced {
        return layer.child(grid(color, Moment::Resting)).into_any_element();
    }
    layer
        .with_animation(
            key,
            Animation::new(Duration::from_millis(SPAN_MS as u64)),
            move |layer, progress| layer.child(grid(color, Moment::Playing(progress))),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_strong_efforts_have_a_tone() {
        for effort in ["max", "ultra", "ultracode", "ultrathink"] {
            assert!(Tone::of(effort).is_some(), "{effort}");
        }
        for effort in ["", "low", "medium", "high", "xhigh", "Max"] {
            assert_eq!(Tone::of(effort), None, "{effort}");
        }
    }

    #[test]
    fn each_tone_has_its_own_colour() {
        let tones = [Tone::Max, Tone::Ultra, Tone::Ultracode, Tone::Ultrathink];
        for (at, a) in tones.iter().enumerate() {
            for b in &tones[at + 1..] {
                assert_ne!(a.color(), b.color());
            }
        }
    }

    #[test]
    fn the_centre_fills_before_the_corners() {
        assert!(distance(15, 2) < distance(16, 2) + 0.1);
        assert!(distance(15, 2) < 0.1);
        assert!(distance(0, 0) > 1.3);
        // The centre is lit while a corner still waits for its turn.
        assert!(fill(distance(16, 2), 300.) > 0.);
        assert_eq!(fill(distance(0, 0), 300.), 0.);
    }

    #[test]
    fn a_tile_lights_dims_and_clears_three_times() {
        assert_eq!(fill(0., 0.), 0.);
        let peak = FILL_MS * 0.26;
        assert!((fill(0., peak) - 1.).abs() < 1e-3);
        assert!(fill(0., FILL_MS * 0.52) < 0.2);
        // The second and third plays repeat the first.
        assert!((fill(0., FILL_MS + peak) - 1.).abs() < 1e-3);
        assert!((fill(0., FILL_MS * 2. + peak) - 1.).abs() < 1e-3);
        // After the third it stays clear.
        assert_eq!(fill(0., FILL_MS * 3. + 10.), 0.);
    }

    #[test]
    fn the_edge_pulses_three_times_then_is_gone() {
        for peak in [0.19, 0.47, 0.75] {
            assert!((edge(peak) - 1.).abs() < 1e-3, "{peak}");
        }
        for rest in [0., 0.05, 0.33, 0.6, 0.9, 1.] {
            assert!(edge(rest) < 0.5, "{rest}");
        }
        assert_eq!(edge(1.), 0.);
    }

    #[test]
    fn the_layer_fades_in_holds_and_clears() {
        assert_eq!(layer(0.), 0.);
        assert!(layer(FADE_IN_MS / 2.) < 1.);
        assert_eq!(layer(SPAN_MS * 0.5), 1.);
        assert!(layer(SPAN_MS * 0.96) < 1.);
        assert_eq!(layer(SPAN_MS), 0.);
    }

    #[test]
    fn the_layer_dissolves_into_the_rows_sides() {
        let width = 200.;
        assert_eq!(feather(0., 0.5, width), 0.);
        assert_eq!(feather(1., 0.5, width), 0.);
        assert_eq!(feather(0.5, 0.5, width), 1.);
        assert_eq!(feather(0.5, 0., width), 0.);
        assert_eq!(feather(0.5, 1., width), 0.);
        assert!(feather(0.5, 0.2, width) < feather(0.5, 0.5, width));
        assert!(feather(0.02, 0.5, width) < 1.);
    }

    #[test]
    fn about_two_thirds_of_the_tiles_take_part() {
        let count = (0..COLUMNS * ROWS).filter(|i| takes_part(*i)).count();
        let share = count as f32 / (COLUMNS * ROWS) as f32;
        assert!((0.55..0.75).contains(&share), "{share}");
    }
}
