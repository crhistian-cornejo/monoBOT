//! The short decorative scenes Tauri plays over a session when the user
//! picks an Astra model (a small sun with falling stars) or Opus 5.5 (a
//! handwritten staff with notes). They live in the pane, never take input,
//! and are skipped under reduced motion.
//!
//! The scenes are pure functions of time: [`astra`] and [`opus`] return the
//! shapes for one instant, and [`State::overlay`] paints them on a canvas
//! while the shared pulse clock keeps the pane repainting.
use crate::{popover::AnchorSlot, pulse, reveal::bezier, theme};
use gpui::{
    AnyElement, App, Bounds, ContentMask, Hsla, PathBuilder, Pixels, Window, canvas, fill, point,
    prelude::*, px, size,
};
use std::time::{Duration, Instant};

const ASTRA_MS: u64 = 7600;
const OPUS_MS: u64 = 7000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Scene {
    Astra,
    Opus,
}

impl Scene {
    fn duration(self) -> Duration {
        Duration::from_millis(match self {
            Scene::Astra => ASTRA_MS,
            Scene::Opus => OPUS_MS,
        })
    }
}

/// `astra` as a word of its own: `gpt-astra`, `GPT-5.5 Astra`, never `astrally`.
fn mentions_astra(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    lower.match_indices("astra").any(|(at, word)| {
        let before = at == 0 || !bytes[at - 1].is_ascii_alphanumeric();
        let after = bytes
            .get(at + word.len())
            .is_none_or(|b| !b.is_ascii_alphanumeric());
        before && after
    })
}

/// Opus 5.5 across catalog ids (`opus-5-5`), names (`Opus 5.5`) and dated
/// native ids, but not `opus-5.50` or `opus-5.5.1`.
fn mentions_opus_55(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    lower.match_indices("opus").any(|(at, word)| {
        if at > 0 && bytes[at - 1].is_ascii_alphanumeric() {
            return false;
        }
        let mut i = at + word.len();
        if matches!(bytes.get(i), Some(b' ' | b'\t' | b'-')) {
            i += 1;
        }
        let rest = &bytes[i..];
        rest.len() >= 3
            && rest[0] == b'5'
            && matches!(rest[1], b'.' | b'-')
            && rest[2] == b'5'
            && !matches!(rest.get(3), Some(b'0'..=b'9' | b'.'))
    })
}

/// The scene a picked model earns, from its id and its visible name.
pub(crate) fn scene_for(values: &[&str]) -> Option<Scene> {
    if values.iter().any(|v| mentions_astra(v)) {
        Some(Scene::Astra)
    } else if values.iter().any(|v| mentions_opus_55(v)) {
        Some(Scene::Opus)
    } else {
        None
    }
}

/// Pane-relative band for the Opus scene: the free space under a centered
/// composer, or a band just above a docked one that leaves no room below.
pub(crate) fn opus_stage(pane_height: f32, composer: Option<(f32, f32)>) -> (f32, f32) {
    const MIN_BELOW: f32 = 140.;
    const MAX_ABOVE: f32 = 240.;
    let Some((top, bottom)) = composer else {
        let top = (pane_height * 0.55).round();
        return (top, pane_height - top);
    };
    let below = pane_height - bottom;
    if below >= MIN_BELOW {
        return (bottom, below);
    }
    let height = top.clamp(0., MAX_ABOVE);
    (top - height, height)
}

/// Linear keyframes over `0..1`; holds the first and last value outside them.
fn key(t: f32, frames: &[(f32, f32)]) -> f32 {
    let (first, last) = (frames[0], frames[frames.len() - 1]);
    if t <= first.0 {
        return first.1;
    }
    if t >= last.0 {
        return last.1;
    }
    frames
        .windows(2)
        .find(|pair| t <= pair[1].0)
        .map(|pair| {
            let span = (pair[1].0 - pair[0].0).max(f32::EPSILON);
            pair[0].1 + (pair[1].1 - pair[0].1) * ((t - pair[0].0) / span)
        })
        .unwrap_or(last.1)
}

/// How far an animation that starts after `delay` seconds and lasts `length`
/// seconds has got at `seconds` into the scene; none before or after it.
fn local(seconds: f32, delay: f32, length: f32) -> Option<f32> {
    let u = (seconds - delay) / length;
    (0. ..=1.).contains(&u).then_some(u)
}

