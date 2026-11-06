//! A chart's axis maths, ported from `chartScale.ts`: round bounds, a 1, 2,
//! 2.5 or 5 step, short labels and the segments stacking leaves.
use monocode_core::office::drawings::{ChartSeries, Stacking};

/// An axis from `min` to `max` in round steps, as a chart labels it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Scale {
    pub min: f64,
    pub max: f64,
    pub step: f64,
}

/// Round bounds and a step that covers the values in about `count` ticks.
pub(super) fn nice_scale(low: f64, high: f64, count: usize) -> Scale {
    let (mut low, mut high) = (low, high);
    if !low.is_finite() || !high.is_finite() {
        return Scale {
            min: 0.,
            max: 1.,
            step: 0.2,
        };
    }
    // A flat series still gets an axis: from zero up to it, or down to it.
    if low == high {
        if low == 0. {
            high = 1.;
        } else if low > 0. {
            low = 0.;
        } else {
            high = 0.;
        }
    }
    let raw = (high - low) / count.max(1) as f64;
    let power = 10f64.powf(raw.log10().floor());
    let step = [1., 2., 2.5, 5., 10.]
        .iter()
        .map(|factor| factor * power)
        .find(|candidate| *candidate >= raw)
        .unwrap_or(10. * power);
    let min = (low / step + 1e-9).floor() * step;
    let max = (high / step - 1e-9).ceil() * step;
    Scale {
        min,
        max: if max == min { min + step } else { max },
        step,
    }
}

/// Every tick from the axis minimum to its maximum.
pub(super) fn ticks(scale: Scale) -> Vec<f64> {
    let mut out = Vec::new();
    let mut value = scale.min;
    // Bounded: a degenerate step must not spin.
    while value <= scale.max + scale.step / 1e6 && out.len() < 200 {
        out.push(to_precision(value, 12));
        value += scale.step;
    }
    out
}

/// `Number(value.toPrecision(digits))`: float noise from adding steps goes.
fn to_precision(value: f64, digits: i32) -> f64 {
    if value == 0. || !value.is_finite() {
        return value;
    }
    let magnitude = value.abs().log10().floor() as i32;
    let factor = 10f64.powi(digits - 1 - magnitude);
    (value * factor).round() / factor
}

fn grouped(value: f64, fraction_digits: usize) -> String {
    let text = format!("{:.*}", fraction_digits, value.abs());
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    let fraction = fraction.trim_end_matches('0');
    let mut digits = String::new();
    for (index, ch) in whole.chars().enumerate() {
        if index > 0 && (whole.len() - index).is_multiple_of(3) {
            digits.push(',');
        }
        digits.push(ch);
    }
    let sign = if value < 0. && (digits != "0" || !fraction.is_empty()) {
        "-"
    } else {
        ""
    };
    if fraction.is_empty() {
        format!("{sign}{digits}")
    } else {
        format!("{sign}{digits}.{fraction}")
    }
}

/// Short axis labels: 1,500, 12.5, 240K, 2.4M.
pub(super) fn axis_label(value: f64) -> String {
    if value.abs() >= 1e5 {
        let (divisor, suffix) = if value.abs() >= 1e12 {
            (1e12, "T")
        } else if value.abs() >= 1e9 {
            (1e9, "B")
        } else if value.abs() >= 1e6 {
            (1e6, "M")
        } else {
            (1e3, "K")
        };
        return format!("{}{suffix}", grouped(value / divisor, 1));
    }
    grouped(value, 2)
}

/// Where one point's segment starts and ends once stacked.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Segment {
    pub from: f64,
    pub to: f64,
}

