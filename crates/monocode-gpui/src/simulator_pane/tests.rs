use super::*;
use gpui::{
    Bounds, Modifiers, TestAppContext, VisualTestContext, WindowBounds, WindowHandle,
    WindowOptions, point, size,
};
use std::{cell::RefCell, rc::Rc};

const UDID: &str = "00000000-0000-0000-0000-00000000F00D";
const TREE: &str = "App \"Example\" at (201, 437) size 402×874
  Button \"Sign in\" id=login at (201, 412) size 120×44
    StaticText \"Sign in\" at (201, 412) size 60×20";

/// A pane showing a fake device through the runner's frames, focused,
/// with the device input it sends caught instead of sent.
fn pane(
    cx: &mut TestAppContext,
) -> (
    WindowHandle<SimulatorPane>,
    VisualTestContext,
    async_channel::Receiver<(String, Input)>,
) {
    let (sender, sent) = async_channel::unbounded();
    let window = cx.update(|cx| {
        crate::input::bind(cx);
        bind(cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(500.), px(1000.)),
                ))),
                ..Default::default()
            },
            |window, cx| {
                let pane = cx.new(|cx| {
                    let mut pane = SimulatorPane::build(cx);
                    pane.inputs = sender;
                    pane.device = Some(Device::booted_named(UDID, "iPhone 18 Pro", "iOS 27.0"));
                    pane.source = Source::Frames(None);
                    pane.status = Status::Live;
                    pane.points = (402., 874.);
                    pane.frame = Some(bgra_image(image::RgbaImage::new(4, 8)));
                    pane
                });
                window.focus(&pane.focus_handle(cx), cx);
                pane
            },
        )
        .unwrap()
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    visual.refresh().unwrap();
    (window, visual, sent)
}

fn click(visual: &mut VisualTestContext, selector: &'static str) {
    visual.refresh().unwrap();
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not on screen"));
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
    visual.refresh().unwrap();
}

#[gpui::test]
fn a_selected_simulator_can_be_retried_after_a_boot_failure(cx: &mut TestAppContext) {
    const OTHER: &str = "00000000-0000-0000-0000-00000000F00E";
    let (window, mut visual, _) = pane(cx);
    window
        .update(&mut visual, |pane, _, cx| {
            pane.frame = None;
            pane.device = None;
            pane.status = Status::NoDevice;
            pane.available_devices = [
                Device::booted_named(UDID, "iPhone 18 Pro", "iOS 27.0"),
                Device::booted_named(OTHER, "iPhone 17", "iOS 26.0"),
            ]
            .into_iter()
            .map(|mut device| {
                device.state = "Shutdown".into();
                device
            })
            .collect();
            pane.boot_simulator = |target, open| {
                assert_eq!(target, Some(OTHER));
                assert!(!open);
                Err("Could not boot the device.".into())
            };
            cx.notify();
        })
        .unwrap();
    click(&mut visual, "simulator-device-1");
    click(&mut visual, "simulator-boot");
    window
        .update(&mut visual, |pane, _, _| {
            assert_eq!(pane.boot_target(), Some(OTHER));
            assert_eq!(
                pane.status,
                Status::BootFailed("Could not boot the device.".into())
            );
            pane.boot_simulator = |target, open| {
                assert_eq!(target, Some(OTHER));
                assert!(!open);
                Ok(Device::booted_named(OTHER, "iPhone 17", "iOS 26.0"))
            };
        })
        .unwrap();
    click(&mut visual, "simulator-boot");
    window
        .update(&mut visual, |pane, _, _| {
            assert_eq!(pane.status, Status::Looking);
            assert_eq!(pane.preferred_device.as_deref(), Some(OTHER));
        })
        .unwrap();
}

/// Where the device point `at` is drawn.
fn on_screen(
    window: WindowHandle<SimulatorPane>,
    visual: &mut VisualTestContext,
    at: (f32, f32),
) -> Point<Pixels> {
    window
        .update(visual, |pane, _, _| {
            let (points, orientation) = pane.view();
            let rect =
                geometry::screen_rect(pane.screen.get().unwrap(), points, orientation, pane.zoom)
                    .unwrap();
            geometry::view_point(at, rect, points, orientation)
        })
        .unwrap()
}