fn tint(hex: u32, alpha: f32) -> Hsla {
    let mut color: Hsla = gpui::rgb(hex).into();
    color.a = alpha.clamp(0., 1.);
    color
}

#[derive(Clone, Debug, PartialEq)]
enum Shape {
    Dot {
        x: f32,
        y: f32,
        r: f32,
        color: Hsla,
    },
    Ring {
        x: f32,
        y: f32,
        r: f32,
        color: Hsla,
    },
    Line {
        from: (f32, f32),
        to: (f32, f32),
        width: f32,
        color: Hsla,
    },
    Poly {
        points: Vec<(f32, f32)>,
        closed: bool,
        fill: bool,
        width: f32,
        color: Hsla,
    },
}

fn glow(shapes: &mut Vec<Shape>, x: f32, y: f32, radius: f32, hex: u32, alpha: f32) {
    const LAYERS: usize = 10;
    for layer in 0..LAYERS {
        shapes.push(Shape::Dot {
            x,
            y,
            r: radius * (1. - layer as f32 / LAYERS as f32),
            color: tint(hex, alpha / LAYERS as f32),
        });
    }
}

fn ellipse(
    center: (f32, f32),
    radius: f32,
    squash: f32,
    turn: f32,
    steps: usize,
) -> Vec<(f32, f32)> {
    (0..steps)
        .map(|i| {
            let a = i as f32 / steps as f32 * std::f32::consts::TAU;
            orbit_point(center, radius, squash, turn, a)
        })
        .collect()
}

/// A point `angle` clockwise from the top of a circle that is squashed
/// vertically, then turned, the way the orbits are transformed.
fn orbit_point(center: (f32, f32), radius: f32, squash: f32, turn: f32, angle: f32) -> (f32, f32) {
    let (x, y) = (radius * angle.sin(), -radius * angle.cos() * squash);
    let (s, c) = turn.sin_cos();
    (center.0 + x * c - y * s, center.1 + x * s + y * c)
}

