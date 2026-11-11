//! The face of a Mono. A coding Mono wears its pixel mascot (PixelMascot.tsx
//! / pixelMascots.ts rendered as native quads); a personal bot wears its 3D
//! persona (`monocode_core::mono::persona`), rendered once per pose off the
//! interface thread and painted from then on as a picture.
use gpui::{
    AnyElement, App, Bounds, Corners, Hsla, Pixels, RenderImage, Window, canvas, point, prelude::*,
    px, rgb, rgba, size,
};
use monocode_core::mono::persona::{self, Look, Pose};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};
#[derive(Deserialize)]
struct Sprite {
    name: String,
    rest: Vec<String>,
    talk: Vec<String>,
    eyes: [Vec<[u32; 2]>; 2],
}
static SPRITES: LazyLock<Vec<Sprite>> =
    LazyLock::new(|| serde_json::from_str(include_str!("sprites.json")).unwrap());
fn tone(mut c: Hsla, t: f32) -> Hsla {
    c.l = if t >= 0. {
        c.l + (1. - c.l) * t
    } else {
        c.l * (1. + t)
    };
    c
}
fn material(name: &str, x: usize, y: usize, base: Hsla) -> Hsla {
    match name {
        "ghost" if y >= 6 => tone(base, -0.08),
        "robot" => match y {
            0 => rgb(0xf0d9b1).into(),
            2 | 3 => tone(base, 0.26),
            4 => tone(base, -0.24),
            _ => base,
        },
        "cat" if (x == 2 || x == 5) && y == 1 => rgb(0xd78f8b).into(),
        "cat" if y <= 1 => tone(base, -0.03),
        "cat" if y == 5 || y == 6 => tone(base, 0.07),
        "crab" if y <= 1 => tone(base, 0.05),
        "mushroom" if y >= 5 => rgb(0xf1e3be).into(),
        "rocket" if y >= 6 => rgb(0xf9c081).into(),
        "rocket" if y == 5 => tone(base, -0.25),
        "dino" if y == 2 && x >= 5 => rgb(0xece0ae).into(),
        "frog" if y == 4 => tone(rgb(0xf5e7ad).into(), -0.03),
        _ => base,
    }
}
/// The badge a Mono that waits on the user wears at its top right corner.
fn needs_you_badge(bounds: Bounds<Pixels>, window: &mut Window) {
    let unit = bounds.size.width / 16.;
    for (x, y, w, h, color) in [
        (12.5, -2.5, 3., 3., 0xffd24a),
        (13.25, 0.5, 1.5, 1., 0x8c8680),
    ] {
        window.paint_quad(gpui::fill(
            Bounds::new(
                bounds.origin + point(unit * x, unit * y),
                size(unit * w, unit * h),
            ),
            rgb(color),
        ));
    }
}

type FrameKey = (Look, Pose, u32);

/// Persona pictures already rendered, and the ones on their way.
#[derive(Default)]
struct Frames {
    ready: HashMap<FrameKey, Arc<RenderImage>>,
    rendering: HashSet<FrameKey>,
}

static FRAMES: LazyLock<Mutex<Frames>> = LazyLock::new(Default::default);

/// Pictures kept before the cache starts over: every bot at every size the
/// app draws, in each pose, is a few hundred.
const KEPT_FRAMES: usize = 600;

/// The persona's picture in `pose` at `pixels` a side. One not rendered yet
/// starts on a background thread and the windows redraw when it lands;
/// meanwhile its resting picture stands in, or nothing the first time.
fn frame(look: Look, pose: Pose, pixels: u32, cx: &mut App) -> Option<Arc<RenderImage>> {
    let key = (look, pose, pixels);
    let mut frames = FRAMES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(image) = frames.ready.get(&key) {
        return Some(image.clone());
    }
    if frames.rendering.insert(key) {
        let rendered = cx.background_executor().spawn(async move {
            let mut bgra = persona::render(look, pose, pixels);
            for pixel in bgra.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
            image::RgbaImage::from_raw(pixels, pixels, bgra)
        });
        cx.spawn(async move |cx| {
            let image = rendered.await;
            let mut frames = FRAMES.lock().unwrap_or_else(|e| e.into_inner());
            frames.rendering.remove(&key);
            if let Some(image) = image {
                if frames.ready.len() >= KEPT_FRAMES {
                    frames.ready.clear();
                }
                let image = RenderImage::new(vec![image::Frame::new(image)]);
                frames.ready.insert(key, Arc::new(image));
            }
            drop(frames);
            cx.update(|cx| cx.refresh_windows());
        })
        .detach();
    }
    frames.ready.get(&(look, Pose::Rest, pixels)).cloned()
}