#[gpui::test]
fn the_bezel_shortcuts_and_buttons_switch_the_zoom(cx: &mut TestAppContext) {
    let (window, mut visual, _) = pane(cx);
    let zoom =
        |visual: &mut VisualTestContext| window.update(visual, |pane, _, _| pane.zoom).unwrap();
    assert_eq!(zoom(&mut visual), Zoom::Fit);
    visual.simulate_keystrokes("cmd-1");
    assert_eq!(zoom(&mut visual), Zoom::Actual);
    visual.simulate_keystrokes("cmd-0");
    assert_eq!(zoom(&mut visual), Zoom::Fit);
    click(&mut visual, "simulator-actual");
    assert_eq!(zoom(&mut visual), Zoom::Actual);
    click(&mut visual, "simulator-fit");
    assert_eq!(zoom(&mut visual), Zoom::Fit);
}

#[gpui::test]
fn home_by_shortcut_and_button_and_typing_reach_the_device(cx: &mut TestAppContext) {
    let (_window, mut visual, sent) = pane(cx);
    visual.simulate_keystrokes("cmd-shift-h");
    assert_eq!(sent.try_recv().unwrap(), (UDID.to_owned(), Input::Home));
    click(&mut visual, "simulator-home");
    assert_eq!(sent.try_recv().unwrap().1, Input::Home);
    // A click on the screen moved the focus back to the pane.
    visual.simulate_keystrokes("a enter");
    assert_eq!(sent.try_recv().unwrap().1, Input::Text("a".into()));
    assert_eq!(sent.try_recv().unwrap().1, Input::Key("return"));
    // ⌘ shortcuts never type into the device.
    visual.simulate_keystrokes("cmd-0");
    assert!(sent.try_recv().is_err());
}

#[gpui::test]
fn a_click_taps_the_point_under_it_and_a_drag_swipes(cx: &mut TestAppContext) {
    let (window, mut visual, sent) = pane(cx);
    let centre = on_screen(window, &mut visual, (201., 437.));
    visual.simulate_click(centre, Modifiers::default());
    visual.run_until_parked();
    assert_eq!(sent.try_recv().unwrap().1, Input::Tap { x: 201., y: 437. });
    let from = on_screen(window, &mut visual, (200., 700.));
    let to = on_screen(window, &mut visual, (200., 200.));
    visual.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
    visual.run_until_parked();
    assert_eq!(
        sent.try_recv().unwrap().1,
        Input::Swipe {
            from: (200., 700.),
            to: (200., 200.)
        }
    );
    // In the letterbox beside the screen nothing is sent.
    visual.simulate_click(point(px(2.), px(2.)), Modifiers::default());
    visual.run_until_parked();
    assert!(sent.try_recv().is_err());
}