/// The Astra scene at `t` (`0..1`) in a pane of `w` by `h`.
fn astra(t: f32, w: f32, h: f32, light: bool) -> Vec<Shape> {
    let seconds = t * ASTRA_MS as f32 / 1000.;
    let fade = key(t, &[(0., 0.), (0.08, 1.), (0.72, 1.), (1., 0.)]);
    let star = 0xd8c9a2;
    let bright = 0xf5f0e4;
    let ink = if light { 0x806c3b } else { bright };
    let mut shapes = Vec::new();

    let nebula = key(t, &[(0., 0.), (0.25, 1.), (0.55, 1.), (1., 0.)]) * fade;
    glow(
        &mut shapes,
        0.82 * w,
        0.12 * h,
        0.5 * w,
        star,
        0.15 * nebula,
    );
    glow(&mut shapes, w, 0., 0.65 * w, star, 0.07 * nebula);
    glow(
        &mut shapes,
        0.15 * w,
        0.75 * h,
        0.6 * w,
        star,
        0.08 * nebula,
    );

    let size = (0.1 * w).clamp(46., 86.);
    let center = (0.82 * w, (0.15 * h).clamp(58., 120.));
    let arrival = (seconds / 6.).clamp(0., 1.);
    let sys = key(arrival, &[(0., 0.), (0.16, 0.85), (0.62, 0.6), (1., 0.)]) * fade;
    let scale = key(arrival, &[(0., 0.7), (0.16, 1.), (1., 1.06)]);
    let unit = size * scale;
    if sys > 0.001 {
        // Corona: rays that turn slowly and fade in from the core outward.
        let turn = 45f32.to_radians() * t;
        for ray in 0..24 {
            let a = ray as f32 * 15f32.to_radians() + turn;
            let (s, c) = a.sin_cos();
            for (from, to, alpha) in [
                (0.24, 0.34, 0.08),
                (0.34, 0.44, 0.18),
                (0.44, 0.7, 0.2),
                (0.7, 0.98, 0.1),
                (0.98, 1.29, 0.04),
            ] {
                shapes.push(Shape::Line {
                    from: (center.0 + s * from * unit, center.1 - c * from * unit),
                    to: (center.0 + s * to * unit, center.1 - c * to * unit),
                    width: 1.2,
                    color: tint(star, alpha * sys),
                });
            }
        }
        // Shock waves leaving the core.
        for delay in [0.2f32, 0.85] {
            if let Some(u) = local(seconds, delay, 3.2) {
                let eased = bezier(u, 0.16, 1., 0.3, 1.);
                let radius = unit / 2. * (0.5 + 4. * eased);
                let alpha = key(u, &[(0., 0.), (0.15, 0.25), (1., 0.)]);
                shapes.push(Shape::Ring {
                    x: center.0,
                    y: center.1,
                    r: radius,
                    color: tint(star, alpha * sys),
                });
            }
        }
        // Two tilted orbits, each carrying a small planet in opposite directions.
        for (radius, squash, tilt, alpha, reverse) in [
            (0.95, 0.38, -28f32, 0.21, false),
            (1.2, 0.32, 48f32, 0.19, true),
        ] {
            let tilt = tilt.to_radians();
            shapes.push(Shape::Poly {
                points: ellipse(center, radius * unit, squash, tilt, 72),
                closed: true,
                fill: false,
                width: 1.,
                color: tint(star, alpha * sys),
            });
            let degrees = if reverse {
                235. - 220. * t
            } else {
                15. + 220. * t
            };
            let (x, y) = orbit_point(center, radius * unit, squash, tilt, degrees.to_radians());
            shapes.push(Shape::Dot {
                x,
                y,
                r: 5.,
                color: tint(star, 0.12 * sys),
            });
            shapes.push(Shape::Dot {
                x,
                y,
                r: 2.,
                color: tint(star, sys),
            });
        }
        // The core: a lit sphere inside a soft halo.
        let core = 0.22 * unit;
        for (factor, alpha) in [(3.2, 0.03), (2.4, 0.05), (1.7, 0.08), (1.25, 0.12)] {
            shapes.push(Shape::Dot {
                x: center.0,
                y: center.1,
                r: core * factor,
                color: tint(star, alpha * sys),
            });
        }
        if light {
            shapes.push(Shape::Ring {
                x: center.0,
                y: center.1,
                r: core + 1.,
                color: tint(0xa89560, 0.2 * sys),
            });
        }
        for (factor, offset, hex) in [(1., 0., 0xa89560), (0.65, -0.1, star), (0.3, -0.2, bright)] {
            shapes.push(Shape::Dot {
                x: center.0 + core * offset,
                y: center.1 + core * offset,
                r: core * factor,
                color: tint(hex, sys),
            });
        }
    }

    // Sparkles drifting down while they bloom.
    for i in 0..36usize {
        let delay = 0.2 + ((i * 7) % 24) as f32 * 0.12;
        let length = 2.2 + (i % 4) as f32 * 0.45;
        let Some(u) = local(seconds, delay, length) else {
            continue;
        };
        let cross = i % 6 == 0;
        let base = if cross { 9. } else { (1 + i % 3) as f32 };
        let x = w * (3 + (i * 37) % 94) as f32 / 100.;
        let y = h * (4 + (i * 23) % 86) as f32 / 100. + key(u, &[(0., 0.), (0.3, 4.), (1., 16.)]);
        let alpha = key(u, &[(0., 0.), (0.3, 0.7), (0.65, 0.3), (1., 0.)]) * fade;
        let spark = base * key(u, &[(0., 0.6), (0.3, 1.), (1., 0.7)]);
        let hex = if light { 0x806c3b } else { star };
        shapes.push(Shape::Dot {
            x,
            y,
            r: spark / 2. + 3.,
            color: tint(hex, 0.15 * alpha),
        });
        if cross {
            let r = spark / 2.;
            let points = [
                (0., -1.),
                (0.22, -0.24),
                (1., 0.),
                (0.22, 0.22),
                (0., 1.),
                (-0.22, 0.22),
                (-1., 0.),
                (-0.22, -0.24),
            ]
            .map(|(dx, dy)| (x + dx * r, y + dy * r));
            shapes.push(Shape::Poly {
                points: points.to_vec(),
                closed: true,
                fill: true,
                width: 0.,
                color: tint(if light { 0x806c3b } else { bright }, alpha),
            });
        } else {
            shapes.push(Shape::Dot {
                x,
                y,
                r: spark / 2.,
                color: tint(hex, alpha),
            });
        }
    }

    // Shooting stars fall toward the lower left.
    let direction = 55f32.to_radians();
    for i in 0..38usize {
        let delay = 0.35 + i as f32 * 0.12;
        let length = 1.4 + (i % 5) as f32 * 0.22;
        let Some(u) = local(seconds, delay, length) else {
            continue;
        };
        let brightness = if i % 3 == 0 { 0.95 } else { 0.5 };
        let alpha = key(
            u,
            &[(0., 0.), (0.12, brightness), (0.7, brightness), (1., 0.)],
        ) * fade;
        let long = i % 9 == 0;
        let tail = if long {
            190.
        } else {
            45. + ((i * 23) % 95) as f32
        };
        let head = (
            w * (8 + (i * 37) % 125) as f32 / 100. - 0.7 * h * u,
            h * (-12. + ((i * 19) % 48) as f32) / 100. + h * u,
        );
        let pieces = 6;
        for piece in 0..pieces {
            let a = piece as f32 / pieces as f32;
            let b = (piece + 1) as f32 / pieces as f32;
            let at = |f: f32| {
                (
                    head.0 + direction.cos() * tail * f,
                    head.1 - direction.sin() * tail * f,
                )
            };
            shapes.push(Shape::Line {
                from: at(a),
                to: at(b),
                width: if long { 2. } else { 1. },
                color: tint(ink, alpha * (1. - (a + b) / 2.)),
            });
        }
        shapes.push(Shape::Dot {
            x: head.0,
            y: head.1,
            r: 3.,
            color: tint(star, 0.3 * alpha),
        });
        shapes.push(Shape::Dot {
            x: head.0,
            y: head.1,
            r: 1.5,
            color: tint(ink, alpha),
        });
    }
    shapes
}

