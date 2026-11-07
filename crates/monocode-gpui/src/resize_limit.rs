//! Port of src/shared/lib/resizeLimit.ts and its `[data-resize-limit]` rules
//! in index.css: a resize handle pushed past its limit glows in the accent
//! near the pointer, stronger the further it goes, so a pane that stops
//! shrinking says why instead of just going still. The glow fades when the
//! drag lets go.
use crate::theme::accent_color;
use gpui::{
    Bounds, BoxShadow, ContentMask, Corners, IntoElement, Styled, canvas, deferred, fill,
    linear_color_stop, linear_gradient, point, px, size,
};
use std::time::{Duration, Instant};

/// The axis a handle travels: X for a vertical strip, Y for a horizontal one.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Axis {
    X,
    Y,
}

/// How far past the limit, in px, the glow reaches full strength.
const FULL_STRAIN_PX: f32 = 72.;
/// The glow is already visible the moment the limit is hit.
const FIRST_STRAIN: f32 = 0.35;
/// How far the glow leans toward the pointer at full strength, in px.
const MAX_PULL_PX: f32 = 4.;
/// The release fade, the ::after's opacity transition.
const FADE: Duration = Duration::from_millis(160);

/// limitStrain: 0 inside the limits, then rising from FIRST_STRAIN toward 1.
pub(crate) fn limit_strain(overshoot: f32) -> f32 {
    let past = overshoot.abs();
    if !past.is_finite() || past < 1. {
        return 0.;
    }
    FIRST_STRAIN + (1. - FIRST_STRAIN) * (1. - (-past / FULL_STRAIN_PX).exp())
}

/// limitPull: how far the glow leans in the pointer's direction, in px.
fn limit_pull(overshoot: f32) -> f32 {
    let strain = limit_strain(overshoot);
    if strain == 0. {
        return 0.;
    }
    (overshoot.signum() * strain * MAX_PULL_PX * 10.).round() / 10.
}

/// paintResizeLimit's state for a handle under strain: how far past the limit
/// the pointer pushes (signed the way it goes), its place along the handle in
/// window px, and the fade still playing once the drag lets go.
#[derive(Clone)]
pub(crate) struct LimitGlow {
    pub overshoot: f32,
    pub along: f32,
    pub released: Option<Instant>,
}

impl LimitGlow {
    pub(crate) fn new(overshoot: f32, along: f32) -> Self {
        Self {
            overshoot,
            along,
            released: None,
        }
    }
    /// paintResizeLimit while the drag is held: the glow follows the pointer.
    pub(crate) fn paint(&mut self, overshoot: f32, along: f32) {
        self.along = along;
        if limit_strain(overshoot) > 0. {
            self.overshoot = overshoot;
            self.released = None;
        } else if limit_strain(self.overshoot) > 0. {
            self.release();
        } else {
            self.overshoot = 0.;
        }
    }
    /// clearResizeLimit: the glow fades out over the CSS transition's time.
    pub(crate) fn release(&mut self) {
        if self.released.is_none() {
            self.released = Some(Instant::now());
        }
    }
}

