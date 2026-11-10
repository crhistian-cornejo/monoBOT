//! The illustration above each step of the Poki tutorial (`Scene` in
//! PokiTutorial.tsx). Every scene is drawn from one clock, plays once when its
//! step opens and rests on its final frame; under reduced motion the first
//! frame already is that final one. Everything here is a mock: it names
//! itself "Example" and never reads the real status, which the step prints
//! on its own line below.
use crate::{right_panel::DIFF_ADD, theme::*};
use gpui::{Animation, AnimationExt, AnyElement, Div, FontWeight, div, prelude::*, px, rgb};
use std::time::Duration;

const FRAME_HEIGHT: f32 = 204.;
/// The scene's repaint rate: slow enough to cost little, fast enough for a
/// typed line to read as typing.
const FPS: f32 = 30.;
const TYPE_STEP: f32 = 0.045;
const AFTER_TYPE: f32 = 0.35;
const AFTER_OUT: f32 = 0.45;
const FIRST_LINE: f32 = 0.2;
/// Rest after the last thing appears, so the final frame is seen settling.
const TAIL: f32 = 0.4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Scene {
    Needs,
    Get,
    Cable,
    Developer,
    Start,
    Folder,
    Bot,
}

/// The scene of each tutorial step, in the order of `STEPS`.
const ORDER: [Scene; 7] = [
    Scene::Needs,
    Scene::Get,
    Scene::Cable,
    Scene::Developer,
    Scene::Start,
    Scene::Folder,
    Scene::Bot,
];

pub(super) fn for_step(step: usize) -> Scene {
    ORDER[step.min(ORDER.len() - 1)]
}

