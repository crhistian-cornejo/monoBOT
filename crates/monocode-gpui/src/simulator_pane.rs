//! The Simulator tab: the booted iOS simulator's screen, live, through the
//! XCUITest runner the agents use (`monocode_core::simulator`). A click taps,
//! a drag swipes and typing types. Frames are fetched only while the tab is
//! in view; every call to the simulator runs off the interface thread.
use crate::{
    popover::AnchorSlot,
    right_panel::{icon_action, pane_header, spinner},
    theme::*,
};
use gpui::{
    AnyElement, App, Context, FocusHandle, Focusable, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseUpEvent, ObjectFit, Pixels, Point, RenderImage, Role, SharedString, Task, Window, div,
    img, prelude::*, px,
};
use monocode_core::simulator::{self, Device};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

/// The pause between frames. Capturing one takes about a quarter of a second
/// in the runner, so the pane shows three or four a second.
const FRAME_GAP: Duration = Duration::from_millis(40);
/// How often an empty tab looks for a booted device again.
const LOOK_AGAIN: Duration = Duration::from_secs(3);
/// A press that moves less than this is a tap; more is a swipe.
const DRAG_SLOP: f32 = 6.;

#[derive(Clone, Debug, PartialEq)]
enum Status {
    Looking,
    NoDevice,
    Booting,
    /// The first frame of a session builds and starts the runner.
    Starting,
    Live,
    Failed(String),
}

/// What the user did on the screen, sent to the runner in order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Input {
    Tap { x: f32, y: f32 },
    Swipe { from: (f32, f32), to: (f32, f32) },
    Text(String),
    Key(&'static str),
    Home,
}

impl Input {
    fn request(&self) -> (&'static str, Value) {
        match self {
            Self::Tap { x, y } => ("tap", json!({"x": x, "y": y})),
            Self::Swipe { from, to } => (
                "swipe",
                json!({"from_x": from.0, "from_y": from.1, "to_x": to.0, "to_y": to.1}),
            ),
            Self::Text(text) => ("type_text", json!({"text": text})),
            Self::Key(key) => ("press_key", json!({"key": key})),
            Self::Home => ("press_button", json!({"button": "home"})),
        }
    }
}

/// The point on the device under `position`, when the screen is drawn
/// `ObjectFit::Contain` inside `bounds`. `None` off the picture.
pub(crate) fn device_point(
    position: Point<Pixels>,
    bounds: gpui::Bounds<Pixels>,
    points: (f32, f32),
) -> Option<(f32, f32)> {
    let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    if points.0 <= 0. || points.1 <= 0. || width <= 0. || height <= 0. {
        return None;
    }
    let scale = (width / points.0).min(height / points.1);
    let left = f32::from(bounds.origin.x) + (width - points.0 * scale) / 2.;
    let top = f32::from(bounds.origin.y) + (height - points.1 * scale) / 2.;
    let x = (f32::from(position.x) - left) / scale;
    let y = (f32::from(position.y) - top) / scale;
    (x >= 0. && y >= 0. && x <= points.0 && y <= points.1).then_some((x.round(), y.round()))
}

/// The runner request a key press makes: text for printable keys, a named
/// key for the rest. Shortcuts with ⌘ or ⌃ stay with the app.
pub(crate) fn key_input(event: &KeyDownEvent) -> Option<Input> {
    let keystroke = &event.keystroke;
    if keystroke.modifiers.platform || keystroke.modifiers.control {
        return None;
    }
    let named = match keystroke.key.as_str() {
        "enter" => Some("return"),
        "backspace" => Some("delete"),
        "tab" => Some("tab"),
        "escape" => Some("escape"),
        "up" => Some("up"),
        "down" => Some("down"),
        "left" => Some("left"),
        "right" => Some("right"),
        _ => None,
    };
    if let Some(key) = named {
        return Some(Input::Key(key));
    }
    keystroke
        .key_char
        .clone()
        .filter(|text| !text.is_empty() && !text.chars().any(char::is_control))
        .map(Input::Text)
}

pub(crate) struct SimulatorPane {
    focus: FocusHandle,
    shown: bool,
    device: Option<Device>,
    frame: Option<Arc<RenderImage>>,
    points: (f32, f32),
    status: Status,
    screen: AnchorSlot,
    press: Option<Point<Pixels>>,
    frames: Option<Task<()>>,
    inputs: async_channel::Sender<(String, Input)>,
    _worker: Task<()>,
}