/// The `[data-resize-limit]::after` layer: an accent thread centred on the
/// pointer along the handle — a bright hairline fading along its run over a
/// soft halo — leaning a few px the way the pointer pushes. Put it inside the
/// handle; `along` is the pointer's place along it in window px.
pub(crate) fn limit_glow(axis: Axis, glow: LimitGlow, reduce_motion: bool) -> impl IntoElement {
    deferred(
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let mut opacity = limit_strain(glow.overshoot);
                // translate: var(--resize-pull): none under reduced motion.
                let mut pull = if reduce_motion {
                    0.
                } else {
                    limit_pull(glow.overshoot)
                };
                if let Some(released) = glow.released {
                    // prefers-reduced-motion snaps the transition off.
                    if reduce_motion {
                        return;
                    }
                    let fade = released.elapsed().as_secs_f32() / FADE.as_secs_f32();
                    if fade >= 1. {
                        return;
                    }
                    let rest = 1. - crate::reveal::bezier(fade, 0., 0., 0.58, 1.);
                    opacity *= rest;
                    pull *= rest;
                    window.request_animation_frame();
                }
                if opacity <= 0. {
                    return;
                }
                let accent = accent_color();
                let mut core = accent;
                core.a = opacity;
                let mut halo = accent;
                halo.a = 0.45 * opacity;
                let mut clear = accent;
                clear.a = 0.;
                let center = match axis {
                    Axis::X => point(bounds.center().x + px(pull), px(glow.along)),
                    Axis::Y => point(px(glow.along), bounds.center().y + px(pull)),
                };
                // Clip to the ::after box (28px across, inset along the handle)
                // so the 140px thread and 120px halo never bleed past the handle's ends.
                let after = match axis {
                    Axis::X => Bounds::new(
                        point(bounds.center().x - px(14.) + px(pull), bounds.top()),
                        size(px(28.), bounds.size.height),
                    ),
                    Axis::Y => Bounds::new(
                        point(bounds.left(), bounds.center().y - px(14.) + px(pull)),
                        size(bounds.size.width, px(28.)),
                    ),
                };
                window.with_content_mask(Some(ContentMask { bounds: after }), |window| {
                    // The 14px halo gradient as the thread's blurred silhouette.
                    let (hw, hh) = match axis {
                        Axis::X => (1.5, 106.),
                        Axis::Y => (106., 1.5),
                    };
                    window.paint_drop_shadows(
                        Bounds::new(
                            center - point(px(hw / 2.), px(hh / 2.)),
                            size(px(hw), px(hh)),
                        ),
                        Corners::all(px(hw.min(hh) / 2.)),
                        &[BoxShadow {
                            color: halo,
                            offset: point(px(0.), px(0.)),
                            blur_radius: px(7.),
                            spread_radius: px(0.),
                            inset: false,
                        }],
                    );
                    // The core thread, 1.5px across and 140px along, fading at both
                    // ends: two halves for the middle stop a two-stop gradient can't.
                    let (w, h) = match axis {
                        Axis::X => (1.5, 70.),
                        Axis::Y => (70., 1.5),
                    };
                    let angle = match axis {
                        Axis::X => 180.,
                        Axis::Y => 90.,
                    };
                    let behind = match axis {
                        Axis::X => Corners {
                            top_left: px(0.75),
                            top_right: px(0.75),
                            ..Default::default()
                        },
                        Axis::Y => Corners {
                            top_left: px(0.75),
                            bottom_left: px(0.75),
                            ..Default::default()
                        },
                    };
                    let ahead = match axis {
                        Axis::X => Corners {
                            bottom_left: px(0.75),
                            bottom_right: px(0.75),
                            ..Default::default()
                        },
                        Axis::Y => Corners {
                            top_right: px(0.75),
                            bottom_right: px(0.75),
                            ..Default::default()
                        },
                    };
                    let first = Bounds::new(
                        center
                            - match axis {
                                Axis::X => point(px(w / 2.), px(h)),
                                Axis::Y => point(px(w), px(h / 2.)),
                            },
                        size(px(w), px(h)),
                    );
                    let second =
                        Bounds::new(center - point(px(w / 2.), px(h / 2.)), size(px(w), px(h)));
                    window.paint_quad(
                        fill(
                            first,
                            linear_gradient(
                                angle,
                                linear_color_stop(clear, 0.),
                                linear_color_stop(core, 1.),
                            ),
                        )
                        .corner_radii(behind),
                    );
                    window.paint_quad(
                        fill(
                            second,
                            linear_gradient(
                                angle,
                                linear_color_stop(core, 0.),
                                linear_color_stop(clear, 1.),
                            ),
                        )
                        .corner_radii(ahead),
                    );
                });
            },
        )
        .absolute()
        .inset_0(),
    )
    .with_priority(10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strain_stays_dark_inside_the_limits_and_for_rounding_noise() {
        assert_eq!(limit_strain(0.), 0.);
        assert_eq!(limit_strain(0.5), 0.);
        assert_eq!(limit_strain(-0.4), 0.);
        assert_eq!(limit_strain(f32::NAN), 0.);
    }

    #[test]
    fn strain_shows_at_once_at_the_limit_and_grows_toward_1_either_way() {
        let near = limit_strain(2.);
        let far = limit_strain(200.);
        assert!(near > 0.3);
        assert!(far > near);
        assert!(far <= 1.);
        assert_eq!(limit_strain(-200.), far);
    }

    #[test]
    fn pull_leans_the_way_the_pointer_pushes_a_few_px_at_most() {
        assert_eq!(limit_pull(0.), 0.);
        assert!(limit_pull(500.) > 0.);
        assert!(limit_pull(-500.) < 0.);
        assert!(limit_pull(10_000.).abs() <= 4.);
    }

    #[test]
    fn returning_inside_limits_mid_drag_fades_out_last_strain() {
        let mut glow = LimitGlow::new(40., 120.);
        assert!(glow.released.is_none());
        glow.paint(0., 140.);
        assert_eq!(glow.overshoot, 40.);
        assert_eq!(glow.along, 140.);
        assert!(glow.released.is_some());
        glow.paint(-25., 150.);
        assert_eq!(glow.overshoot, -25.);
        assert_eq!(glow.along, 150.);
        assert!(glow.released.is_none());
    }
}
