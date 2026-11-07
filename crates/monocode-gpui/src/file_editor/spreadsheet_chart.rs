//! A workbook's chart drawn from its own series: columns, bars, lines,
//! areas, scatter, pie and doughnut. Ported from `SpreadsheetChart.tsx`.
//!
//! The chart is first laid out as a [`Scene`] of rectangles, polygons and
//! text labels in the chart's own pixels (no GPUI types, so the layout is
//! tested without a window), then painted by [`element`]. Text and axes follow
//! the theme; series keep the colours the file gave them, and a series
//! without one takes the next hue of Office's default palette.
use std::f32::consts::{PI, TAU};
use std::sync::Arc;

use gpui::{
    AnyElement, Bounds, FontWeight, Hsla, PathBuilder, Window, canvas, div, point, prelude::*, px,
    rgb, size,
};
use monocode_core::office::drawings::{ChartKind, ChartSeries, SheetChart};

use super::chart_scale::{Scale, axis_label, nice_scale, stacked_segments, ticks, value_range};
use crate::theme::{background_base, content};

const FONT: f32 = 10.;
const PAD: f32 = 8.;
/// Width of the strip the rotated axis title takes.
const SIDE_TITLE_W: f32 = 14.;
/// Width of one character at the chart's text size, for fitting labels.
const CHAR_WIDTH: f32 = 5.6;
/// Office's accent hues, in the order a chart without colours takes them.
pub(super) const PALETTE: [u32; 6] = [0x4472c4, 0xed7d31, 0xa5a5a5, 0xffc000, 0x5b9bd5, 0x70ad47];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Ink {
    /// The theme's text colour at this opacity.
    Content(f32),
    /// A series colour (0xrrggbb) at this opacity.
    Rgb(u32, f32),
    /// The surface the chart sits on.
    Background,
}