impl Focusable for SimulatorPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl SimulatorPane {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let (inputs, queue) = async_channel::unbounded::<(String, Input)>();
        // One worker sends the user's input in order; typing that piles up
        // while a tap runs goes as one piece of text.
        let worker = cx.spawn(async move |this, cx| {
            while let Ok((udid, mut input)) = queue.recv().await {
                if let Input::Text(text) = &mut input {
                    while let Ok((_, Input::Text(more))) = queue.try_recv() {
                        text.push_str(&more);
                    }
                }
                let (action, body) = input.request();
                let result = cx
                    .background_executor()
                    .spawn(async move { simulator::act(Some(&udid), action, |_| body) })
                    .await;
                if let Err(error) = result {
                    let alive = this.update(cx, |this, cx| {
                        this.status = Status::Failed(error);
                        cx.notify();
                    });
                    if alive.is_err() {
                        break;
                    }
                }
            }
        });
        Self {
            focus: cx.focus_handle(),
            shown: false,
            device: None,
            frame: None,
            points: (0., 0.),
            status: Status::Looking,
            screen: AnchorSlot::default(),
            press: None,
            frames: None,
            inputs,
            _worker: worker,
        }
    }

    pub(crate) fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        if self.shown == shown {
            return;
        }
        self.shown = shown;
        if shown {
            self.start(cx);
        } else {
            self.frames = None;
        }
        cx.notify();
    }

    /// Finds the booted device, then fetches frames until the tab is hidden.
    fn start(&mut self, cx: &mut Context<Self>) {
        self.status = Status::Looking;
        self.frames = Some(cx.spawn(async move |this, cx| {
            loop {
                let found = cx
                    .background_executor()
                    .spawn(async {
                        match simulator::xcode_problem() {
                            Some(problem) => Err(problem),
                            None => simulator::booted_device(),
                        }
                    })
                    .await;
                let device = match found {
                    Ok(Some(device)) => device,
                    Ok(None) | Err(_) => {
                        let status = match found {
                            Err(problem) => Status::Failed(problem),
                            _ => Status::NoDevice,
                        };
                        if this
                            .update(cx, |this, cx| {
                                if this.status != Status::Booting {
                                    this.status = status;
                                }
                                this.device = None;
                                cx.notify();
                            })
                            .is_err()
                        {
                            return;
                        }
                        cx.background_executor().timer(LOOK_AGAIN).await;
                        continue;
                    }
                };
                let udid = device.udid.clone();
                if this
                    .update(cx, |this, cx| {
                        if this.frame.is_none() {
                            this.status = Status::Starting;
                        }
                        this.device = Some(device);
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
                loop {
                    let udid = udid.clone();
                    let frame = cx
                        .background_executor()
                        .spawn(async move {
                            simulator::live_frame(&udid).map(|frame| {
                                let image = RenderImage::new(vec![image::Frame::new(frame.bgra)]);
                                (Arc::new(image), (frame.point_width, frame.point_height))
                            })
                        })
                        .await;
                    let failed = frame.is_err();
                    let alive = this.update(cx, |this, cx| {
                        match frame {
                            Ok((image, points)) => {
                                if let Some(old) = this.frame.replace(image) {
                                    cx.drop_image(old, None);
                                }
                                this.points = points;
                                this.status = Status::Live;
                            }
                            Err(error) => this.status = Status::Failed(error),
                        }
                        cx.notify();
                    });
                    if alive.is_err() {
                        return;
                    }
                    if failed {
                        // The device may have shut down: look again.
                        cx.background_executor().timer(LOOK_AGAIN).await;
                        break;
                    }
                    cx.background_executor().timer(FRAME_GAP).await;
                }
            }
        }));
    }

    fn send(&mut self, input: Input) {
        if let Some(device) = &self.device {
            let _ = self.inputs.try_send((device.udid.clone(), input));
        }
    }

    fn boot(&mut self, cx: &mut Context<Self>) {
        self.status = Status::Booting;
        cx.notify();
        let work = cx
            .background_executor()
            .spawn(async { simulator::boot(None, false) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.status = match result {
                    Ok(_) => Status::Looking,
                    Err(error) => Status::Failed(error),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn open_in_simulator_app(&self, cx: &mut Context<Self>) {
        if let Some(device) = &self.device {
            let udid = device.udid.clone();
            cx.background_executor()
                .spawn(async move {
                    let _ = simulator::open_simulator_app(&udid);
                })
                .detach();
        }
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.press = Some(event.position);
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        let (Some(start), Some(bounds)) = (self.press.take(), self.screen.get()) else {
            return;
        };
        let Some(from) = device_point(start, bounds, self.points) else {
            return;
        };
        let moved = (f32::from(event.position.x) - f32::from(start.x))
            .hypot(f32::from(event.position.y) - f32::from(start.y));
        if moved < DRAG_SLOP {
            self.send(Input::Tap {
                x: from.0,
                y: from.1,
            });
        } else if let Some(to) = device_point(event.position, bounds, self.points) {
            self.send(Input::Swipe { from, to });
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(input) = key_input(event) {
            self.send(input);
            cx.stop_propagation();
        }
    }

    fn message(&self) -> Option<(SharedString, bool)> {
        let busy = |text: &'static str| Some((SharedString::from(text), true));
        match &self.status {
            Status::Looking => busy("Looking for a simulator…"),
            Status::Booting => busy("Booting the simulator…"),
            Status::Starting => busy(
                "Starting monoBOT's test runner on the device. The first time builds it, which takes about a minute.",
            ),
            Status::NoDevice => Some(("No iOS simulator is running.".into(), false)),
            Status::Failed(error) if self.frame.is_none() => Some((error.clone().into(), false)),
            Status::Live | Status::Failed(_) => None,
        }
    }

    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        let live = self.device.is_some();
        let name: SharedString = self
            .device
            .as_ref()
            .map(|d| format!("{} · {}", d.name, d.runtime).into())
            .unwrap_or_else(|| "Simulator".into());
        pane_header("simulator-header", "Simulator")
            .child(div().flex_1().min_w_0().truncate().child(name))
            .when(
                matches!(self.status, Status::Failed(_)) && self.frame.is_some(),
                |d| {
                    d.child(
                        icon_action("simulator-retry", "Reconnect", "refresh-cw", true)
                            .on_click(cx.listener(|this, _, _, cx| this.start(cx))),
                    )
                },
            )
            .child(
                icon_action("simulator-home", "Home", "circle-dot", live).when(live, |d| {
                    d.on_click(cx.listener(|this, _, _, _| this.send(Input::Home)))
                }),
            )
            .child(
                icon_action(
                    "simulator-open",
                    "Open in Simulator.app",
                    "external-link",
                    live,
                )
                .when(live, |d| {
                    d.on_click(cx.listener(|this, _, _, cx| this.open_in_simulator_app(cx)))
                }),
            )
            .into_any_element()
    }

    fn screen(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(frame) = self.frame.clone() else {
            return div().into_any_element();
        };
        div()
            .id("simulator-screen")
            .debug_selector(|| "simulator-screen".into())
            .role(Role::Image)
            .aria_label("Simulator screen")
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .p_3()
            .child(
                div()
                    .relative()
                    .size_full()
                    .child(self.screen.probe())
                    .child(img(frame).size_full().object_fit(ObjectFit::Contain))
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up)),
            )
            .into_any_element()
    }

    fn placeholder(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (text, busy) = self.message()?;
        let empty = self.status == Status::NoDevice;
        Some(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .px_6()
                .text_size(px(12.))
                .text_color(content(0.6))
                .when(busy, |d| {
                    d.child(spinner("simulator-spinner", "loader", 16.))
                })
                .child(div().text_center().child(text))
                .when(empty, |d| {
                    let (fill, ink) = primary_action(true).unwrap_or((content(0.1), content(1.)));
                    d.child(
                        div()
                            .id("simulator-boot")
                            .role(Role::Button)
                            .aria_label("Boot a simulator")
                            .px_3()
                            .py_1()
                            .rounded(px(6.))
                            .bg(fill)
                            .text_color(ink)
                            .cursor_pointer()
                            .child("Boot a simulator")
                            .on_click(cx.listener(|this, _, _, cx| this.boot(cx))),
                    )
                })
                .into_any_element(),
        )
    }
}

impl Render for SimulatorPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = self.placeholder(cx).unwrap_or_else(|| self.screen(cx));
        div()
            .id("simulator-pane")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .role(Role::Group)
            .aria_label("Simulator")
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .child(self.header(cx))
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Bounds, point, size};

    #[test]
    fn a_click_lands_on_the_device_point_under_it() {
        // A 402 × 874 point screen in a 201 × 874 px box: drawn at half
        // size, 437 px tall, centred with 218.5 px above it.
        let bounds = Bounds::new(point(px(10.), px(20.)), size(px(201.), px(874.)));
        let points = (402., 874.);
        assert_eq!(
            device_point(point(px(10.), px(20. + 218.5)), bounds, points),
            Some((0., 0.))
        );
        assert_eq!(
            device_point(point(px(110.5), px(20. + 437.)), bounds, points),
            Some((201., 437.))
        );
        // Above the picture, in the letterbox.
        assert_eq!(device_point(point(px(50.), px(30.)), bounds, points), None);
        assert_eq!(
            device_point(point(px(50.), px(30.)), bounds, (0., 0.)),
            None
        );
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
}