/// A short phrase for the staff: it climbs, hesitates, and resolves on the
/// middle line.
const MELODY: [(f32, i32); 10] = [
    (0.06, -3),
    (0.13, -1),
    (0.2, 1),
    (0.28, 0),
    (0.36, 2),
    (0.44, 4),
    (0.52, 3),
    (0.6, 1),
    (0.68, 2),
    (0.77, 0),
];

/// Where a staff line passes at `t` across the stage, `step` half-lines from
/// the middle: a loose handwritten wave that gathers into a single thread.
fn staff_y(t: f32, step: i32, height: f32) -> f32 {
    let end = height * 0.42;
    let rise = height * 0.86 - end;
    end + rise * (1. - t).powf(1.7)
        + (t * std::f32::consts::PI * 2.4 + 0.3).sin() * 20f32.min(height * 0.08) * (1. - t)
        - step as f32 / 2. * 11. * (1. - 0.84 * t)
}

/// The Opus scene at `t` in a pane `w` wide whose staff plays in the band
/// `stage` = (top, height).
fn opus(t: f32, w: f32, h: f32, stage: (f32, f32), light: bool) -> Vec<Shape> {
    let seconds = t * OPUS_MS as f32 / 1000.;
    let fade = key(t, &[(0., 0.), (0.06, 1.), (0.78, 1.), (1., 0.)]);
    let (clay, note) = if light {
        (0xbd5d3a, 0x8a4128)
    } else {
        (0xd97757, 0xf4ede4)
    };
    let mut shapes = Vec::new();

    let breathe = key(t, &[(0., 0.), (0.22, 1.), (0.58, 1.), (1., 0.)]) * fade;
    glow(
        &mut shapes,
        0.82 * w,
        0.14 * h,
        0.5 * w,
        clay,
        0.14 * breathe,
    );
    glow(
        &mut shapes,
        0.4 * w,
        0.18 * h,
        0.6 * w,
        clay,
        0.07 * breathe,
    );
    glow(
        &mut shapes,
        0.1 * w,
        0.85 * h,
        0.55 * w,
        note,
        0.05 * breathe,
    );

    let (top, height) = stage;
    if height <= 0. {
        return shapes;
    }

    for i in 0..26usize {
        let delay = 0.5 + ((i * 11) % 30) as f32 * 0.12;
        let length = 2.6 + (i % 5) as f32 * 0.4;
        let Some(u) = local(seconds, delay, length) else {
            continue;
        };
        let drift = if i % 2 == 1 { 1. } else { -1. } * (6 + (i % 7) * 3) as f32;
        let x = w * (4 + (i * 41) % 92) as f32 / 100.
            + key(u, &[(0., 0.), (0.25, drift * 0.3), (1., drift)]);
        let y = top
            + height * (20 + (i * 29) % 72) as f32 / 100.
            + key(u, &[(0., 0.), (0.25, -10.), (1., -46.)]);
        let alpha = key(u, &[(0., 0.), (0.25, 0.55), (1., 0.)]) * fade;
        let size = (2 + i % 4) as f32 * key(u, &[(0., 0.6), (0.25, 1.), (1., 0.8)]);
        if i % 5 == 0 {
            shapes.push(Shape::Ring {
                x,
                y,
                r: size * 1.1,
                color: tint(clay, 0.6 * alpha),
            });
        } else {
            shapes.push(Shape::Dot {
                x,
                y,
                r: size / 2. + 3.,
                color: tint(clay, 0.12 * alpha),
            });
            shapes.push(Shape::Dot {
                x,
                y,
                r: size / 2.,
                color: tint(clay, alpha),
            });
        }
    }

    // Five lines write themselves from left to right with a soft pen edge.
    let settle = key(t, &[(0., 1.), (0.6, 1.), (1., 0.)]) * fade;
    for (index, step) in [-4, -2, 0, 2, 4].into_iter().enumerate() {
        let Some(u) = local(seconds, 0.15 + index as f32 * 0.09, 2.3).or_else(|| {
            // Before its turn nothing shows; afterwards the line is fully written.
            (seconds > 0.15 + index as f32 * 0.09 + 2.3).then_some(1.)
        }) else {
            continue;
        };
        let pen = bezier(u, 0.55, 0.1, 0.3, 1.);
        let origin = -1.5 * w * (1. - pen);
        let (solid_until, clear_from) = (origin + 1.05 * w, origin + 1.45 * w);
        let base = if index == 2 { 0.8 } else { 0.55 } * settle;
        let at = |i: usize| {
            let along = i as f32 / 120.;
            (along * w, top + staff_y(along, step, height))
        };
        let mut solid = Vec::new();
        for i in 0..=120 {
            let p = at(i);
            if p.0 <= solid_until {
                solid.push(p);
                continue;
            }
            if solid.len() > 1 {
                shapes.push(Shape::Poly {
                    points: std::mem::take(&mut solid),
                    closed: false,
                    fill: false,
                    width: 1.,
                    color: tint(clay, base),
                });
            }
            if p.0 < clear_from {
                let prior = at(i - 1);
                let mid = (p.0 + prior.0) / 2.;
                let edge = 1. - (mid - solid_until) / (clear_from - solid_until);
                shapes.push(Shape::Line {
                    from: prior,
                    to: p,
                    width: 1.,
                    color: tint(clay, base * edge),
                });
            }
        }
        if solid.len() > 1 {
            shapes.push(Shape::Poly {
                points: solid,
                closed: false,
                fill: false,
                width: 1.,
                color: tint(clay, base),
            });
        }
    }

    for (index, (along, step)) in MELODY.into_iter().enumerate() {
        let delay = 1.05 + index as f32 * 0.23;
        let Some(u) = local(seconds, delay, 3.6) else {
            continue;
        };
        let size = 12.6 * (1. - 0.84 * along);
        let wide = size * 1.45;
        let alpha = key(u, &[(0., 0.), (0.12, 1.), (0.6, 0.9), (1., 0.)]) * fade;
        let land = (u / 0.12).min(1.);
        let scale = 0.2 + 0.8 * land;
        let x = along * w + key(u, &[(0.6, 0.), (1., 0.18 * wide)]);
        let y = top + staff_y(along, step, height) - (1. - land) * 0.4 * size;
        if let Some(ring) = local(seconds, delay, 1.1) {
            shapes.push(Shape::Ring {
                x,
                y,
                r: 0.9 * size * (0.4 + 2.2 * ring),
                color: tint(clay, key(ring, &[(0., 0.), (0.2, 0.6), (1., 0.)]) * fade),
            });
        }
        shapes.push(Shape::Dot {
            x,
            y,
            r: size * 0.9 * scale,
            color: tint(clay, 0.2 * alpha),
        });
        let (rx, ry) = (wide / 2. * scale, size / 2. * scale);
        let tilt = -22f32.to_radians();
        let (s, c) = tilt.sin_cos();
        let head: Vec<(f32, f32)> = (0..20)
            .map(|k| {
                let a = k as f32 / 20. * std::f32::consts::TAU;
                let (ex, ey) = (rx * a.cos(), ry * a.sin());
                (x + ex * c - ey * s, y + ex * s + ey * c)
            })
            .collect();
        shapes.push(Shape::Poly {
            points: head,
            closed: true,
            fill: true,
            width: 0.,
            color: tint(note, alpha),
        });
        let stem = (size * 0.12).max(1.);
        let reach = 3.1 * size * scale;
        let (edge, y0, y1) = if step >= 1 {
            (
                x - rx,
                y - 0.05 * size * scale,
                y - 0.05 * size * scale + reach,
            )
        } else {
            (
                x + rx,
                y + 0.05 * size * scale,
                y + 0.05 * size * scale - reach,
            )
        };
        shapes.push(Shape::Line {
            from: (edge, y0),
            to: (edge, y1),
            width: stem,
            color: tint(note, 0.85 * alpha),
        });
    }
    shapes
}

