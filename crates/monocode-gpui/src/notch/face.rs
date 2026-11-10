//! The face the notch wears at rest: two eyes that blink now and then,
//! glance toward the pointer and smile for a moment when an agent finishes.
//! Its arithmetic is `monocode_core::notch::face`; this keeps its clock and
//! paints it. Between blinks the eyes follow the pointer: they look for it a
//! dozen times a second and repaint only when it moved. It asks for frames
//! only during a blink (a fifth of a second every few seconds), and nothing
//! moves under reduced motion.
use super::*;
use monocode_core::notch::face as eyes;

/// The face as painted this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Eyes {
    /// 0 shut, 1 open.
    pub open: f32,
    /// -1 looking left, 1 looking right.
    pub gaze: f32,
    /// Smiling: an agent just finished.
    pub glad: bool,
}

/// The face's clock.
#[derive(Default)]
pub(super) struct Face {
    /// When the blink under way began.
    blink: Option<Instant>,
    /// Where the eyes looked before that blink, and where they look after.
    gaze: (f32, f32),
    glad: bool,
    blinks: Option<Task<()>>,
    /// Looks for the pointer while the face shows.
    follows: Option<Task<()>>,
    cheer: Option<Task<()>>,
}

impl Notch {
    pub(super) fn wears_face(&self, cx: &App) -> bool {
        model::wears_face(&self.look, &self.screen(cx).screen)
    }

    /// Blinks every few seconds while the look asks for the face and this
    /// screen's notch wears one. A camera housing has none, and then nothing
    /// here wakes the interface.
    pub(super) fn start_face(&mut self, cx: &mut Context<Self>) {
        if !self.look.face || !self.wears_face(cx) {
            self.face = Face::default();
            return;
        }
        self.face.blinks = Some(cx.spawn(async move |this, cx| {
            let mut count = 0u32;
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs_f32(eyes::wait(count)))
                    .await;
                count = count.wrapping_add(1);
                if this.update(cx, |this, cx| this.blink(cx)).is_err() {
                    break;
                }
            }
        }));
        // Tests move the eyes themselves; there is no pointer to ask about.
        if cfg!(test) {
            return;
        }
        self.face.follows = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs_f32(eyes::FOLLOW))
                    .await;
                if this.update(cx, |this, cx| this.follow_pointer(cx)).is_err() {
                    break;
                }
            }
        }));
    }

    /// Where the pointer is from the face: -1 far left to 1 far right.
    fn toward_pointer(&self, cx: &App) -> f32 {
        match (platform::pointer_x(), self.screen_island) {
            (Some(pointer), Some([left, _, width, _])) => {
                // The island's width tells the scale: the screen is as many
                // physical pixels wide as the island is, in proportion.
                let shoulder = if self.look.island() { 0. } else { SHOULDER };
                let rest = self
                    .shown
                    .map_or(width, |shown| shown.width + 2. * shoulder)
                    .max(1.);
                let screen = self.screen(cx).screen.width * width / rest;
                eyes::gaze(pointer, left + width / 2., screen)
            }
            _ => 0.,
        }
    }

    /// The eyes turn a step toward the pointer. A still pointer changes
    /// nothing, so nothing is painted; a blink under way moves them itself.
    pub(super) fn follow_pointer(&mut self, cx: &mut Context<Self>) {
        if self.face.blink.is_some()
            || self.shape() != Shape::Quiet
            || cx.reduce_motion()
            || !self.wears_face(cx)
        {
            return;
        }
        let shown = self.face.gaze.1;
        let next = eyes::follow(shown, self.toward_pointer(cx));
        if next != shown {
            self.face.gaze = (next, next);
            cx.notify();
        }
    }

    /// One blink, the eyes turning to the pointer while they are shut. Only
    /// at rest: nothing is painted for a face nobody sees.
    pub(super) fn blink(&mut self, cx: &mut Context<Self>) {
        if self.shape() != Shape::Quiet || cx.reduce_motion() || !self.wears_face(cx) {
            return;
        }
        let toward = self.toward_pointer(cx);
        self.face.gaze = (self.face.gaze.1, toward);
        self.face.blink = Some(Instant::now());
        cx.notify();
    }

    /// The eyes smile for a moment: an agent finished its turn.
    pub(super) fn cheer(&mut self, cx: &mut Context<Self>) {
        if !self.look.face {
            return;
        }
        self.face.glad = true;
        self.face.cheer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs_f32(eyes::CHEER))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.face.glad = false;
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// The face to paint now, while the notch rests and wears one.
    pub(super) fn eyes(&mut self, window: &mut Window, cx: &App) -> Option<Eyes> {
        if !self.wears_face(cx) {
            return None;
        }
        let since = self
            .face
            .blink
            .map(|began| began.elapsed().as_secs_f32())
            .filter(|since| *since < eyes::BLINK && !cx.reduce_motion());
        match since {
            Some(since) => {
                window.request_animation_frame();
                Some(Eyes {
                    open: eyes::openness(since),
                    gaze: eyes::glance(self.face.gaze.0, self.face.gaze.1, since),
                    glad: self.face.glad,
                })
            }
            None => {
                self.face.blink = None;
                Some(Eyes {
                    open: 1.,
                    gaze: self.face.gaze.1,
                    glad: self.face.glad,
                })
            }
        }
    }
}

/// Paints the eyes in the middle of the notch's body: two upright bars in
/// the notch's square language, or two chevrons while they smile.
pub(super) fn paint_eyes(eyes: Eyes, bounds: Bounds<Pixels>, window: &mut Window) {
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let tall = (h * 0.44).clamp(5., 11.);
    let wide = (tall * 0.6).max(3.);
    let apart = tall * 2.2;
    let middle = f32::from(bounds.origin.x) + w / 2. + eyes.gaze * wide * 0.8;
    let level = f32::from(bounds.origin.y) + h / 2.;
    let color = ink(0.9);
    for side in [-1., 1.] {
        let x = middle + side * apart / 2.;
        if eyes.glad {
            let mut smile = gpui::PathBuilder::stroke(px(1.6));
            smile.add_polygon(
                &[
                    point(px(x - wide * 0.8), px(level + tall * 0.25)),
                    point(px(x), px(level - tall * 0.3)),
                    point(px(x + wide * 0.8), px(level + tall * 0.25)),
                ],
                false,
            );
            if let Ok(path) = smile.build() {
                window.paint_path(path, color);
            }
        } else {
            let open = (tall * eyes.open).max(1.5);
            window.paint_quad(
                gpui::fill(
                    Bounds::new(
                        point(px(x - wide / 2.), px(level - open / 2.)),
                        gpui::size(px(wide), px(open)),
                    ),
                    color,
                )
                .corner_radii(px(1.5_f32.min(open / 2.))),
            );
        }
    }
}