enum Line {
    /// A command typed after the prompt.
    Type(&'static str),
    /// What the terminal prints, with a check when it went well.
    Out(&'static str, bool),
}

const GET: &[Line] = &[
    Line::Out("Downloading Poki's published release (about 11 MB)…", false),
    Line::Out("SHA-256 matches the one GitHub lists", true),
    Line::Out("Unpacked inside monoBOT's data folder", true),
    Line::Out("No install scripts run", true),
];
const START: &[Line] = &[
    Line::Type("npm start"),
    Line::Out(
        r#"{"event":"ready","url":"http://127.0.0.1:<port>/mcp"}"#,
        false,
    ),
];

impl Scene {
    /// Seconds from the first frame to the settled one.
    fn length(self) -> f32 {
        match self {
            Scene::Needs => 1.4,
            Scene::Get => schedule(GET).1 + 0.3 + TAIL,
            Scene::Cable => 2.4,
            Scene::Developer => 3.0,
            Scene::Start => schedule(START).1 + 0.3 + TAIL,
            Scene::Folder => 2.6,
            Scene::Bot => 3.1,
        }
    }

    fn draw(self, secs: f32) -> Div {
        match self {
            Scene::Needs => needs(secs),
            Scene::Get => terminal(
                GET,
                "No admin rights, nothing else on your Mac changes",
                secs,
            ),
            Scene::Cable => cable(secs),
            Scene::Developer => developer(secs),
            Scene::Start => terminal(
                START,
                "monoBOT runs this for you while the connector is on",
                secs,
            ),
            Scene::Folder => folder(secs),
            Scene::Bot => bot(secs),
        }
    }
}

/// The scene for `step`, framed and playing. A step that is already open keeps
/// its element id, so redrawing it never restarts the scene.
pub(super) fn view(step: usize) -> AnyElement {
    let scene = for_step(step);
    let length = scene.length();
    div()
        .debug_selector(|| "settings-poki-scene".into())
        .relative()
        .w_full()
        .h(px(FRAME_HEIGHT))
        .flex_shrink_0()
        .overflow_hidden()
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(content(0.1))
        .bg(content(0.03))
        .child(
            div()
                .absolute()
                .top(px(6.))
                .left(px(8.))
                .text_size(px(10.))
                .text_color(content(0.45))
                .child("Example"),
        )
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .justify_center()
                .with_animation(
                    ("poki-scene", step),
                    Animation::new(Duration::from_secs_f32(length)).with_max_fps(FPS),
                    move |root, progress| {
                        root.child(
                            scene
                                .draw(progress * length)
                                .debug_selector(|| "settings-poki-scene-content".into()),
                        )
                    },
                ),
        )
        .into_any_element()
}

/// How far a part that starts at `start` seconds and takes `len` has come,
/// slowing toward the end.
fn stage(secs: f32, start: f32, len: f32) -> f32 {
    let x = ((secs - start) / len).clamp(0., 1.);
    1. - (1. - x).powi(3)
}

/// Up and back down once; zero before `start` and after the end.
fn bump(secs: f32, start: f32, len: f32) -> f32 {
    let x = ((secs - start) / len).clamp(0., 1.);
    1. - (2. * x - 1.).abs()
}

/// The first characters of `text` typed by `secs`, one every `per_char`.
fn typed(text: &str, secs: f32, start: f32, per_char: f32) -> String {
    let count = ((secs - start) / per_char).floor().max(0.) as usize;
    text.chars().take(count).collect()
}

/// When each line of a terminal starts, and when the last one is done.
fn schedule(lines: &[Line]) -> (Vec<f32>, f32) {
    let mut at = FIRST_LINE;
    let mut starts = Vec::with_capacity(lines.len());
    for line in lines {
        starts.push(at);
        at += match line {
            Line::Type(text) => text.chars().count() as f32 * TYPE_STEP + AFTER_TYPE,
            Line::Out(..) => AFTER_OUT,
        };
    }
    (starts, at)
}

/// The four things the first step lists, each ticking in turn.
fn needs(secs: f32) -> Div {
    const ITEMS: [&str; 4] = ["macOS 15+", "Xcode", "Node 24", "iPhone + cable"];
    div()
        .w_full()
        .min_w_0()
        .max_w(px(340.))
        .flex()
        .flex_wrap()
        .justify_center()
        .gap_2()
        .children(ITEMS.iter().enumerate().map(|(at, item)| {
            let start = at as f32 * 0.2;
            div()
                .w(px(160.))
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .border_1()
                .border_color(content(0.1))
                .bg(background_base())
                .text_size(px(12.))
                .opacity(stage(secs, start, 0.3))
                .child(
                    div()
                        .size(px(16.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(accent_color())
                        .opacity(stage(secs, start + 0.25, 0.25))
                        .child(icon("check", 10.).text_color(background_base())),
                )
                .child(*item)
        }))
}

/// A terminal that types its commands, then prints what they print.
fn terminal(lines: &[Line], footer: &'static str, secs: f32) -> Div {
    let (starts, end) = schedule(lines);
    let rows = lines
        .iter()
        .zip(starts)
        .map(|(line, start)| match line {
            Line::Type(text) => div()
                .min_h(px(15.))
                .flex()
                .child(div().text_color(accent_color()).child("$ "))
                .child(typed(text, secs, start, TYPE_STEP)),
            Line::Out(text, ok) => div()
                .min_h(px(15.))
                .flex()
                .items_center()
                .gap_2()
                .text_color(content(0.6))
                .opacity(stage(secs, start, 0.25))
                .child(if *ok {
                    icon("check", 11.)
                        .text_color(accent_color())
                        .into_any_element()
                } else {
                    div().size(px(11.)).flex_shrink_0().into_any_element()
                })
                .child(div().min_w_0().truncate().child(*text)),
        })
        .collect::<Vec<_>>();
    div()
        .w(px(420.))
        .min_w_0()
        .max_w_full()
        .border_1()
        .border_color(content(0.15))
        .bg(background_base())
        .child(
            div()
                .flex()
                .gap(px(6.))
                .px(px(10.))
                .py(px(6.))
                .border_b_1()
                .border_color(content(0.1))
                .children((0..3).map(|_| div().size(px(8.)).rounded_full().bg(content(0.2)))),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .px_3()
                .py_2()
                .font_family("Menlo")
                .text_size(px(11.))
                .children(rows)
                .child(
                    div()
                        .w(px(6.))
                        .h(px(12.))
                        .bg(content(0.6))
                        .opacity(stage(secs, end, 0.05)),
                ),
        )
        .child(
            div()
                .border_t_1()
                .border_color(content(0.1))
                .px_3()
                .py(px(6.))
                .text_size(px(11.))
                .text_color(accent_color())
                .opacity(stage(secs, end, 0.3))
                .child(footer),
        )
}

/// A phone's outline with its notch; `body` fills the screen.
fn phone(width: f32, height: f32, body: Div) -> Div {
    div()
        .w(px(width))
        .h(px(height))
        .flex_shrink_0()
        .overflow_hidden()
        .border_2()
        .border_color(content(0.45))
        .bg(background_base())
        .child(
            div()
                .pt(px(6.))
                .pb(px(8.))
                .flex()
                .justify_center()
                .child(div().w(px(24.)).h(px(5.)).bg(content(0.3))),
        )
        .child(body)
}

/// A Mac and an iPhone joined by a cable, the phone asking to trust it.
fn cable(secs: f32) -> Div {
    const LINK: f32 = 72.;
    let current = stage(secs, 0.1, 0.8);
    let alert = stage(secs, 0.8, 0.3);
    let ring = bump(secs, 1.5, 0.5);
    let trusted = stage(secs, 1.75, 0.25);
    let mac = div()
        .flex()
        .flex_col()
        .items_center()
        .flex_shrink_0()
        .child(
            div()
                .w(px(136.))
                .h(px(88.))
                .border_2()
                .border_color(content(0.4))
                .bg(background_base())
                .p(px(6.))
                .child(
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(content(0.05))
                        .font_family("Menlo")
                        .text_size(px(9.))
                        .text_color(content(0.5))
                        .child("Mac"),
                ),
        )
        .child(div().w(px(156.)).h(px(6.)).bg(content(0.15)));
    let link = div()
        .relative()
        .min_w_0()
        .overflow_hidden()
        .w(px(LINK))
        .h(px(3.))
        .bg(content(0.25))
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .h_full()
                .w(px(LINK * current))
                .bg(accent_color()),
        );
    let prompt = div()
        .mx(px(6.))
        .bg(content(0.1))
        .opacity(alert)
        .flex()
        .flex_col()
        .text_size(px(9.))
        .child(
            div()
                .px_2()
                .py(px(6.))
                .text_center()
                .font_weight(FontWeight::SEMIBOLD)
                .child("Trust This Computer?"),
        )
        .child(div().h(px(1.)).bg(content(0.2)))
        .child(
            div()
                .py(px(4.))
                .flex()
                .justify_center()
                .bg(accent_color().opacity(0.15 * trusted))
                .child(
                    div()
                        .px_3()
                        .py(px(2.))
                        .rounded_full()
                        .border_1()
                        .border_color(accent_color().opacity(ring))
                        .text_color(accent_color())
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Trust"),
                ),
        )
        .child(div().h(px(1.)).bg(content(0.2)))
        .child(
            div()
                .py(px(5.))
                .text_center()
                .text_color(content(0.5))
                .child("Don't Trust"),
        );
    div()
        .w_full()
        .min_w_0()
        .max_w(px(332.))
        .flex()
        .items_center()
        .child(mac)
        .child(link)
        .child(phone(104., 150., div().child(prompt)))
}

/// A phone's settings, with Developer Mode and UI Automation turning on.
fn developer(secs: f32) -> Div {
    let rows = [
        ("Privacy & Security", "Developer Mode", 0.2, 0.7),
        ("Developer", "UI Automation", 1.2, 1.7),
    ];
    phone(
        136.,
        184.,
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(6.))
            .child(
                div()
                    .px(px(4.))
                    .text_size(px(10.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Settings"),
            )
            .children(rows.map(|(group, label, appears, flips)| {
                div()
                    .px(px(8.))
                    .py(px(6.))
                    .bg(content(0.08))
                    .opacity(stage(secs, appears, 0.3))
                    .child(
                        div()
                            .text_size(px(8.))
                            .text_color(content(0.55))
                            .child(group),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .text_size(px(9.5))
                            .child(label)
                            .child(toggle(stage(secs, flips, 0.4))),
                    )
            }))
            .child(
                div()
                    .px(px(4.))
                    .text_size(px(8.))
                    .text_color(content(0.55))
                    .opacity(stage(secs, 2.4, 0.3))
                    .child("Restart when asked."),
            ),
    )
}

/// A switch `on` of the way from off to on (0 to 1).
fn toggle(on: f32) -> Div {
    div()
        .relative()
        .flex_shrink_0()
        .w(px(24.))
        .h(px(14.))
        .rounded_full()
        .bg(content(0.2))
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded_full()
                .bg(accent_color().opacity(on)),
        )
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(2. + 10. * on))
                .size(px(10.))
                .rounded_full()
                .bg(background_base()),
        )
}