/// A personal bot's 3D persona. It breathes at rest and blinks on its own
/// beat. While it works it hops and, three times a second, trades its
/// resting picture for the one with its mouth open and its hands up, as the
/// pixel mascots trade frames. Under reduced motion it stands still.
fn persona_avatar(look: Look, status: &str, side: f32, still: bool) -> AnyElement {
    let status = status.to_string();
    canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            let working = status == "working";
            let moves = working || (!still && status != "needs-you");
            // A hop reads at 30 frames a second; a breath of about one
            // percent and a blink at 15, so a page of resting faces no
            // longer holds the window at 60.
            let rate = if working {
                crate::pulse::Rate::Steady
            } else {
                crate::pulse::Rate::Low
            };
            let animate = moves && crate::pulse::request(rate, window, cx);
            let t = crate::pulse::elapsed(cx).as_secs_f32();
            let beat = (look.color % 89) as f32 * 0.21;
            let pose = if animate && working && (t * 3.).floor() as u64 % 2 == 1 {
                Pose::Talk
            } else if animate && (t + beat) % 5.2 > 5.05 {
                Pose::Blink
            } else {
                Pose::Rest
            };
            // How far it is off the ground and how much taller it stretches,
            // as parts of its height: a hop while working, a breath at rest.
            let (rise, stretch) = match (animate, working) {
                (false, _) => (0., 0.),
                (true, true) => {
                    let hop = (t * 9.).sin().abs();
                    (hop * 0.08, hop * 0.06)
                }
                (true, false) => {
                    let breath = (t * 2.5).sin();
                    (breath * 0.012, breath * 0.012)
                }
            };
            let width = bounds.size.width * (1. - stretch * 0.6);
            let height = bounds.size.height * (1. + stretch);
            let drawn = Bounds::new(
                point(
                    bounds.origin.x + (bounds.size.width - width) / 2.,
                    bounds.origin.y + bounds.size.height * (1. - rise) - height,
                ),
                size(width, height),
            );
            let pixels = (side * window.scale_factor()).round() as u32;
            let pixels = pixels.clamp(8, persona::MAX_SIDE);
            match frame(look, pose, pixels, cx) {
                Some(image) => {
                    let _ = window.paint_image(drawn, drawn, Corners::default(), image, 0, false);
                }
                None => {
                    // Its colour as a soft dot until the first picture lands.
                    let dot = bounds.size.width * 0.56;
                    let mut color: Hsla = rgb(look.color).into();
                    color.a = 0.5;
                    let origin = bounds.origin
                        + point(
                            (bounds.size.width - dot) / 2.,
                            (bounds.size.height - dot) / 2.,
                        );
                    window.paint_quad(
                        gpui::fill(Bounds::new(origin, size(dot, dot)), color)
                            .corner_radii(dot / 2.),
                    );
                }
            }
            if status == "needs-you" {
                needs_you_badge(bounds, window);
            }
        },
    )
    .size(px(side))
    .flex_shrink_0()
    .into_any_element()
}