#[gpui::test]
fn the_inspector_picks_steps_through_the_stack_and_adds_to_chat(cx: &mut TestAppContext) {
    let (window, mut visual, sent) = pane(cx);
    let added = Rc::new(RefCell::new(Vec::new()));
    let seen = added.clone();
    let _events = window
        .update(&mut visual, |_, _, cx| {
            cx.subscribe(&cx.entity(), move |_, _, event: &SimulatorEvent, _| {
                let SimulatorEvent::AddToChat { text, image } = event;
                seen.borrow_mut().push((text.clone(), image.clone()));
            })
        })
        .unwrap();
    window
        .update(&mut visual, |pane, _, cx| {
            pane.drawer = Drawer::Inspector;
            let mut portrait = image::RgbaImage::new(402, 874);
            portrait.put_pixel(201, 412, image::Rgba([1, 2, 3, 255]));
            pane.inspector = Some(Inspector {
                elements: inspect::parse(TREE),
                stack: Vec::new(),
                index: 0,
                space: geometry::Space::Portrait,
                still: Some(Still {
                    shown: bgra_image(portrait.clone()),
                    upright: Arc::new(portrait.clone()),
                    portrait: Arc::new(portrait),
                    scale: 1.,
                }),
                note: cx
                    .new(|cx| crate::input::Input::field("Note", "Add a note…", content(0.4), cx)),
                loading: false,
                error: None,
            });
            cx.notify();
        })
        .unwrap();
    visual.run_until_parked();
    visual.refresh().unwrap();
    // A click picks instead of tapping: the text, on the button, in the app.
    let text = on_screen(window, &mut visual, (201., 412.));
    visual.simulate_click(text, Modifiers::default());
    visual.run_until_parked();
    assert!(sent.try_recv().is_err(), "inspecting does not tap");
    let chosen = |visual: &mut VisualTestContext| {
        window
            .update(visual, |pane, _, _| {
                let inspector = pane.inspector.as_ref().unwrap();
                (
                    inspector.chosen().map(|e| e.kind.clone()),
                    inspector.stack.len(),
                )
            })
            .unwrap()
    };
    assert_eq!(chosen(&mut visual), (Some("StaticText".into()), 3));
    click(&mut visual, "simulator-behind");
    assert_eq!(chosen(&mut visual).0.as_deref(), Some("Button"));
    click(&mut visual, "simulator-front");
    assert_eq!(chosen(&mut visual).0.as_deref(), Some("StaticText"));
    click(&mut visual, "simulator-behind");
    window
        .update(&mut visual, |pane, _, cx| {
            let note = pane.inspector.as_ref().unwrap().note.clone();
            note.update(cx, |input, cx| input.set_text("Too small".into(), cx));
        })
        .unwrap();
    click(&mut visual, "simulator-add-to-chat");
    let added = added.borrow();
    assert_eq!(added.len(), 1);
    let (text, image) = &added[0];
    assert_eq!(
        text,
        "Simulator element on iPhone 18 Pro: Button \"Sign in\" id=login — frame x 141, y 390, 120×44 pt.\nNote: Too small"
    );
    let crop = image::open(image).unwrap().to_rgba8();
    assert_eq!(crop.dimensions(), (136, 60));
    // RGBA now: the BGRA pixel at (201, 412) is 8 pt into the margin.
    assert_eq!(
        crop.get_pixel(201 - 141 + 8, 412 - 390 + 8).0,
        [3, 2, 1, 255]
    );
    std::fs::remove_file(image).unwrap();
}

#[gpui::test]
fn the_history_offers_its_latest_stretch_again(cx: &mut TestAppContext) {
    let (window, mut visual, _) = pane(cx);
    window
        .update(&mut visual, |pane, _, cx| {
            pane.drawer = Drawer::History;
            pane.steps = vec![
                Step {
                    at: 1,
                    tool: "launch".into(),
                    input: json!({"bundle_id": "com.example.app"}),
                },
                Step {
                    at: 2,
                    tool: "tap".into(),
                    input: json!({"element": "Sign in"}),
                },
            ];
            cx.notify();
        })
        .unwrap();
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(visual.debug_bounds("simulator-again").is_some());
    assert_eq!(
        window
            .update(&mut visual, |pane, _, _| pane.last_app())
            .unwrap()
            .as_deref(),
        Some("com.example.app")
    );
}

#[gpui::test]
fn the_log_drawer_filters_by_level(cx: &mut TestAppContext) {
    let (window, mut visual, _) = pane(cx);
    window
        .update(&mut visual, |pane, _, cx| {
            pane.drawer = Drawer::Logs;
            cx.notify();
        })
        .unwrap();
    click(&mut visual, "simulator-simulator-log-level-error");
    let level = window
        .update(&mut visual, |pane, _, _| pane.log_filter.level)
        .unwrap();
    assert_eq!(level, Level::Error);
}

#[test]
fn input_becomes_the_runner_request_it_names() {
    assert_eq!(
        Input::Tap { x: 1., y: 2. }.request(),
        ("tap", json!({"x": 1.0, "y": 2.0}))
    );
    assert_eq!(Input::Key("return").request().0, "press_key");
    assert_eq!(
        Input::Home.request(),
        ("press_button", json!({"button": "home"}))
    );
}

#[test]
fn a_still_turns_with_the_device() {
    let portrait = image::RgbaImage::new(2, 4);
    assert_eq!(
        turn(&portrait, Orientation::LandscapeRight).dimensions(),
        (4, 2)
    );
    assert_eq!(
        turn(&portrait, Orientation::PortraitUpsideDown).dimensions(),
        (2, 4)
    );
}