/// monoBOT's own setting being filled in, its status turning to Ready. The
/// path and the status are examples; the step prints the real status itself.
fn folder(secs: f32) -> Div {
    const PATH: &str = "/Users/you/poki";
    let ready = stage(secs, 2.0, 0.25);
    let green: gpui::Hsla = rgb(DIFF_ADD).into();
    div()
        .w(px(380.))
        .min_w_0()
        .max_w_full()
        .flex()
        .flex_col()
        .gap(px(10.))
        .p_3()
        .border_1()
        .border_color(content(0.1))
        .bg(background_base())
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .child("iPhone (Poki)"),
                )
                .child(toggle(stage(secs, 1.3, 0.4))),
        )
        .child(
            div()
                .h(px(32.))
                .flex()
                .items_center()
                .px(px(10.))
                .border_1()
                .border_color(content(0.1))
                .font_family("Menlo")
                .text_size(px(12.))
                .child(typed(PATH, secs, 0.2, 0.05)),
        )
        .child(
            div()
                .relative()
                .h(px(16.))
                .text_size(px(12.))
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_color(content(0.55))
                        .opacity(1. - ready)
                        .child(div().size(px(8.)).rounded_full().bg(content(0.3)))
                        .child("Checking…"),
                )
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .gap_2()
                        .opacity(ready)
                        .child(div().size(px(8.)).rounded_full().bg(green))
                        .child("Ready (example)"),
                ),
        )
}