/// `name` is a pixel mascot's name or a bot's persona (`Mono::face`).
pub(crate) fn avatar(name: &str, color: u32, status: &str, side: f32, still: bool) -> AnyElement {
    if let Some((species, outfit)) = persona::parse(name) {
        let look = Look {
            species,
            outfit,
            color: color & 0xffffff,
        };
        return persona_avatar(look, status, side, still);
    }
    let index = SPRITES.iter().position(|m| m.name == name).unwrap_or(1);
    let status = status.to_string();
    canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            // "needs-you" holds still, so it must not keep the clock running.
            let working = status == "working";
            let moves = working || (!still && status != "needs-you");
            // Its poses trade a few times a second, but the hop and the bob
            // between them move every frame.
            // A hop reads at 30 frames a second; a breath of about one
            // percent and a blink at 15, so a page of resting faces no
            // longer holds the window at 60.
            let rate = if working {
                crate::pulse::Rate::Steady
            } else {
                crate::pulse::Rate::Low
            };
            let animate = moves && crate::pulse::request(rate, window, cx);
            let t = crate::pulse::elapsed(cx).as_secs_f32();
            let talk = animate && status == "working" && (t * 3.).floor() as u64 % 2 == 1;
            let sprite = &SPRITES[index];
            let rows = if talk { &sprite.talk } else { &sprite.rest };
            let unit = bounds.size.width / 16.;
            let hop = if animate {
                if status == "working" {
                    -(t * 9.).sin().abs() * 0.9
                } else if status == "needs-you" {
                    0.
                } else {
                    (t * 2.5).sin() * 0.25
                }
            } else {
                0.
            };
            let draw = |x: f32, y: f32, w: f32, h: f32, color: Hsla, window: &mut gpui::Window| {
                window.paint_quad(gpui::fill(
                    gpui::Bounds::new(
                        bounds.origin + gpui::point(unit * x, unit * (y + hop)),
                        gpui::size(unit * w, unit * h),
                    ),
                    color,
                ));
            };
            let mut base: Hsla = rgb(color).into();
            base.s *= 0.55;
            base.l += (0.82 - base.l) * 0.55;
            let filled = |x: isize, y: isize| {
                x >= 0
                    && y >= 0
                    && rows
                        .get(y as usize)
                        .and_then(|s| s.as_bytes().get(x as usize))
                        == Some(&b'#')
            };
            for depth in [true, false] {
                for (y, row) in rows.iter().enumerate() {
                    for (x, pixel) in row.bytes().enumerate() {
                        if pixel != b'#' {
                            continue;
                        }
                        let face = tone(
                            material(&sprite.name, x, y, base),
                            0.04 + (7. - y as f32) * 0.009 + (3. - x as f32) * 0.006,
                        );
                        let offset = if depth { 0.14 } else { 0. };
                        draw(
                            2. + (x as f32 + offset) * 1.5,
                            2. + (y as f32 + offset * 0.66) * 1.5,
                            1.5,
                            1.5,
                            if depth { tone(face, -0.32) } else { face },
                            window,
                        );
                        if !depth {
                            if !filled(x as isize, y as isize - 1) {
                                draw(
                                    2. + x as f32 * 1.5,
                                    2. + y as f32 * 1.5,
                                    1.5,
                                    0.12,
                                    rgba(0xffffed3d).into(),
                                    window,
                                );
                            }
                            if !filled(x as isize - 1, y as isize) {
                                draw(
                                    2. + x as f32 * 1.5,
                                    2. + y as f32 * 1.5,
                                    0.09,
                                    1.5,
                                    rgba(0xffffed21).into(),
                                    window,
                                );
                            }
                        }
                    }
                }
            }
            for &[x, y] in &sprite.eyes[usize::from(talk)] {
                let x = 2. + x as f32 * 1.5;
                let y = 2. + y as f32 * 1.5;
                let blink =
                    animate && status != "needs-you" && (t + index as f32 * 0.37) % 5.2 > 5.05;
                if blink {
                    draw(x, y + 0.825, 1.5, 0.375, rgb(0x263331).into(), window);
                } else {
                    draw(
                        x,
                        y,
                        1.5,
                        1.5,
                        rgb(if sprite.name == "rocket" {
                            0x243e51
                        } else {
                            0x263331
                        })
                        .into(),
                        window,
                    );
                    draw(
                        x + 0.225,
                        y + 0.225,
                        0.375,
                        0.375,
                        rgba(0xfffff4bf).into(),
                        window,
                    );
                }
            }
            if sprite.name == "mushroom" {
                for (x, y, w, h) in [
                    (3., 0.42, 0.6, 0.35),
                    (1.65, 1.4, 0.5, 0.38),
                    (5.5, 1.7, 0.35, 0.4),
                ] {
                    draw(
                        2. + x * 1.5,
                        2. + (y + if talk { 1. } else { 0. }) * 1.5,
                        w * 1.5,
                        h * 1.5,
                        rgba(0xfff8e2a8).into(),
                        window,
                    );
                }
            }
            if status == "needs-you" {
                needs_you_badge(bounds, window);
            }
        },
    )
    .size(px(side))
    .flex_shrink_0()
    .into_any_element()
}

/// A member's look for an avatar stack (mascot and color).
#[derive(Clone, Debug)]
pub(crate) struct StackAvatar {
    pub mascot: String,
    pub color: u32,
}

