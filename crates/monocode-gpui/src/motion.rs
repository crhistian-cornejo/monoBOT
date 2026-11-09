//! The app's shared motion: how long things take, the curves they move on,
//! and the entrances several surfaces share. A new transition picks its
//! timing and curve here rather than inventing its own, so the app moves as
//! one thing. The entrances are GPUI animations, which draw their final
//! state under reduced motion.
//!
//! Looping decoration keeps its clock in `pulse`, panes their spring in
//! `pane_motion`, and tabs their widths in `tab_motion`.
use crate::reveal::bezier;
use gpui::{Animation, AnimationElement, AnimationExt, ElementId, IntoElement, Styled, px};
use std::time::Duration;

/// A CSS cubic-bezier: `(x1, y1, x2, y2)`.
pub(crate) type Curve = (f32, f32, f32, f32);

/// Fast out and a long settle: whatever arrives or changes shape.
pub(crate) const EMPHASIZED: Curve = (0.22, 1., 0.36, 1.);
/// CSS `ease-out`: fades and short moves.
pub(crate) const EASE_OUT: Curve = (0., 0., 0.58, 1.);
/// Menus and flyouts opening.
#[allow(dead_code)]
pub(crate) const DECELERATE: Curve = (0.16, 1., 0.3, 1.);
/// Leaving, and turns that start and end at rest.
#[allow(dead_code)]
pub(crate) const STANDARD: Curve = (0.4, 0., 0.2, 1.);
/// Loops that breathe.
#[allow(dead_code)]
pub(crate) const EASE_IN_OUT: Curve = (0.42, 0., 0.58, 1.);

/// Reorders, glows, hover feedback.
#[allow(dead_code)]
pub(crate) const FAST: Duration = Duration::from_millis(160);
/// Modals, fades, tabs, labels.
pub(crate) const QUICK: Duration = Duration::from_millis(200);
/// Panels, pickers, rails.
#[allow(dead_code)]
pub(crate) const NORMAL: Duration = Duration::from_millis(240);
/// A mark that lands: the tick after a copy.
pub(crate) const POP: Duration = Duration::from_millis(260);
/// Morphs between two shapes.
#[allow(dead_code)]
pub(crate) const MEDIUM: Duration = Duration::from_millis(320);
/// Sheets, theme blends, messages entering.
#[allow(dead_code)]
pub(crate) const SLOW: Duration = Duration::from_millis(360);

/// `t` (0 to 1) along `curve`.
pub(crate) fn ease(t: f32, curve: Curve) -> f32 {
    bezier(t, curve.0, curve.1, curve.2, curve.3)
}

/// Cubic ease-out, the curve of the modal card and what enters like it.
pub(crate) fn cubic_out(t: f32) -> f32 {
    1. - (1. - t.clamp(0., 1.)).powi(3)
}

/// Where an entering element comes from.
#[derive(Clone, Copy)]
pub(crate) enum Origin {
    Below,
    Right,
}

/// Fades `element` in while it travels `travel` points into place: a modal
/// card rising, a toast sliding in from the window's edge.
pub(crate) fn enter<E: Styled + IntoElement + 'static>(
    id: impl Into<ElementId>,
    element: E,
    origin: Origin,
    travel: f32,
) -> AnimationElement<E> {
    element.with_animation(
        id,
        Animation::new(QUICK).with_easing(cubic_out),
        move |element, t| {
            let away = px(travel * (1. - t));
            let element = element.relative().opacity(t);
            match origin {
                Origin::Below => element.top(away),
                Origin::Right => element.left(away),
            }
        },
    )
}

/// Fades `element` in where it stands.
pub(crate) fn fade_in<E: Styled + IntoElement + 'static>(
    id: impl Into<ElementId>,
    element: E,
) -> AnimationElement<E> {
    element.with_animation(
        id,
        Animation::new(QUICK).with_easing(cubic_out),
        |element, t| element.opacity(t),
    )
}

/// Grows a `side`-point mark in as something lands, like the tick that
/// answers a copy. A new `id` plays it again.
pub(crate) fn pop_in<E: Styled + IntoElement + 'static>(
    id: impl Into<ElementId>,
    mark: E,
    side: f32,
) -> AnimationElement<E> {
    mark.with_animation(
        id,
        Animation::new(POP).with_easing(cubic_out),
        move |mark, t| mark.size(px(side * (0.5 + 0.5 * t))).opacity(t),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_start_at_rest_and_end_in_place() {
        for curve in [EMPHASIZED, EASE_OUT, DECELERATE, STANDARD, EASE_IN_OUT] {
            assert_eq!(ease(0., curve), 0.);
            assert_eq!(ease(1., curve), 1.);
            // Never backwards: none of the shared curves overshoots.
            let mut last = 0.;
            for step in 1..=20 {
                let at = ease(step as f32 / 20., curve);
                assert!(at >= last - 1e-4, "{curve:?} at {step}");
                last = at;
            }
        }
        assert_eq!(cubic_out(0.), 0.);
        assert_eq!(cubic_out(1.), 1.);
        // Out of range holds the ends, so a late frame cannot overshoot.
        assert_eq!(cubic_out(1.4), 1.);
        assert_eq!(cubic_out(-0.2), 0.);
        assert!(cubic_out(0.5) > 0.5);
    }

    #[test]
    fn the_shared_timings_stay_in_order() {
        let timings = [FAST, QUICK, NORMAL, POP, MEDIUM, SLOW];
        assert!(timings.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