/// The Phone bot's chat: it reads, asks before a tap, then reads again.
fn bot(secs: f32) -> Div {
    let press = bump(secs, 1.9, 0.25);
    let bubble = |text: &'static str, start: f32| {
        div()
            .max_w(px(300.))
            .px(px(10.))
            .py(px(6.))
            .bg(content(0.08))
            .opacity(stage(secs, start, 0.3))
            .child(text)
    };
    div()
        .w(px(380.))
        .min_w_0()
        .max_w_full()
        .flex()
        .flex_col()
        .items_start()
        .gap_2()
        .text_size(px(11.))
        .child(bubble("Reading the screen: Settings is open.", 0.15))
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_2()
                .p(px(10.))
                .border_1()
                .border_color(content(0.1))
                .bg(background_base())
                .opacity(stage(secs, 0.9, 0.3))
                .child(
                    div()
                        .flex()
                        .gap(px(4.))
                        .child("Phone wants to")
                        .child(div().font_weight(FontWeight::MEDIUM).child("tap “Wi-Fi”")),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            div()
                                .px_2()
                                .py(px(2.))
                                .text_color(content(0.6))
                                .child("Deny"),
                        )
                        .child(
                            div()
                                .px_2()
                                .py(px(2.))
                                .font_weight(FontWeight::MEDIUM)
                                .bg(accent_color())
                                .text_color(background_base())
                                .opacity(1. - 0.3 * press)
                                .child("Allow"),
                        ),
                ),
        )
        .child(bubble("Wi-Fi is on and connected to Home.", 2.6))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Context, Render, TestAppContext, VisualTestContext, Window, size};

    const SCENES: [Scene; 7] = ORDER;

    #[test]
    fn each_of_the_seven_steps_has_its_own_scene() {
        let scenes = (0..7).map(for_step).collect::<Vec<_>>();
        for (at, scene) in scenes.iter().enumerate() {
            assert_eq!(scenes.iter().filter(|other| *other == scene).count(), 1);
            assert_eq!(*scene, SCENES[at]);
        }
        assert_eq!(
            [scenes[0], scenes[2], scenes[5], scenes[6]],
            [Scene::Needs, Scene::Cable, Scene::Folder, Scene::Bot]
        );
    }

    #[test]
    fn a_step_past_the_last_keeps_the_last_scene() {
        assert_eq!(for_step(6), Scene::Bot);
        assert_eq!(for_step(99), Scene::Bot);
    }

    #[test]
    fn every_scene_is_short_and_settles_on_its_final_frame() {
        for scene in SCENES {
            let length = scene.length();
            assert!(
                (1. ..=4.).contains(&length),
                "{scene:?} runs {length}s, a tutorial illustration stays brief"
            );
            // The animation ends at progress 1, and reduced motion draws it
            // from the start: both must be fully shown with nothing in flight.
            assert_eq!(stage(length, 0., 1.), 1.);
            assert!(bump(length, length - 0.3, 0.3) < 1e-3);
        }
    }

    #[test]
    fn stages_start_hidden_and_slow_toward_their_end() {
        assert_eq!(stage(0., 0.5, 0.4), 0.);
        assert_eq!(stage(1., 0.5, 0.4), 1.);
        let (early, late) = (stage(0.6, 0.5, 0.4), stage(0.8, 0.5, 0.4));
        assert!(early > 0.25, "ease-out leads quickly");
        assert!(late - early < early, "and slows down");
        assert_eq!(bump(0.25, 0., 0.5), 1.);
        assert_eq!(bump(0.5, 0., 0.5), 0.);
    }

    #[test]
    fn typing_adds_one_character_at_a_time_and_ends_whole() {
        assert_eq!(typed("npm start", 0., 0.2, 0.05), "");
        assert_eq!(typed("npm start", 0.31, 0.2, 0.05), "np");
        assert_eq!(typed("npm start", 9., 0.2, 0.05), "npm start");
    }

    #[test]
    fn terminal_lines_run_in_order_before_the_scene_ends() {
        for (lines, scene) in [(GET, Scene::Get), (START, Scene::Start)] {
            let (starts, end) = schedule(lines);
            assert_eq!(starts.len(), lines.len());
            assert!(starts.windows(2).all(|pair| pair[0] < pair[1]));
            assert!(end < scene.length());
        }
        let (starts, _) = schedule(START);
        assert!(
            starts[1] >= starts[0] + "npm start".len() as f32 * TYPE_STEP,
            "the output waits for the command to finish typing"
        );
    }

    struct Host(usize);
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            view(self.0)
        }
    }

    #[gpui::test]
    fn every_scene_draws_with_and_without_reduced_motion(cx: &mut TestAppContext) {
        for reduced in [true, false] {
            cx.update(|cx| cx.set_reduce_motion(reduced));
            for width in [320., 640.] {
                for step in 0..7 {
                    let window = cx.open_window(size(px(width), px(260.)), move |_, _| Host(step));
                    let visual = &mut VisualTestContext::from_window(window.into(), cx);
                    visual.run_until_parked();
                    // Reduced motion draws the settled frame immediately;
                    // normal motion exercises the real animated entry point.
                    let frame = visual.debug_bounds("settings-poki-scene").unwrap();
                    let content = visual.debug_bounds("settings-poki-scene-content").unwrap();
                    assert!(
                        content.left() >= frame.left()
                            && content.right() <= frame.right()
                            && content.top() >= frame.top()
                            && content.bottom() <= frame.bottom(),
                        "step {step}, width {width}, reduced {reduced}: {content:?} must fit {frame:?}"
                    );
                }
            }
        }
    }
}