impl Ink {
    fn color(self) -> Hsla {
        match self {
            Ink::Content(alpha) => content(alpha),
            Ink::Rgb(value, alpha) => {
                let mut color: Hsla = rgb(value).into();
                color.a = alpha;
                color
            }
            Ink::Background => background_base(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Shape {
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        ink: Ink,
    },
    /// A polygon, or an open line when `closed` is false.
    Poly {
        points: Vec<(f32, f32)>,
        closed: bool,
        fill: Option<Ink>,
        stroke: Option<(Ink, f32)>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Anchor {
    Start,
    Middle,
    End,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Label {
    pub x: f32,
    /// The text's baseline.
    pub y: f32,
    pub text: String,
    pub anchor: Anchor,
    pub size: f32,
    pub ink: Ink,
    pub bold: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Scene {
    pub width: f32,
    pub height: f32,
    pub shapes: Vec<Shape>,
    pub labels: Vec<Label>,
    /// The rotated axis title as SVG markup and where it sits
    /// (left, top, width, height).
    pub side_title: Option<(String, f32, f32, f32, f32)>,
}

/// `#rrggbb` as a number.
pub(super) fn parse_hex(value: &str) -> Option<u32> {
    let digits = value.strip_prefix('#')?;
    match digits.len() {
        6 => u32::from_str_radix(digits, 16).ok(),
        3 => {
            let short = u32::from_str_radix(digits, 16).ok()?;
            let (r, g, b) = ((short >> 8) & 0xf, (short >> 4) & 0xf, short & 0xf);
            Some(((r * 17) << 16) | ((g * 17) << 8) | (b * 17))
        }
        _ => None,
    }
}

/// Black or white, whichever reads on the fill.
pub(super) fn readable_on(fill: u32) -> u32 {
    let (r, g, b) = (
        (fill >> 16 & 0xff) as f32,
        (fill >> 8 & 0xff) as f32,
        (fill & 0xff) as f32,
    );
    if 0.299 * r + 0.587 * g + 0.114 * b > 150. {
        0x000000
    } else {
        0xffffff
    }
}

fn series_color(entry: &ChartSeries, index: usize) -> u32 {
    entry
        .color
        .as_deref()
        .and_then(parse_hex)
        .unwrap_or(PALETTE[index % PALETTE.len()])
}

fn point_color(entry: &ChartSeries, series_index: usize, point: usize) -> u32 {
    entry
        .point_colors
        .as_ref()
        .and_then(|colors| colors.get(point))
        .and_then(|color| color.as_deref())
        .and_then(parse_hex)
        .unwrap_or_else(|| series_color(entry, series_index + point))
}

fn clip(label: &str, keep: usize) -> String {
    if label.chars().count() > keep {
        let head: String = label.chars().take(keep - 1).collect();
        format!("{head}…")
    } else {
        label.to_string()
    }
}

#[derive(Clone, Copy)]
struct Frame {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
}

#[derive(Default)]
struct Builder {
    shapes: Vec<Shape>,
    labels: Vec<Label>,
    side_title: Option<(String, f32, f32, f32, f32)>,
}

impl Builder {
    fn label(&mut self, x: f32, y: f32, text: impl Into<String>, anchor: Anchor, alpha: f32) {
        self.labels.push(Label {
            x,
            y,
            text: text.into(),
            anchor,
            size: FONT,
            ink: Ink::Content(alpha),
            bold: false,
        });
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, ink: Ink) {
        self.shapes.push(Shape::Rect { x, y, w, h, ink });
    }

    /// Gridlines and value labels along one axis.
    fn value_axis(&mut self, scale: Scale, frame: Frame, horizontal: bool, title: Option<&str>) {
        let span = (scale.max - scale.min).max(f64::EPSILON);
        let at = |value: f64| -> f32 {
            let share = ((value - scale.min) / span) as f32;
            if horizontal {
                frame.left + share * (frame.right - frame.left)
            } else {
                frame.bottom - share * (frame.bottom - frame.top)
            }
        };
        for value in ticks(scale) {
            let position = at(value);
            let ink = Ink::Content(if value == 0. { 0.35 } else { 0.1 });
            if horizontal {
                self.rect(position, frame.top, 1., frame.bottom - frame.top, ink);
                self.label(
                    position,
                    frame.bottom + 12.,
                    axis_label(value),
                    Anchor::Middle,
                    0.6,
                );
            } else {
                self.rect(frame.left, position, frame.right - frame.left, 1., ink);
                self.label(
                    frame.left - 4.,
                    position + 3.,
                    axis_label(value),
                    Anchor::End,
                    0.6,
                );
            }
        }
        if let (Some(title), true) = (title, horizontal) {
            self.label(
                (frame.left + frame.right) / 2.,
                frame.bottom + 26.,
                title,
                Anchor::Middle,
                0.7,
            );
        }
    }

    /// A line through the points, broken where a value is missing, with a dot
    /// on each point of a short series.
    fn series_line(
        &mut self,
        color: u32,
        points: &[Option<(f32, f32)>],
        baseline: &dyn Fn((f32, f32)) -> (f32, f32),
        as_area: bool,
    ) {
        if as_area {
            let drawn: Vec<(f32, f32)> = points.iter().flatten().copied().collect();
            if drawn.len() < 2 {
                return;
            }
            let mut outline = drawn.clone();
            outline.push(baseline(drawn[drawn.len() - 1]));
            outline.push(baseline(drawn[0]));
            self.shapes.push(Shape::Poly {
                points: outline,
                closed: true,
                fill: Some(Ink::Rgb(color, 0.55)),
                stroke: Some((Ink::Rgb(color, 1.), 1.)),
            });
            return;
        }
        let mut run: Vec<(f32, f32)> = Vec::new();
        let flush = |run: &mut Vec<(f32, f32)>, shapes: &mut Vec<Shape>| {
            if run.len() > 1 {
                shapes.push(Shape::Poly {
                    points: std::mem::take(run),
                    closed: false,
                    fill: None,
                    stroke: Some((Ink::Rgb(color, 1.), 2.)),
                });
            } else {
                run.clear();
            }
        };
        for point in points {
            match point {
                Some(point) => run.push(*point),
                None => flush(&mut run, &mut self.shapes),
            }
        }
        flush(&mut run, &mut self.shapes);
        if points.len() <= 60 {
            for (x, y) in points.iter().flatten() {
                self.shapes.push(Shape::Poly {
                    points: circle(*x, *y, 2.5),
                    closed: true,
                    fill: Some(Ink::Rgb(color, 1.)),
                    stroke: None,
                });
            }
        }
    }
}

fn circle(cx: f32, cy: f32, radius: f32) -> Vec<(f32, f32)> {
    (0..12)
        .map(|step| {
            let angle = step as f32 / 12. * TAU;
            (cx + radius * angle.cos(), cy + radius * angle.sin())
        })
        .collect()
}

fn arc(cx: f32, cy: f32, radius: f32, from: f32, to: f32) -> Vec<(f32, f32)> {
    let steps = (((to - from).abs() / TAU) * 96.).ceil().max(2.) as usize;
    (0..=steps)
        .map(|step| {
            let angle = from + (to - from) * step as f32 / steps as f32;
            (cx + radius * angle.cos(), cy + radius * angle.sin())
        })
        .collect()
}

/// Lay a chart out in its own pixels.
pub(super) fn build(chart: &SheetChart) -> Scene {
    let (width, height) = (chart.bounds.width as f32, chart.bounds.height as f32);
    let round = chart
        .series
        .iter()
        .any(|entry| matches!(entry.kind, ChartKind::Pie | ChartKind::Doughnut));
    let legend_items: Vec<(String, Option<u32>)> = if round {
        chart
            .categories
            .iter()
            .enumerate()
            .map(|(index, label)| {
                let color = chart.series.first().map(|s| point_color(s, 0, index));
                (label.clone(), color)
            })
            .collect()
    } else {
        chart
            .series
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.name.clone(), Some(series_color(entry, index))))
            .collect()
    };
    let show_legend = chart.legend && legend_items.len() > usize::from(!round);
    let top = PAD + if chart.title.is_some() { 20. } else { 0. };
    let bottom = height - PAD - if show_legend { 18. } else { 0. };

    let mut b = Builder::default();
    if let Some(title) = &chart.title {
        b.labels.push(Label {
            x: width / 2.,
            y: PAD + 12.,
            text: title.clone(),
            anchor: Anchor::Middle,
            size: 12.,
            ink: Ink::Content(1.),
            bold: true,
        });
    }
    if chart.unsupported.is_some() || chart.series.is_empty() {
        let text = match &chart.unsupported {
            Some(kind) => format!("{kind} chart — open the file to see it"),
            None => "This chart has no data".into(),
        };
        b.labels.push(Label {
            x: width / 2.,
            y: (top + bottom) / 2.,
            text,
            anchor: Anchor::Middle,
            size: 11.,
            ink: Ink::Content(0.6),
            bold: false,
        });
    } else if round {
        pie(&mut b, chart, width, top, bottom);
    } else if chart
        .series
        .iter()
        .all(|entry| entry.kind == ChartKind::Scatter)
    {
        scatter(&mut b, chart, width, top, bottom);
    } else {
        categories(&mut b, chart, width, top, bottom);
    }
    if show_legend {
        legend(&mut b, &legend_items, width, height - PAD - 6.);
    }
    Scene {
        width,
        height,
        shapes: b.shapes,
        labels: b.labels,
        side_title: b.side_title,
    }
}

fn legend(b: &mut Builder, items: &[(String, Option<u32>)], width: f32, y: f32) {
    // Each entry takes the room its label needs, centred as a row.
    let sizes: Vec<f32> = items
        .iter()
        .map(|(label, _)| 14. + label.chars().count().min(24) as f32 * CHAR_WIDTH)
        .collect();
    let total: f32 = sizes.iter().map(|size| size + 10.).sum::<f32>() - 10.;
    let mut x = PAD.max((width - total) / 2.);
    for (index, (label, color)) in items.iter().enumerate() {
        let at = x;
        x += sizes[index] + 10.;
        if at + 14. > width {
            continue;
        }
        if let Some(color) = color {
            b.rect(at, y - 7., 9., 9., Ink::Rgb(*color, 1.));
        }
        b.label(at + 13., y + 1., clip(label, 24), Anchor::Start, 0.75);
    }
}

fn categories(b: &mut Builder, chart: &SheetChart, width: f32, top: f32, bottom: f32) {
    let count = chart.categories.len();
    if count == 0 {
        return;
    }
    let horizontal = chart.series.iter().any(|e| e.kind == ChartKind::Bar);
    let indexed: Vec<(usize, &ChartSeries)> = chart.series.iter().enumerate().collect();
    let blocks: Vec<(usize, &ChartSeries)> = indexed
        .iter()
        .copied()
        .filter(|(_, e)| matches!(e.kind, ChartKind::Column | ChartKind::Bar))
        .collect();
    let others: Vec<(usize, &ChartSeries)> = indexed
        .iter()
        .copied()
        .filter(|(_, e)| matches!(e.kind, ChartKind::Line | ChartKind::Area))
        .collect();
    let any_area = others.iter().any(|(_, e)| e.kind == ChartKind::Area);
    let block_refs: Vec<&ChartSeries> = blocks.iter().map(|(_, e)| *e).collect();
    let other_refs: Vec<&ChartSeries> = others.iter().map(|(_, e)| *e).collect();
    let stacked = stacked_segments(&block_refs, chart.stacking, count);
    let flat = stacked_segments(
        &other_refs,
        any_area.then_some(chart.stacking).flatten(),
        count,
    );
    let all: Vec<_> = stacked.iter().chain(flat.iter()).cloned().collect();
    let (low, high) = value_range(&all, !blocks.is_empty() || any_area);
    let scale = nice_scale(low, high, 5);
    let label_width = ticks(scale)
        .iter()
        .map(|value| axis_label(*value).chars().count())
        .max()
        .unwrap_or(1) as f32
        * CHAR_WIDTH
        + 8.;
    let category_width = if horizontal {
        90f32.min(
            chart
                .categories
                .iter()
                .map(|l| l.chars().count())
                .max()
                .unwrap_or(0) as f32
                * CHAR_WIDTH
                + 8.,
        )
    } else {
        0.
    };
    let (value_title, category_title) = if horizontal {
        (chart.x_title.as_deref(), chart.y_title.as_deref())
    } else {
        (chart.y_title.as_deref(), chart.x_title.as_deref())
    };
    let side_title = if horizontal {
        category_title
    } else {
        value_title
    };
    let frame = Frame {
        left: PAD
            + if side_title.is_some() {
                SIDE_TITLE_W
            } else {
                0.
            }
            + if horizontal {
                category_width
            } else {
                label_width
            },
        right: width - PAD - 4.,
        top: top + 4.,
        bottom: bottom - 16. - if chart.x_title.is_some() { 14. } else { 0. },
    };
    if frame.right <= frame.left || frame.bottom <= frame.top {
        return;
    }
    // The vertical axis' title reads bottom-up beside it, as the desktop's
    // rotated label; a flat title would sit above the axis instead.
    b.side_title = side_title.map(|title| {
        let (w, h) = (SIDE_TITLE_W, frame.bottom - frame.top);
        let (x, y) = (w / 2., h / 2.);
        let ink: gpui::Rgba = Ink::Content(0.7).color().into();
        let (r, g, b) = (
            (ink.r * 255.).round() as u8,
            (ink.g * 255.).round() as u8,
            (ink.b * 255.).round() as u8,
        );
        let title = title
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        (
            format!(
                "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 {w} {h}'>\
                 <text x='{x}' y='{y}' fill='#{r:02x}{g:02x}{b:02x}' font-size='9' \
                 font-family='sans-serif' text-anchor='middle' dominant-baseline='middle' \
                 transform='rotate(-90 {x} {y})'>{title}</text></svg>",
            ),
            0.,
            frame.top,
            w,
            h,
        )
    });
    let span = if horizontal {
        frame.bottom - frame.top
    } else {
        frame.right - frame.left
    };
    let band = span / count as f32;
    let range = (scale.max - scale.min).max(f64::EPSILON);
    let value = |amount: f64| -> f32 {
        let share = ((amount - scale.min) / range) as f32;
        if horizontal {
            frame.left + share * (frame.right - frame.left)
        } else {
            frame.bottom - share * (frame.bottom - frame.top)
        }
    };
    let center = |index: usize| -> f32 {
        (if horizontal { frame.top } else { frame.left }) + band * index as f32 + band / 2.
    };
    let zero = value(0f64.clamp(scale.min, scale.max));
    let clusters = if chart.stacking.is_some() {
        1
    } else {
        blocks.len().max(1)
    };
    let bar_size = band * 0.7 / clusters as f32;
    // Category labels thin out when they would overlap.
    let every = if horizontal {
        (12. / band).ceil()
    } else {
        let longest = chart
            .categories
            .iter()
            .map(|l| l.chars().count().min(14))
            .max()
            .unwrap_or(0) as f32;
        ((longest * CHAR_WIDTH + 6.) / band).ceil()
    }
    .max(1.) as usize;

    b.value_axis(scale, frame, horizontal, value_title);
    if !horizontal && let Some(title) = category_title {
        b.label(
            (frame.left + frame.right) / 2.,
            frame.bottom + 28.,
            title,
            Anchor::Middle,
            0.7,
        );
    }
    for (index, label) in chart.categories.iter().enumerate() {
        if index % every != 0 {
            continue;
        }
        let text = clip(label, 14);
        if horizontal {
            b.label(frame.left - 4., center(index) + 3., text, Anchor::End, 0.6);
        } else {
            b.label(center(index), frame.bottom + 12., text, Anchor::Middle, 0.6);
        }
    }
    for (slot, (series_index, entry)) in blocks.iter().enumerate() {
        let ink = Ink::Rgb(series_color(entry, *series_index), 1.);
        for (index, segment) in stacked[slot].iter().enumerate() {
            if entry.values.get(index).copied().flatten().is_none() {
                continue;
            }
            let offset = if chart.stacking.is_some() { 0 } else { slot };
            let start = center(index) - bar_size * clusters as f32 / 2. + offset as f32 * bar_size;
            let a = if chart.stacking.is_some() {
                value(segment.from)
            } else {
                zero
            };
            let c = value(segment.to);
            if horizontal {
                b.rect(a.min(c), start, (c - a).abs(), (bar_size - 1.).max(1.), ink);
            } else {
                b.rect(start, a.min(c), (bar_size - 1.).max(1.), (c - a).abs(), ink);
            }
        }
    }
    for (slot, (series_index, entry)) in others.iter().enumerate() {
        let points: Vec<Option<(f32, f32)>> = flat[slot]
            .iter()
            .enumerate()
            .map(|(index, segment)| {
                entry.values.get(index).copied().flatten()?;
                Some(if horizontal {
                    (value(segment.to), center(index))
                } else {
                    (center(index), value(segment.to))
                })
            })
            .collect();
        let baseline = |p: (f32, f32)| if horizontal { (zero, p.1) } else { (p.0, zero) };
        b.series_line(
            series_color(entry, *series_index),
            &points,
            &baseline,
            entry.kind == ChartKind::Area,
        );
    }
}

fn scatter(b: &mut Builder, chart: &SheetChart, width: f32, top: f32, bottom: f32) {
    let xs: Vec<f64> = chart
        .series
        .iter()
        .flat_map(|e| e.x_values.iter().flatten().flatten().copied())
        .collect();
    let ys: Vec<f64> = chart
        .series
        .iter()
        .flat_map(|e| e.values.iter().flatten().copied())
        .collect();
    if xs.is_empty() || ys.is_empty() {
        return;
    }
    let min = |v: &[f64]| v.iter().copied().fold(f64::INFINITY, f64::min);
    let max = |v: &[f64]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let x_scale = nice_scale(min(&xs), max(&xs), 5);
    let y_scale = nice_scale(min(&ys), max(&ys), 5);
    let label_width = ticks(y_scale)
        .iter()
        .map(|v| axis_label(*v).chars().count())
        .max()
        .unwrap_or(1) as f32
        * CHAR_WIDTH
        + 8.;
    let frame = Frame {
        left: PAD + label_width,
        right: width - PAD - 8.,
        top: top + 4. + if chart.y_title.is_some() { 12. } else { 0. },
        bottom: bottom - 16. - if chart.x_title.is_some() { 14. } else { 0. },
    };
    if frame.right <= frame.left || frame.bottom <= frame.top {
        return;
    }
    if let Some(title) = &chart.y_title {
        b.label(PAD, top + 8., title.clone(), Anchor::Start, 0.7);
    }
    let x = |value: f64| {
        frame.left
            + ((value - x_scale.min) / (x_scale.max - x_scale.min)) as f32
                * (frame.right - frame.left)
    };
    let y = |value: f64| {
        frame.bottom
            - ((value - y_scale.min) / (y_scale.max - y_scale.min)) as f32
                * (frame.bottom - frame.top)
    };
    b.value_axis(y_scale, frame, false, None);
    b.value_axis(x_scale, frame, true, chart.x_title.as_deref());
    for (index, entry) in chart.series.iter().enumerate() {
        let points: Vec<Option<(f32, f32)>> = entry
            .values
            .iter()
            .enumerate()
            .map(|(point, value)| {
                let at = entry.x_values.as_ref()?.get(point).copied().flatten()?;
                Some((x(at), y((*value)?)))
            })
            .collect();
        b.series_line(series_color(entry, index), &points, &|p| p, false);
    }
}

fn pie(b: &mut Builder, chart: &SheetChart, width: f32, top: f32, bottom: f32) {
    let Some(series) = chart.series.first() else {
        return;
    };
    let values: Vec<f64> = series
        .values
        .iter()
        .map(|v| v.unwrap_or(0.).max(0.))
        .collect();
    let total: f64 = values.iter().sum();
    let radius = ((width - PAD * 2.).min(bottom - top) / 2. - 4.).max(0.);
    if total <= 0. || radius <= 0. {
        return;
    }
    let (cx, cy) = (width / 2., (top + bottom) / 2.);
    let inner = if series.kind == ChartKind::Doughnut {
        radius * 0.55
    } else {
        0.
    };
    let mut angle = -PI / 2.;
    for (index, value) in values.iter().enumerate() {
        if *value <= 0. {
            continue;
        }
        let start = angle;
        let sweep = (*value / total) as f32 * TAU;
        angle += sweep;
        let color = point_color(series, 0, index);
        let mut points;
        if sweep >= TAU - 1e-6 {
            points = circle_fine(cx, cy, radius);
            if inner > 0. {
                // The inner ring runs the other way, which cuts the hole.
                let mut hole = circle_fine(cx, cy, inner);
                hole.reverse();
                points.extend(hole);
            }
        } else {
            points = arc(cx, cy, radius, start, angle);
            if inner > 0. {
                let mut back = arc(cx, cy, inner, start, angle);
                back.reverse();
                points.extend(back);
            } else {
                points.push((cx, cy));
            }
        }
        b.shapes.push(Shape::Poly {
            points,
            closed: true,
            fill: Some(Ink::Rgb(color, 1.)),
            stroke: Some((Ink::Background, 1.)),
        });
        let share = (*value / total * 100.).round();
        if share >= 5. {
            let middle = start + sweep / 2.;
            let label_radius = if inner > 0. {
                (radius + inner) / 2.
            } else {
                radius * 0.65
            };
            b.labels.push(Label {
                x: cx + label_radius * middle.cos(),
                y: cy + label_radius * middle.sin() + 3.,
                text: format!("{share}%"),
                anchor: Anchor::Middle,
                size: FONT,
                ink: Ink::Rgb(readable_on(color), 1.),
                bold: false,
            });
        }
    }
}

fn circle_fine(cx: f32, cy: f32, radius: f32) -> Vec<(f32, f32)> {
    arc(cx, cy, radius, 0., TAU)
}

fn paint_poly(
    window: &mut Window,
    origin: gpui::Point<gpui::Pixels>,
    points: &[(f32, f32)],
    closed: bool,
    fill: Option<Ink>,
    stroke: Option<(Ink, f32)>,
) {
    if points.len() < 2 {
        return;
    }
    let at = |(x, y): (f32, f32)| origin + point(px(x), px(y));
    if let Some(ink) = fill {
        let mut path = PathBuilder::fill();
        path.move_to(at(points[0]));
        for p in &points[1..] {
            path.line_to(at(*p));
        }
        path.close();
        if let Ok(path) = path.build() {
            window.paint_path(path, ink.color());
        }
    }
    if let Some((ink, width)) = stroke {
        let mut path = PathBuilder::stroke(px(width));
        path.move_to(at(points[0]));
        for p in &points[1..] {
            path.line_to(at(*p));
        }
        if closed {
            path.close();
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, ink.color());
        }
    }
}

/// The chart as an element filling its box: a bordered surface with the
/// shapes painted on a canvas and the labels laid over them.
pub(super) fn element(scene: &Arc<Scene>) -> AnyElement {
    const LABEL_BOX: f32 = 240.;
    let painted = scene.clone();
    let mut surface = div()
        .size_full()
        .relative()
        .overflow_hidden()
        .border_1()
        .border_color(content(0.15))
        .bg(background_base())
        .text_color(content(1.))
        .child(
            canvas(
                |_, _, _| (),
                move |bounds: Bounds<gpui::Pixels>, _, window, _| {
                    for shape in &painted.shapes {
                        match shape {
                            Shape::Rect { x, y, w, h, ink } => {
                                window.paint_quad(gpui::fill(
                                    Bounds::new(
                                        bounds.origin + point(px(*x), px(*y)),
                                        size(px(*w), px(*h)),
                                    ),
                                    ink.color(),
                                ));
                            }
                            Shape::Poly {
                                points,
                                closed,
                                fill,
                                stroke,
                            } => paint_poly(window, bounds.origin, points, *closed, *fill, *stroke),
                        }
                    }
                },
            )
            .absolute()
            .size_full(),
        );
    for label in &scene.labels {
        let left = match label.anchor {
            Anchor::Start => label.x,
            Anchor::Middle => label.x - LABEL_BOX / 2.,
            Anchor::End => label.x - LABEL_BOX,
        };
        let mut text = div()
            .absolute()
            .left(px(left))
            .top(px(label.y - label.size * 0.9))
            .w(px(LABEL_BOX))
            .text_size(px(label.size))
            .line_height(px(label.size * 1.2))
            .text_color(label.ink.color())
            .whitespace_nowrap();
        text = match label.anchor {
            Anchor::Start => text.text_left(),
            Anchor::Middle => text.text_center(),
            Anchor::End => text.text_right(),
        };
        if label.bold {
            text = text.font_weight(FontWeight::SEMIBOLD);
        }
        surface = surface.child(text.child(label.text.clone()));
    }
    if let Some((markup, left, top, w, h)) = &scene.side_title {
        surface = surface.child(
            div()
                .absolute()
                .left(px(*left))
                .top(px(*top))
                .w(px(*w))
                .h(px(*h))
                .child(gpui::svg().data(markup.as_bytes()).size_full()),
        );
    }
    surface.into_any_element()
}

impl Scene {
    /// The text this chart shows, for tests and accessibility.
    #[cfg(test)]
    pub(super) fn texts(&self) -> Vec<&str> {
        self.labels.iter().map(|l| l.text.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::office::drawings::{DrawingBox, Stacking};

    fn series(kind: ChartKind, name: &str, values: &[f64], color: Option<&str>) -> ChartSeries {
        ChartSeries {
            kind,
            name: name.into(),
            values: values.iter().map(|v| Some(*v)).collect(),
            x_values: None,
            color: color.map(str::to_string),
            point_colors: None,
        }
    }

    fn chart(series: Vec<ChartSeries>) -> SheetChart {
        SheetChart {
            bounds: DrawingBox {
                x: 0.,
                y: 0.,
                width: 400.,
                height: 260.,
            },
            title: Some("Ventas".into()),
            categories: vec!["Ene".into(), "Feb".into(), "Mar".into()],
            series,
            stacking: None,
            legend: true,
            x_title: Some("Mes".into()),
            y_title: Some("Unidades".into()),
            unsupported: None,
        }
    }

    fn rects(scene: &Scene) -> Vec<&Shape> {
        scene
            .shapes
            .iter()
            .filter(|s| matches!(s, Shape::Rect { w, h, .. } if *w > 2. && *h > 2.))
            .collect()
    }

    #[test]
    fn columns_are_drawn_per_point_with_axes_titles_and_legend() {
        let scene = build(&chart(vec![
            series(
                ChartKind::Column,
                "Norte",
                &[10., 20., 30.],
                Some("#4472c4"),
            ),
            series(ChartKind::Column, "Sur", &[5., 15., 25.], None),
        ]));
        let texts = scene.texts();
        for expected in [
            "Ventas", "Ene", "Mar", "Mes", "Unidades", "Norte", "Sur", "0", "30",
        ] {
            assert!(texts.contains(&expected), "{expected} in {texts:?}");
        }
        // Six columns plus the two legend swatches.
        let columns = rects(&scene)
            .into_iter()
            .filter(|s| {
                matches!(
                    s,
                    Shape::Rect {
                        ink: Ink::Rgb(..),
                        ..
                    }
                )
            })
            .count();
        assert_eq!(columns, 8);
        // A series without a colour takes the palette's second hue.
        assert!(scene.shapes.iter().any(|s| matches!(
            s,
            Shape::Rect { ink: Ink::Rgb(c, _), .. } if *c == PALETTE[1]
        )));
    }

    #[test]
    fn stacked_columns_share_one_slot_and_bars_run_sideways() {
        let mut stacked = chart(vec![
            series(ChartKind::Column, "A", &[1., 2., 3.], None),
            series(ChartKind::Column, "B", &[1., 2., 3.], None),
        ]);
        stacked.stacking = Some(Stacking::Stacked);
        let scene = build(&stacked);
        let widths: Vec<f32> = scene
            .shapes
            .iter()
            .filter_map(|s| match s {
                Shape::Rect {
                    w,
                    h,
                    ink: Ink::Rgb(c, _),
                    ..
                } if *c == PALETTE[0] && *w > 20. => Some(*w),
                _ => None,
            })
            .collect();
        assert_eq!(widths.len(), 3);
        let mut bars = chart(vec![series(ChartKind::Bar, "A", &[1., 2., 3.], None)]);
        bars.legend = false;
        let scene = build(&bars);
        let wide = scene
            .shapes
            .iter()
            .any(|s| matches!(s, Shape::Rect { w, h, ink: Ink::Rgb(..), .. } if *w > *h));
        assert!(wide);
        assert!(scene.texts().contains(&"Ene"));
    }

    #[test]
    fn lines_areas_scatter_pie_and_doughnut_draw_polygons() {
        let line = build(&chart(vec![series(
            ChartKind::Line,
            "L",
            &[1., 3., 2.],
            None,
        )]));
        assert!(line.shapes.iter().any(|s| matches!(
            s,
            Shape::Poly { closed: false, stroke: Some((_, w)), .. } if *w == 2.
        )));
        let area = build(&chart(vec![series(
            ChartKind::Area,
            "A",
            &[1., 3., 2.],
            None,
        )]));
        assert!(area.shapes.iter().any(|s| matches!(
            s,
            Shape::Poly { closed: true, fill: Some(Ink::Rgb(_, a)), .. } if *a == 0.55
        )));
        let mut dots = series(ChartKind::Scatter, "S", &[1., 4., 2.], None);
        dots.x_values = Some(vec![Some(1.), Some(2.), Some(3.)]);
        let scatter = build(&chart(vec![dots]));
        assert!(scatter.texts().contains(&"Mes"));
        assert!(
            scatter
                .shapes
                .iter()
                .any(|s| matches!(s, Shape::Poly { closed: true, .. }))
        );

        let mut pie_chart = chart(vec![series(ChartKind::Pie, "P", &[50., 30., 20.], None)]);
        pie_chart.series[0].point_colors = Some(vec![
            Some("#ff0000".into()),
            Some("#00ff00".into()),
            Some("#0000ff".into()),
        ]);
        let pie = build(&pie_chart);
        assert_eq!(
            pie.shapes
                .iter()
                .filter(|s| matches!(s, Shape::Poly { closed: true, .. }))
                .count(),
            3
        );
        let texts = pie.texts();
        assert!(texts.contains(&"50%") && texts.contains(&"30%") && texts.contains(&"20%"));
        // The pie's legend lists its categories.
        assert!(texts.contains(&"Feb"));

        let mut ring = chart(vec![series(ChartKind::Doughnut, "D", &[1.], None)]);
        ring.categories = vec!["Todo".into()];
        let ring = build(&ring);
        let Some(Shape::Poly { points, .. }) = ring
            .shapes
            .iter()
            .find(|s| matches!(s, Shape::Poly { closed: true, .. }))
        else {
            panic!("a ring")
        };
        // Outer and inner circle in one polygon.
        assert!(points.len() > 150);
    }

    #[test]
    fn charts_without_data_or_of_an_unknown_kind_say_so() {
        let empty = build(&chart(vec![]));
        assert!(empty.texts().contains(&"This chart has no data"));
        let mut odd = chart(vec![series(ChartKind::Column, "A", &[1.], None)]);
        odd.unsupported = Some("surface".into());
        assert!(
            build(&odd)
                .texts()
                .contains(&"surface chart — open the file to see it")
        );
    }

    #[test]
    fn colours_parse_and_text_picks_black_or_white() {
        assert_eq!(parse_hex("#4472c4"), Some(0x4472c4));
        assert_eq!(parse_hex("#fff"), Some(0xffffff));
        assert_eq!(parse_hex("red"), None);
        assert_eq!(readable_on(0xffffff), 0x000000);
        assert_eq!(readable_on(0x1f2937), 0xffffff);
    }
}