/// A group's members as overlapping round avatars, the lead first, with the
/// rest counted at the end: who is in the room, at a glance (matching
/// GroupAvatarStack.tsx from Tauri).
pub(crate) fn group_avatar_stack(
    members: &[StackAvatar],
    status: &str,
    more: usize,
    side: f32,
    show_status: bool,
) -> AnyElement {
    if members.is_empty() {
        return avatar("robot", 0xffffff, status, side, true);
    }
    let circle_size = px(side);
    let inner_size = (side * 0.72).round();
    let overlap = (side * 0.32).round();
    let bg_base = crate::theme::background_base();
    let lead_color = members.first().map_or(0xffffff, |m| m.color);

    let count = members.len() + if more > 0 { 1 } else { 0 };
    let total_width = if count <= 1 {
        side
    } else {
        side + (count - 1) as f32 * (side - overlap)
    };

    let mut stack = gpui::div()
        .id("group-avatar-stack")
        .debug_selector(|| "group-avatar-stack".into())
        .relative()
        .flex()
        .items_center()
        .flex_shrink_0()
        .w(px(total_width))
        .h(circle_size);

    for (ix, member) in members.iter().enumerate() {
        stack = stack.child(
            gpui::div()
                .size(circle_size)
                .flex_shrink_0()
                .rounded_full()
                .border_1()
                .border_color(bg_base)
                .bg(bg_base)
                .flex()
                .items_center()
                .justify_center()
                .when(ix > 0, |d| d.ml(px(-overlap)))
                .child(
                    gpui::div()
                        .size_full()
                        .rounded_full()
                        .bg(crate::theme::content(0.07))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(avatar(
                            &member.mascot,
                            member.color,
                            "idle",
                            inner_size,
                            true,
                        )),
                ),
        );
    }

    if more > 0 {
        let font_size = (side * 0.5).max(8.);
        stack = stack.child(
            gpui::div()
                .size(circle_size)
                .flex_shrink_0()
                .rounded_full()
                .border_1()
                .border_color(bg_base)
                .bg(bg_base)
                .flex()
                .items_center()
                .justify_center()
                .when(!members.is_empty(), |d| d.ml(px(-overlap)))
                .child(
                    gpui::div()
                        .size_full()
                        .rounded_full()
                        .bg(crate::theme::content(0.07))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            gpui::div()
                                .text_size(px(font_size))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(crate::theme::content(0.7))
                                .child(format!("+{more}")),
                        ),
                ),
        );
    }

    if show_status {
        let status_color = if status == "working" {
            rgb(lead_color).into()
        } else if status == "needs-you" {
            crate::theme::accent_color()
        } else {
            crate::theme::content(0.3)
        };
        stack = stack.child(
            gpui::div()
                .absolute()
                .right(px(-2.))
                .top(px(-2.))
                .size(px(6.))
                .rounded_full()
                .border_1()
                .border_color(bg_base)
                .bg(status_color),
        );
    }

    stack.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext};

    fn rendered(color: u32) -> Vec<(Pose, Arc<RenderImage>)> {
        let frames = FRAMES.lock().unwrap_or_else(|e| e.into_inner());
        frames
            .ready
            .iter()
            .filter(|((look, ..), _)| look.color == color)
            .map(|((_, pose, _), image)| (*pose, image.clone()))
            .collect()
    }

    fn draw(face: &str, color: u32, window: &mut VisualTestContext) {
        window.draw(point(px(0.), px(0.)), size(px(40.), px(40.)), |_, _| {
            avatar(face, color, "idle", 32., true)
        });
    }

    #[gpui::test]
    fn a_bot_wears_its_persona_once_it_is_rendered(cx: &mut TestAppContext) {
        // Colours no other test draws, so the shared cache tells them apart.
        let (bot, coder) = (0x12ab34, 0x12ab35);
        let window = cx.add_empty_window();
        let face = persona::key("cat", Some("writer"));
        draw(&face, bot, window);
        // The first paint only asks for the picture.
        window.run_until_parked();
        let frames = rendered(bot);
        assert_eq!(frames.len(), 1);
        let (pose, image) = &frames[0];
        assert_eq!(*pose, Pose::Rest);
        draw(&face, bot, window);
        assert!(window.update(|window, _| window.has_image_atlas_entry(image)));
        // A coding Mono stays pixel art: nothing is rendered for it.
        draw("cat", coder, window);
        window.run_until_parked();
        assert!(rendered(coder).is_empty());
    }
}