/// The values each series draws, after stacking: per point, where its segment
/// starts and ends. Percent stacking turns each category into shares of 100.
pub(super) fn stacked_segments(
    series: &[&ChartSeries],
    stacking: Option<Stacking>,
    count: usize,
) -> Vec<Vec<Segment>> {
    let value_at = |entry: &ChartSeries, index: usize| -> f64 {
        entry.values.get(index).copied().flatten().unwrap_or(0.)
    };
    let Some(stacking) = stacking else {
        return series
            .iter()
            .map(|entry| {
                (0..count)
                    .map(|index| Segment {
                        from: 0.,
                        to: value_at(entry, index),
                    })
                    .collect()
            })
            .collect();
    };
    let totals: Vec<f64> = (0..count)
        .map(|index| series.iter().map(|e| value_at(e, index).abs()).sum())
        .collect();
    let mut up = vec![0.; count];
    let mut down = vec![0.; count];
    series
        .iter()
        .map(|entry| {
            (0..count)
                .map(|index| {
                    let mut value = value_at(entry, index);
                    if stacking == Stacking::Percent {
                        value = if totals[index] != 0. {
                            value / totals[index] * 100.
                        } else {
                            0.
                        };
                    }
                    let base = if value >= 0. { &mut up } else { &mut down };
                    let from = base[index];
                    base[index] += value;
                    Segment {
                        from,
                        to: base[index],
                    }
                })
                .collect()
        })
        .collect()
}

/// The lowest and highest values a chart's value axis must show.
pub(super) fn value_range(segments: &[Vec<Segment>], include_zero: bool) -> (f64, f64) {
    let mut low = if include_zero { 0. } else { f64::INFINITY };
    let mut high = if include_zero { 0. } else { f64::NEG_INFINITY };
    for points in segments {
        for segment in points {
            low = low.min(segment.to);
            high = high.max(segment.to);
        }
    }
    if low.is_finite() {
        (low, high)
    } else {
        (0., 1.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::office::drawings::ChartKind;

    fn series(values: &[Option<f64>]) -> ChartSeries {
        ChartSeries {
            kind: ChartKind::Column,
            name: "s".into(),
            values: values.to_vec(),
            x_values: None,
            color: None,
            point_colors: None,
        }
    }

    #[test]
    fn scales_pick_round_bounds_and_steps() {
        assert_eq!(
            nice_scale(0., 98., 5),
            Scale {
                min: 0.,
                max: 100.,
                step: 20.
            }
        );
        let flat = nice_scale(5., 5., 5);
        assert_eq!((flat.min, flat.max), (0., 5.));
        assert_eq!(nice_scale(f64::NAN, 1., 5).step, 0.2);
        let tiny = nice_scale(0., 1., 5);
        assert_eq!(ticks(tiny), [0., 0.2, 0.4, 0.6, 0.8, 1.]);
        let negative = nice_scale(-30., 10., 5);
        assert_eq!(negative.min, -30.);
        assert_eq!(negative.max, 10.);
    }

    #[test]
    fn labels_are_short() {
        assert_eq!(axis_label(1500.), "1,500");
        assert_eq!(axis_label(12.5), "12.5");
        assert_eq!(axis_label(0.), "0");
        assert_eq!(axis_label(-0.4), "-0.4");
        assert_eq!(axis_label(240_000.), "240K");
        assert_eq!(axis_label(2_400_000.), "2.4M");
        assert_eq!(axis_label(3_000_000_000.), "3B");
    }

    #[test]
    fn stacking_adds_up_and_percent_shares_a_hundred() {
        let a = series(&[Some(2.), Some(-1.)]);
        let b = series(&[Some(3.), None]);
        let refs = [&a, &b];
        let flat = stacked_segments(&refs, None, 2);
        assert_eq!(flat[1][0], Segment { from: 0., to: 3. });
        let stacked = stacked_segments(&refs, Some(Stacking::Stacked), 2);
        assert_eq!(stacked[1][0], Segment { from: 2., to: 5. });
        assert_eq!(stacked[0][1], Segment { from: 0., to: -1. });
        let percent = stacked_segments(&refs, Some(Stacking::Percent), 2);
        assert_eq!(percent[0][0].to, 40.);
        assert_eq!(percent[1][0].to, 100.);
        assert_eq!(value_range(&stacked, true), (-1., 5.));
        assert_eq!(value_range(&[], false), (0., 1.));
    }
}