fn point_at(origin: gpui::Point<Pixels>, (x, y): (f32, f32)) -> gpui::Point<Pixels> {
    origin + point(px(x), px(y))
}

fn paint(shapes: &[Shape], origin: gpui::Point<Pixels>, window: &mut Window) {
    for shape in shapes {
        match shape {
            Shape::Dot { x, y, r, color } => {
                if color.a <= 0.002 || *r <= 0. {
                    continue;
                }
                let bounds = Bounds::new(
                    point_at(origin, (x - r, y - r)),
                    size(px(r * 2.), px(r * 2.)),
                );
                window.paint_quad(fill(bounds, *color).corner_radii(px(*r)));
            }
            Shape::Ring { x, y, r, color } => {
                if color.a <= 0.002 || *r <= 0. {
                    continue;
                }
                let bounds = Bounds::new(
                    point_at(origin, (x - r, y - r)),
                    size(px(r * 2.), px(r * 2.)),
                );
                window.paint_quad(
                    fill(bounds, gpui::transparent_black())
                        .corner_radii(px(*r))
                        .border_widths(px(1.))
                        .border_color(*color),
                );
            }
            Shape::Line {
                from,
                to,
                width,
                color,
            } => {
                if color.a <= 0.002 {
                    continue;
                }
                let mut path = PathBuilder::stroke(px(*width));
                path.move_to(point_at(origin, *from));
                path.line_to(point_at(origin, *to));
                if let Ok(path) = path.build() {
                    window.paint_path(path, *color);
                }
            }
            Shape::Poly {
                points,
                closed,
                fill: filled,
                width,
                color,
            } => {
                if color.a <= 0.002 || points.len() < 2 {
                    continue;
                }
                let mut path = if *filled {
                    PathBuilder::fill()
                } else {
                    PathBuilder::stroke(px(*width))
                };
                path.move_to(point_at(origin, points[0]));
                for p in &points[1..] {
                    path.line_to(point_at(origin, *p));
                }
                if *closed || *filled {
                    path.close();
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, *color);
                }
            }
        }
    }
}

