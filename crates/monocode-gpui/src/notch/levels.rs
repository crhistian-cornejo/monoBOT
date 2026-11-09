//! The system's levels in the notch, as IVY shows them: change the volume
//! or the brightness and the wings open for a moment with its mark, a bar
//! and the percent, then fold back. The levels are read off the interface
//! thread (`monocode_core::notch::system`).
use super::*;
use monocode_core::notch::system::Level;

/// How long a level stays after its last change.
const SHOWN: Duration = Duration::from_millis(1600);
/// The level's wings.
pub(super) const LEVEL_WING: u32 = 104;

impl Notch {
    /// Starts reading the system's levels (macOS only, never in tests).
    pub(super) fn watch_levels(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) || !cfg!(target_os = "macos") {
            return;
        }
        let (sender, levels) = async_channel::unbounded();
        if monocode_core::notch::system::watch(move |level| {
            let _ = sender.try_send(level);
        })
        .is_err()
        {
            return;
        }
        self.level_task = Some(cx.spawn(async move |this, cx| {
            while let Ok(level) = levels.recv().await {
                if this
                    .update(cx, |this, cx| this.show_level(level, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    pub(crate) fn show_level(&mut self, level: Level, cx: &mut Context<Self>) {
        self.level = Some(level);
        self.level_at = Some(Instant::now());
        self.keep_responsive();
        self.level_clear = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SHOWN).await;
            let _ = this.update(cx, |this, cx| {
                this.level = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// The edge's light while a level shows: full at the change, fading
    /// out with the level; steady and dim under reduced motion.
    pub(super) fn level_glow(&self, window: &mut Window, cx: &App) -> f32 {
        let (Some(_), Some(at)) = (self.level, self.level_at) else {
            return 0.;
        };
        if cx.reduce_motion() {
            return 0.6;
        }
        let since = at.elapsed().as_secs_f32();
        let glow = if since < 0.15 {
            1.
        } else {
            (1. - (since - 0.15) / (SHOWN.as_secs_f32() - 0.15)).clamp(0., 1.)
        };
        if glow > 0. {
            window.request_animation_frame();
        }
        glow
    }

    /// The level on the wings: its mark on the left, a bar and the percent
    /// on the right (one row where there is no camera).
    pub(super) fn level_view(
        &self,
        level: Level,
        camera: Option<model::Camera>,
        wings: (f32, f32),
    ) -> AnyElement {
        let (share, muted) = match level {
            Level::Volume(volume, muted) => (if muted { 0. } else { volume }, muted),
            Level::Brightness(brightness) => (brightness, false),
        };
        let mark = level_mark(level).into_any_element();
        let bar = div().flex_1().h(px(3.)).bg(ink(0.16)).child(
            div()
                .h_full()
                .w(gpui::relative(share))
                .bg(if muted { ink(0.4) } else { ink(0.92) }),
        );
        let percent = div()
            .w(px(30.))
            .flex_shrink_0()
            .text_size(px(11.))
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(ink(0.85))
            .child(if muted {
                "Off".to_owned()
            } else {
                format!("{}%", (share * 100.).round() as u32)
            });
        match camera {
            Some(camera) => div()
                .debug_selector(|| "notch-level".into())
                .size_full()
                .flex()
                .items_center()
                .child(
                    div()
                        .w(px(wings.0))
                        .flex_shrink_0()
                        .pl(px(14.))
                        .flex()
                        .child(mark),
                )
                .child(div().w(px(camera.width)).flex_shrink_0())
                .child(
                    div()
                        .w(px(wings.1))
                        .flex_shrink_0()
                        .pl(px(10.))
                        .pr(px(12.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(bar)
                        .child(percent),
                )
                .into_any_element(),
            None => div()
                .debug_selector(|| "notch-level".into())
                .size_full()
                .px(px(12.))
                .flex()
                .items_center()
                .gap(px(10.))
                .child(mark)
                .child(bar)
                .child(percent)
                .into_any_element(),
        }
    }
}

/// A speaker with its waves, crossed when muted, or a sun: drawn, so it
/// needs no icon file.
fn level_mark(level: Level) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let (x, y) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
            let at = |dx: f32, dy: f32| point(px(x + dx), px(y + dy));
            let color = ink(0.92);
            match level {
                Level::Volume(volume, muted) => {
                    let mut body = gpui::PathBuilder::fill();
                    body.add_polygon(
                        &[
                            at(1., 5.),
                            at(4., 5.),
                            at(8., 1.5),
                            at(8., 12.5),
                            at(4., 9.),
                            at(1., 9.),
                        ],
                        true,
                    );
                    if let Ok(path) = body.build() {
                        window.paint_path(path, color);
                    }
                    let waves = if muted {
                        0
                    } else {
                        (volume * 3.).ceil().clamp(0., 3.) as usize
                    };
                    for wave in 0..waves {
                        let r = 2.5 + 2.5 * wave as f32;
                        let mut arc = gpui::PathBuilder::stroke(px(1.3));
                        let points: Vec<_> = (0..=6)
                            .map(|step| {
                                let angle = (-50. + 100. * step as f32 / 6.).to_radians();
                                at(9. + r * angle.cos(), 7. + r * angle.sin())
                            })
                            .collect();
                        arc.add_polygon(&points, false);
                        if let Ok(path) = arc.build() {
                            window.paint_path(path, color);
                        }
                    }
                    if muted {
                        for (from, to) in [((10.5, 4.5), (15., 9.5)), ((15., 4.5), (10.5, 9.5))] {
                            let mut cross = gpui::PathBuilder::stroke(px(1.3));
                            cross.add_polygon(&[at(from.0, from.1), at(to.0, to.1)], false);
                            if let Ok(path) = cross.build() {
                                window.paint_path(path, color);
                            }
                        }
                    }
                }
                Level::Brightness(_) => {
                    let mut sun = gpui::PathBuilder::fill();
                    let core: Vec<_> = (0..12)
                        .map(|step| {
                            let angle = (step as f32 * 30.).to_radians();
                            at(7. + 3. * angle.cos(), 7. + 3. * angle.sin())
                        })
                        .collect();
                    sun.add_polygon(&core, true);
                    if let Ok(path) = sun.build() {
                        window.paint_path(path, color);
                    }
                    for ray in 0..8 {
                        let angle = (ray as f32 * 45.).to_radians();
                        let mut line = gpui::PathBuilder::stroke(px(1.3));
                        line.add_polygon(
                            &[
                                at(7. + 4.6 * angle.cos(), 7. + 4.6 * angle.sin()),
                                at(7. + 6.4 * angle.cos(), 7. + 6.4 * angle.sin()),
                            ],
                            false,
                        );
                        if let Ok(path) = line.build() {
                            window.paint_path(path, color);
                        }
                    }
                }
            }
        },
    )
    .size(px(16.))
    .flex_shrink_0()
}