/// The scene a conversation is playing, and the composer measurement the
/// Opus staff needs.
#[derive(Default)]
pub(crate) struct State {
    playing: Option<(Scene, Instant)>,
    composer: AnchorSlot,
}

impl State {
    /// Starts the scene a picked model earns, if any. Skipped under reduced
    /// motion, like Tauri's `prefers-reduced-motion` check.
    pub(crate) fn start(&mut self, values: &[&str], cx: &App) {
        self.playing = if cx.reduce_motion() {
            None
        } else {
            scene_for(values).map(|scene| (scene, Instant::now()))
        };
    }

    /// The composer's box, which the Opus staff plays around.
    pub(crate) fn composer_probe(&self) -> impl IntoElement {
        self.composer.probe()
    }

    /// Forgets a scene that has played out. Returns whether one is still on.
    pub(crate) fn active(&mut self) -> bool {
        if let Some((scene, started)) = self.playing
            && started.elapsed() >= scene.duration()
        {
            self.playing = None;
        }
        self.playing.is_some()
    }

    /// The decoration to lay over the pane: absolute, no hitbox, so input
    /// goes through it.
    pub(crate) fn overlay(&self) -> Option<AnyElement> {
        let (scene, started) = self.playing?;
        let composer = self.composer.clone();
        Some(
            canvas(
                |bounds, _, _| bounds,
                move |bounds, _, window, cx| {
                    let t = started.elapsed().as_secs_f32() / scene.duration().as_secs_f32();
                    if t >= 1. {
                        return;
                    }
                    pulse::request(pulse::Rate::Smooth, window, cx);
                    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                    let light = theme::is_light();
                    let shapes = match scene {
                        Scene::Astra => astra(t, w, h, light),
                        Scene::Opus => {
                            let measured = composer.get().map(|c| {
                                (
                                    f32::from(c.origin.y - bounds.origin.y),
                                    f32::from(c.origin.y + c.size.height - bounds.origin.y),
                                )
                            });
                            opus(t, w, h, opus_stage(h, measured), light)
                        }
                    };
                    window.with_content_mask(Some(ContentMask { bounds }), |window| {
                        paint(&shapes, bounds.origin, window);
                    });
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn astra_is_a_word_of_its_own() {
        for yes in ["gpt-astra", "GPT-5.5 Astra", "astra", "gpt-6.1-astra-mini"] {
            assert_eq!(scene_for(&[yes]), Some(Scene::Astra), "{yes}");
        }
        for no in ["astrally", "castra", "gpt-6.1-sol", ""] {
            assert_eq!(scene_for(&[no]), None, "{no}");
        }
    }

    #[test]
    fn opus_55_is_matched_across_spellings_only() {
        for yes in [
            "opus-5-5",
            "Opus 5.5",
            "claude-opus-5-5-20261001",
            "opus5.5",
            "Claude Opus 5.5",
        ] {
            assert_eq!(scene_for(&[yes]), Some(Scene::Opus), "{yes}");
        }
        for no in [
            "opus",
            "opus-5.50",
            "opus-5.5.1",
            "opus-4-5",
            "xopus-5-5",
            "sonnet-5.5",
        ] {
            assert_eq!(scene_for(&[no]), None, "{no}");
        }
    }

    #[test]
    fn any_visible_value_can_earn_the_scene() {
        assert_eq!(scene_for(&["custom-id", "Opus 5.5"]), Some(Scene::Opus));
    }

    #[test]
    fn stage_sits_under_a_centered_composer_and_over_a_docked_one() {
        assert_eq!(opus_stage(800., None), (440., 360.));
        assert_eq!(opus_stage(800., Some((300., 400.))), (400., 400.));
        // A docked composer leaves 30px below: the band moves above it.
        assert_eq!(opus_stage(800., Some((650., 770.))), (410., 240.));
        // Little room above as well: the band shrinks to what is there.
        assert_eq!(opus_stage(300., Some((100., 290.))), (0., 100.));
    }

    #[test]
    fn keyframes_interpolate_and_hold() {
        let frames = [(0., 0.), (0.5, 1.), (1., 0.)];
        assert_eq!(key(-1., &frames), 0.);
        assert!((key(0.25, &frames) - 0.5).abs() < 1e-6);
        assert!((key(0.75, &frames) - 0.5).abs() < 1e-6);
        assert_eq!(key(2., &frames), 0.);
    }

    #[test]
    fn scenes_start_and_end_empty_of_visible_marks() {
        for (w, h) in [(900., 700.), (420., 300.)] {
            for shapes in [
                astra(0., w, h, false),
                astra(1., w, h, false),
                opus(0., w, h, (h * 0.55, h * 0.45), false),
                opus(1., w, h, (h * 0.55, h * 0.45), true),
            ] {
                for shape in shapes {
                    let alpha = match shape {
                        Shape::Dot { color, .. }
                        | Shape::Ring { color, .. }
                        | Shape::Line { color, .. }
                        | Shape::Poly { color, .. } => color.a,
                    };
                    assert!(alpha < 0.002, "{shape:?} is visible at an end");
                }
            }
        }
    }

    #[test]
    fn scenes_draw_something_midway() {
        assert!(astra(0.4, 900., 700., false).len() > 100);
        assert!(opus(0.5, 900., 700., (385., 315.), false).len() > 20);
    }

    #[test]
    fn a_stage_with_no_height_draws_only_the_glow() {
        let shapes = opus(0.5, 900., 700., (700., 0.), false);
        assert!(shapes.iter().all(|s| matches!(s, Shape::Dot { .. })));
    }

    #[test]
    fn staff_gathers_toward_the_right() {
        let spread = |t: f32| (staff_y(t, 4, 300.) - staff_y(t, -4, 300.)).abs();
        assert!(spread(0.) > spread(1.));
    }

    #[test]
    fn a_finished_scene_clears() {
        let mut state = State {
            playing: Some((Scene::Astra, Instant::now() - Duration::from_secs(10))),
            ..Default::default()
        };
        assert!(!state.active());
        assert!(state.playing.is_none());
        state.playing = Some((Scene::Opus, Instant::now()));
        assert!(state.active());
    }
}
