//! The Simulator tab: the booted iOS simulator, live, in a device bezel.
//!
//! - **The screen.** On a Mac the framebuffer the simulator draws into is a
//!   native layer placed over an empty, measured slot (`native.rs`), so it
//!   runs at the device's own frame rate. When CoreSimulator does not offer
//!   it, the pane falls back to frames from the XCUITest runner the agents
//!   use, three or four a second.
//! - **Touch and type.** A click taps, a drag swipes, typing types; all go
//!   through the runner in order.
//! - **The bezel's controls**, with Simulator.app's shortcuts: Home ⌘⇧H,
//!   rotate ⌘→ / ⌘←, appearance ⌘⇧A, keyboard ⌘K, screenshot ⌘S, record ⌘R,
//!   fit ⌘0 and actual size ⌘1.
//! - **Drawers** under the device: the agents' action history with
//!   "Again", the device log, and the inspector, which adds an element to
//!   the chat with a note and a crop.
//!
//! Every call to the simulator runs off the interface thread; the logic is
//! in `monocode_core::simulator`.
mod geometry;
#[cfg(target_os = "macos")]
mod native;

use crate::{
    input::{self as text_input},
    popover::AnchorSlot,
    right_panel::{icon_action, pane_header, spinner},
    settings::widgets::{primary_button, secondary_button, segmented},
    theme::*,
};
use geometry::Zoom;
use gpui::{
    AnyElement, App, BorderStyle, Context, Corners, Entity, EventEmitter, FocusHandle, Focusable,
    KeyBinding, KeyDownEvent, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point,
    RenderImage, Role, SharedString, Stateful, Subscription, Task, Window, actions, canvas, div,
    img, prelude::*, px,
};
use monocode_core::simulator::{
    self, Device,
    controls::{self, Orientation, Recording},
    history::{self, Step},
    inspect::{self, Element},
    logs::{self, Level, LogStream, Scope},
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime},
};

/// The pause between fallback frames. Capturing one takes about a quarter
/// of a second in the runner, so the pane shows three or four a second.
const FRAME_GAP: Duration = Duration::from_millis(40);
/// How often an empty tab looks for a booted device again, and a live one
/// checks that its device is still booted.
const LOOK_AGAIN: Duration = Duration::from_secs(3);
/// How often an open history looks for new steps.
const HISTORY_POLL: Duration = Duration::from_secs(1);
/// A press that moves less than this is a tap; more is a swipe.
const DRAG_SLOP: f32 = 6.;
/// The log lines the drawer keeps.
const LOG_KEEP: usize = 2000;
/// How long a notice under the controls stays.
const NOTICE_FOR: Duration = Duration::from_secs(5);
/// The bezel's thickness at full size, in device points.
const BEZEL: f32 = 14.;
/// The screen's corner rounding as a share of its short side.
const SCREEN_CORNER: f32 = 0.137;
/// The key context the bezel's shortcuts live in.
const CONTEXT: &str = "SimulatorPane";

actions!(
    simulator,
    [
        Home,
        RotateRight,
        RotateLeft,
        ToggleAppearance,
        ToggleKeyboard,
        Screenshot,
        Record,
        ZoomToFit,
        ActualSize
    ]
);

/// Simulator.app's shortcuts, while the pane has the focus.
pub(crate) fn bind(cx: &mut App) {
    let c = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("cmd-shift-h", Home, c),
        KeyBinding::new("cmd-right", RotateRight, c),
        KeyBinding::new("cmd-left", RotateLeft, c),
        KeyBinding::new("cmd-shift-a", ToggleAppearance, c),
        KeyBinding::new("cmd-k", ToggleKeyboard, c),
        KeyBinding::new("cmd-s", Screenshot, c),
        KeyBinding::new("cmd-r", Record, c),
        KeyBinding::new("cmd-0", ZoomToFit, c),
        KeyBinding::new("cmd-1", ActualSize, c),
    ]);
}

#[derive(Clone, Debug, PartialEq)]
enum Status {
    Looking,
    NoDevice,
    Booting,
    BootFailed(String),
    /// The first frame of a session builds and starts the runner.
    Starting,
    Live,
    Failed(String),
}

/// How the screen reaches the pane.
#[derive(Clone, Debug, Default, PartialEq)]
enum Source {
    #[default]
    None,
    /// The framebuffer, as a native layer.
    Native,
    /// The runner's frames; the reason the native view is not available.
    Frames(Option<String>),
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

/// Which drawer is open under the device.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Drawer {
    #[default]
    None,
    History,
    Logs,
    Inspector,
}

/// A still of the screen for the inspector: drawn while it is open (the
/// native view would cover the outline), and cut for crops.
struct Still {
    /// Turned like the device, BGRA, for drawing.
    shown: Arc<RenderImage>,
    /// Portrait, BGRA, for crops of portrait frames.
    portrait: Arc<image::RgbaImage>,
    /// Turned like `shown`, BGRA, for crops of a turned interface's frames.
    upright: Arc<image::RgbaImage>,
    /// Pixels per point in both.
    scale: f32,
}

struct Inspector {
    elements: Vec<Element>,
    /// Under the last pick, in front first.
    stack: Vec<usize>,
    /// Which of `stack` is chosen.
    index: usize,
    still: Option<Still>,
    /// The coordinates the tree's frames are in.
    space: geometry::Space,
    note: Entity<text_input::Input>,
    loading: bool,
    error: Option<String>,
}

impl Inspector {
    fn chosen(&self) -> Option<&Element> {
        self.elements.get(*self.stack.get(self.index)?)
    }
}

/// What the pane asks of the workspace.
pub(crate) enum SimulatorEvent {
    /// The inspector's "Add to chat": the element's description and note,
    /// and a crop of the screen around it.
    AddToChat { text: String, image: PathBuf },
}

pub(crate) struct SimulatorPane {
    focus: FocusHandle,
    shown: bool,
    device: Option<Device>,
    available_devices: Vec<Device>,
    selected_device: Option<String>,
    preferred_device: Option<String>,
    boot_simulator: fn(Option<&str>, bool) -> Result<Device, String>,
    /// The fallback's newest frame.
    frame: Option<Arc<RenderImage>>,
    /// The screen's size in points, portrait.
    points: (f32, f32),
    status: Status,
    source: Source,
    screen: AnchorSlot,
    press: Option<Point<Pixels>>,
    frames: Option<Task<()>>,
    inputs: async_channel::Sender<(String, Input)>,
    _worker: Task<()>,
    #[cfg(target_os = "macos")]
    native: native::NativeScreen,
    #[cfg(target_os = "macos")]
    feed: Option<Arc<simulator::native::ScreenFeed>>,
    orientation: Orientation,
    zoom: Zoom,
    hardware_keyboard: bool,
    recording: Option<Recording>,
    recording_tick: Option<Task<()>>,
    notice: Option<(SharedString, bool)>,
    notice_task: Option<Task<()>>,
    drawer: Drawer,
    steps: Vec<Step>,
    steps_version: Option<(u64, SystemTime)>,
    history_poll: Option<Task<()>>,
    /// While "Again" runs: steps done and in all.
    replaying: Option<(usize, usize)>,
    log_lines: VecDeque<logs::Line>,
    log_filter: logs::Filter,
    log_stream: Option<LogStream>,
    log_task: Option<Task<()>>,
    log_query: Entity<text_input::Input>,
    inspector: Option<Inspector>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SimulatorEvent> for SimulatorPane {}

impl Focusable for SimulatorPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn bgra_image(bgra: image::RgbaImage) -> Arc<RenderImage> {
    Arc::new(RenderImage::new(vec![image::Frame::new(bgra)]))
}

/// Turns a portrait picture the way the device is held.
fn turn(picture: &image::RgbaImage, orientation: Orientation) -> image::RgbaImage {
    match orientation.quarter_turns() {
        1 => image::imageops::rotate90(picture),
        2 => image::imageops::rotate180(picture),
        3 => image::imageops::rotate270(picture),
        _ => picture.clone(),
    }
}

impl SimulatorPane {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        Self::build(cx)
    }

    fn build(cx: &mut Context<Self>) -> Self {
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
                    let alive = this.update(cx, |this, cx| this.say(error, true, cx));
                    if alive.is_err() {
                        break;
                    }
                }
            }
        });
        let log_query =
            cx.new(|cx| text_input::Input::field("Filter the log", "Filter", content(0.4), cx));
        log_query.update(cx, |input, _| input.set_font_size(12.));
        let subscriptions =
            vec![
                cx.subscribe(&log_query, |this, input, _: &text_input::Changed, cx| {
                    this.log_filter.text = input.read(cx).content.to_string();
                    cx.notify();
                }),
            ];
        Self {
            focus: cx.focus_handle(),
            shown: false,
            device: None,
            available_devices: Vec::new(),
            selected_device: None,
            preferred_device: None,
            boot_simulator: simulator::boot,
            frame: None,
            points: (0., 0.),
            status: Status::Looking,
            source: Source::None,
            screen: AnchorSlot::default(),
            press: None,
            frames: None,
            inputs,
            _worker: worker,
            #[cfg(target_os = "macos")]
            native: native::NativeScreen::new(),
            #[cfg(target_os = "macos")]
            feed: None,
            orientation: Orientation::Portrait,
            zoom: Zoom::Fit,
            hardware_keyboard: true,
            recording: None,
            recording_tick: None,
            notice: None,
            notice_task: None,
            drawer: Drawer::None,
            steps: Vec::new(),
            steps_version: None,
            history_poll: None,
            replaying: None,
            log_lines: VecDeque::new(),
            log_filter: logs::Filter::default(),
            log_stream: None,
            log_task: None,
            log_query,
            inspector: None,
            _subscriptions: subscriptions,
        }
    }

    pub(crate) fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        if self.shown == shown {
            return;
        }
        self.shown = shown;
        if shown {
            if !matches!(self.status, Status::Booting | Status::BootFailed(_)) {
                self.start(cx);
            }
        } else {
            self.stop_feed(cx);
            self.frames = None;
            self.history_poll = None;
            self.log_stream = None;
            self.log_task = None;
        }
        cx.notify();
    }

    /// Shows a line under the controls for a few seconds.
    fn say(&mut self, text: impl Into<SharedString>, error: bool, cx: &mut Context<Self>) {
        self.notice = Some((text.into(), error));
        self.notice_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(NOTICE_FOR).await;
            let _ = this.update(cx, |this, cx| {
                this.notice = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Drops the native feed; closing it talks to CoreSimulator, so off the
    /// interface thread.
    fn stop_feed(&mut self, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        {
            self.native.hide();
            self.native.set_surface(None);
            if let Some(feed) = self.feed.take() {
                cx.background_executor()
                    .spawn(async move { drop(feed) })
                    .detach();
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = cx;
    }

    /// Finds the booted device, then shows its screen until the tab is
    /// hidden: the native layer when it can, else the runner's frames.
    fn start(&mut self, cx: &mut Context<Self>) {
        self.status = Status::Looking;
        self.stop_feed(cx);
        let preferred = self.preferred_device.clone();
        #[cfg(target_os = "macos")]
        let handler = Arc::new(self.native.handler());
        self.frames = Some(cx.spawn(async move |this, cx| {
            loop {
                let found = cx
                    .background_executor()
                    .spawn(async {
                        match simulator::xcode_problem() {
                            Some(problem) => Err(problem),
                            None => simulator::devices(),
                        }
                    })
                    .await;
                let device = found.as_ref().ok().and_then(|devices| {
                    devices.iter().find(|device| {
                        device.booted() && preferred.as_deref().is_none_or(|id| device.udid == id)
                    })
                });
                let Some(device) = device.cloned() else {
                    let (status, devices) = match found {
                        Ok(devices) => (Status::NoDevice, devices),
                        Err(problem) => (Status::Failed(problem), Vec::new()),
                    };
                    if this
                        .update(cx, |this, cx| {
                            if let Some(frame) = this.frame.take() {
                                cx.drop_image(frame, None);
                            }
                            if this.status != status || this.available_devices != devices {
                                this.status = status;
                                this.available_devices = devices;
                                cx.notify();
                            }
                            this.device = None;
                            this.source = Source::None;
                        })
                        .is_err()
                    {
                        return;
                    }
                    cx.background_executor().timer(LOOK_AGAIN).await;
                    continue;
                };
                let udid = device.udid.clone();
                if this
                    .update(cx, |this, cx| {
                        this.device = Some(device);
                        this.refresh_history(cx);
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
                #[cfg(target_os = "macos")]
                {
                    let handler = handler.clone();
                    let feed_udid = udid.clone();
                    let opened = cx
                        .background_executor()
                        .spawn(async move {
                            let feed = simulator::native::ScreenFeed::open(
                                &feed_udid,
                                simulator::native::Delivery::Main,
                                move |event| handler(event),
                            )?;
                            let scale = simulator::screen_scale(&feed_udid).unwrap_or(3.) as f32;
                            Ok::<_, String>((Arc::new(feed), scale))
                        })
                        .await;
                    match opened {
                        Ok((feed, scale)) => {
                            let surface = feed.surface();
                            let (width, height) = surface.size();
                            let alive = this.update(cx, |this, cx| {
                                this.points = (width as f32 / scale, height as f32 / scale);
                                this.native.set_surface(Some(surface));
                                this.feed = Some(feed);
                                this.source = Source::Native;
                                this.status = Status::Live;
                                cx.notify();
                            });
                            if alive.is_err() {
                                return;
                            }
                            // Live until the device shuts down or changes.
                            loop {
                                cx.background_executor().timer(LOOK_AGAIN).await;
                                let udid = udid.clone();
                                let still = cx
                                    .background_executor()
                                    .spawn(async move {
                                        simulator::devices().is_ok_and(|devices| {
                                            devices.iter().any(|d| d.udid == udid && d.booted())
                                        })
                                    })
                                    .await;
                                if !still {
                                    break;
                                }
                            }
                            if this.update(cx, |this, cx| this.stop_feed(cx)).is_err() {
                                return;
                            }
                            continue;
                        }
                        Err(reason) => {
                            if this
                                .update(cx, |this, _| this.source = Source::Frames(Some(reason)))
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                }
                #[cfg(not(target_os = "macos"))]
                if this
                    .update(cx, |this, _| this.source = Source::Frames(None))
                    .is_err()
                {
                    return;
                }
                if this
                    .update(cx, |this, cx| {
                        if this.frame.is_none() {
                            this.status = Status::Starting;
                        }
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
                                (
                                    bgra_image(frame.bgra),
                                    (frame.point_width, frame.point_height),
                                )
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

    fn udid(&self) -> Option<String> {
        self.device.as_ref().map(|d| d.udid.clone())
    }

    fn send(&mut self, input: Input) {
        if let Some(udid) = self.udid() {
            let _ = self.inputs.try_send((udid, input));
        }
    }

    fn boot_target(&self) -> Option<&str> {
        self.selected_device
            .as_deref()
            .filter(|id| {
                self.available_devices
                    .iter()
                    .any(|device| device.udid == *id)
            })
            .or_else(|| {
                self.available_devices
                    .first()
                    .map(|device| device.udid.as_str())
            })
    }

    fn boot(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.boot_target().map(str::to_owned) else {
            return;
        };
        self.frames = None;
        self.status = Status::Booting;
        cx.notify();
        let boot = self.boot_simulator;
        let work = cx
            .background_executor()
            .spawn(async move { boot(Some(&target), false) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(device) => {
                        this.preferred_device = Some(device.udid);
                        this.status = Status::Looking;
                        if this.shown {
                            this.start(cx);
                        }
                    }
                    Err(error) => this.status = Status::BootFailed(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open_in_simulator_app(&self, cx: &mut Context<Self>) {
        if let Some(udid) = self.udid() {
            cx.background_executor()
                .spawn(async move {
                    let _ = simulator::open_simulator_app(&udid);
                })
                .detach();
        }
    }

    /// Runs `work` on the device off the interface thread; `done` gets the
    /// answer back on it.
    fn on_device<R: Send + 'static>(
        &mut self,
        cx: &mut Context<Self>,
        work: impl FnOnce(String) -> Result<R, String> + Send + 'static,
        done: impl FnOnce(&mut Self, R, &mut Context<Self>) + 'static,
    ) {
        let Some(udid) = self.udid() else { return };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { work(udid) })
                .await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(value) => done(this, value, cx),
                Err(error) => this.say(error, true, cx),
            });
        })
        .detach();
    }

    // The bezel's controls.

    fn home(&mut self, _: &Home, _: &mut Window, _: &mut Context<Self>) {
        self.send(Input::Home);
    }

    fn rotate(&mut self, to: Orientation, cx: &mut Context<Self>) {
        self.on_device(
            cx,
            move |udid| controls::rotate(&udid, to),
            move |this, _, cx| {
                this.orientation = to;
                // The inspector's still and outlines were for the old turn.
                if this.drawer == Drawer::Inspector {
                    this.inspect(cx);
                }
                cx.notify();
            },
        );
    }

    fn rotate_right(&mut self, _: &RotateRight, _: &mut Window, cx: &mut Context<Self>) {
        self.rotate(self.orientation.rotated_right(), cx);
    }

    fn rotate_left(&mut self, _: &RotateLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.rotate(self.orientation.rotated_left(), cx);
    }

    fn toggle_appearance(&mut self, _: &ToggleAppearance, _: &mut Window, cx: &mut Context<Self>) {
        self.on_device(
            cx,
            |udid| controls::toggle_appearance(&udid),
            |this, mode, cx| this.say(format!("Switched to {mode} mode"), false, cx),
        );
    }

    fn toggle_keyboard(&mut self, _: &ToggleKeyboard, _: &mut Window, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        {
            let connected = !self.hardware_keyboard;
            self.on_device(
                cx,
                move |udid| simulator::native::set_hardware_keyboard(&udid, connected),
                move |this, _, cx| {
                    this.hardware_keyboard = connected;
                    this.say(
                        if connected {
                            "The Mac's keyboard types into the device"
                        } else {
                            "The device shows its own keyboard"
                        },
                        false,
                        cx,
                    );
                },
            );
        }
        #[cfg(not(target_os = "macos"))]
        let _ = cx;
    }

    fn screenshot(&mut self, _: &Screenshot, _: &mut Window, cx: &mut Context<Self>) {
        let name = self
            .device
            .as_ref()
            .map(|d| d.name.clone())
            .unwrap_or_default();
        self.on_device(
            cx,
            move |udid| controls::save_screenshot(&udid, &name),
            |this, path, cx| {
                let file = path.file_name().map(|n| n.to_string_lossy().into_owned());
                this.say(
                    format!("Saved {} to the Desktop", file.unwrap_or_default()),
                    false,
                    cx,
                );
            },
        );
    }

    fn record(&mut self, _: &Record, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(recording) = self.recording.take() {
            self.recording_tick = None;
            cx.notify();
            cx.spawn(async move |this, cx| {
                let saved = cx
                    .background_executor()
                    .spawn(async move { recording.stop() })
                    .await;
                let _ = this.update(cx, |this, cx| match saved {
                    Ok(path) => {
                        let file = path.file_name().map(|n| n.to_string_lossy().into_owned());
                        this.say(
                            format!("Saved {} to the Desktop", file.unwrap_or_default()),
                            false,
                            cx,
                        );
                    }
                    Err(error) => this.say(error, true, cx),
                });
            })
            .detach();
            return;
        }
        let name = self
            .device
            .as_ref()
            .map(|d| d.name.clone())
            .unwrap_or_default();
        self.on_device(
            cx,
            move |udid| Recording::start(&udid, &name),
            |this, recording, cx| {
                this.recording = Some(recording);
                // The elapsed time under the controls.
                this.recording_tick = Some(cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor().timer(Duration::from_secs(1)).await;
                        if this.update(cx, |_, cx| cx.notify()).is_err() {
                            return;
                        }
                    }
                }));
                cx.notify();
            },
        );
    }

    fn zoom_to_fit(&mut self, _: &ZoomToFit, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom = Zoom::Fit;
        cx.notify();
    }

    fn actual_size(&mut self, _: &ActualSize, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom = Zoom::Actual;
        cx.notify();
    }

    // Touches.

    /// The screen's size and turn as drawn: the native picture turns with
    /// the device; the runner's frames come already upright.
    fn view(&self) -> ((f32, f32), Orientation) {
        match self.source {
            Source::Native => (self.points, self.orientation),
            _ => (self.points, Orientation::Portrait),
        }
    }

    fn device_point(&self, position: Point<Pixels>) -> Option<(f32, f32)> {
        let slot = self.screen.get()?;
        let (points, orientation) = self.view();
        let rect = geometry::screen_rect(slot, points, orientation, self.zoom)?;
        geometry::device_point(position, rect, points, orientation)
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.press = Some(event.position);
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(start) = self.press.take() else {
            return;
        };
        if self.drawer == Drawer::Inspector {
            self.pick(start, cx);
            return;
        }
        let Some(from) = self.device_point(start) else {
            return;
        };
        let moved = (f32::from(event.position.x) - f32::from(start.x))
            .hypot(f32::from(event.position.y) - f32::from(start.y));
        if moved < DRAG_SLOP {
            self.send(Input::Tap {
                x: from.0,
                y: from.1,
            });
        } else if let Some(to) = self.device_point(event.position) {
            self.send(Input::Swipe { from, to });
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // Typing in the drawers' fields stays there.
        if !self.focus.is_focused(window) {
            return;
        }
        if let Some(input) = key_input(event) {
            self.send(input);
            cx.stop_propagation();
        }
    }

    // Drawers.

    fn toggle_drawer(&mut self, drawer: Drawer, cx: &mut Context<Self>) {
        let next = if self.drawer == drawer {
            Drawer::None
        } else {
            drawer
        };
        self.drawer = next;
        self.history_poll = None;
        if next != Drawer::Logs {
            self.log_stream = None;
            self.log_task = None;
        }
        if next != Drawer::Inspector {
            self.inspector = None;
        }
        match next {
            Drawer::History => self.watch_history(cx),
            Drawer::Logs => self.stream_logs(cx),
            Drawer::Inspector => self.inspect(cx),
            Drawer::None => {}
        }
        cx.notify();
    }

    fn refresh_history(&mut self, cx: &mut Context<Self>) {
        let Some(udid) = self.udid() else { return };
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { (history::version(&udid), history::read(&udid)) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.steps_version = read.0;
                this.steps = read.1;
                cx.notify();
            });
        })
        .detach();
    }

    fn watch_history(&mut self, cx: &mut Context<Self>) {
        self.refresh_history(cx);
        self.history_poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(HISTORY_POLL).await;
                let Ok((udid, known)) =
                    this.update(cx, |this, _| (this.udid(), this.steps_version))
                else {
                    return;
                };
                let Some(udid) = udid else { continue };
                let changed = cx
                    .background_executor()
                    .spawn(async move { history::version(&udid) != known })
                    .await;
                if changed
                    && this
                        .update(cx, |this, cx| this.refresh_history(cx))
                        .is_err()
                {
                    return;
                }
            }
        }));
    }

    /// "Again · N actions": the latest stretch, run once more.
    fn replay(&mut self, cx: &mut Context<Self>) {
        let Some(udid) = self.udid() else { return };
        if self.replaying.is_some() {
            return;
        }
        let steps = history::last_stretch(&self.steps).to_vec();
        if steps.is_empty() {
            return;
        }
        self.replaying = Some((0, steps.len()));
        cx.notify();
        let (progress, done) = async_channel::unbounded::<usize>();
        let work = cx.background_executor().spawn(async move {
            history::replay(&udid, &steps, |i| {
                let _ = progress.send_blocking(i);
            })
        });
        cx.spawn(async move |this, cx| {
            while let Ok(step) = done.recv().await {
                if this
                    .update(cx, |this, cx| {
                        if let Some((_, total)) = this.replaying {
                            this.replaying = Some((step, total));
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
            }
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.replaying = None;
                match result {
                    Ok(()) => this.say("Ran the actions again", false, cx),
                    Err(error) => this.say(error, true, cx),
                }
            });
        })
        .detach();
    }

    /// The bundle the agents launched last, for the log's App scope and
    /// the inspector.
    fn last_app(&self) -> Option<String> {
        self.steps
            .iter()
            .rev()
            .find(|s| s.tool == "launch")
            .and_then(|s| s.input["bundle_id"].as_str().map(str::to_owned))
    }

    fn stream_logs(&mut self, cx: &mut Context<Self>) {
        let Some(udid) = self.udid() else { return };
        self.refresh_history(cx);
        let (sender, lines) = async_channel::unbounded::<Vec<logs::Line>>();
        match LogStream::start(&udid, Level::Info, move |batch| {
            sender.send_blocking(batch).is_ok()
        }) {
            Ok(stream) => self.log_stream = Some(stream),
            Err(error) => {
                self.say(error, true, cx);
                return;
            }
        }
        self.log_task = Some(cx.spawn(async move |this, cx| {
            while let Ok(batch) = lines.recv().await {
                let alive = this.update(cx, |this, cx| {
                    this.log_lines.extend(batch);
                    while this.log_lines.len() > LOG_KEEP {
                        this.log_lines.pop_front();
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
        }));
    }

    /// Opens the inspector: a still of the screen and its elements.
    fn inspect(&mut self, cx: &mut Context<Self>) {
        let Some(udid) = self.udid() else { return };
        let note = match self.inspector.take() {
            Some(old) => old.note,
            None => cx.new(|cx| text_input::Input::field("Note", "Add a note…", content(0.4), cx)),
        };
        self.inspector = Some(Inspector {
            elements: Vec::new(),
            stack: Vec::new(),
            index: 0,
            still: None,
            space: geometry::Space::Portrait,
            note,
            loading: true,
            error: None,
        });
        let bundle = self.last_app();
        let orientation = self.orientation;
        #[cfg(target_os = "macos")]
        let surface = self.feed.as_ref().map(|feed| feed.surface());
        #[cfg(not(target_os = "macos"))]
        let surface: Option<()> = None;
        let points = self.points;
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move {
                    let elements = inspect::describe(&udid, bundle.as_deref());
                    #[cfg(target_os = "macos")]
                    let native = surface.and_then(|s| s.copy_bgra());
                    #[cfg(not(target_os = "macos"))]
                    let native: Option<image::RgbaImage> = surface.and(None);
                    let (portrait, turned) = match native {
                        Some(portrait) => {
                            let shown = turn(&portrait, orientation);
                            (Some(portrait), Some(shown))
                        }
                        None => match simulator::live_frame(&udid) {
                            Ok(frame) => (Some(frame.bgra.clone()), Some(frame.bgra)),
                            Err(_) => (None, None),
                        },
                    };
                    let still = portrait.zip(turned).map(|(portrait, shown)| Still {
                        scale: portrait.width() as f32 / points.0.max(1.),
                        portrait: Arc::new(portrait),
                        upright: Arc::new(shown.clone()),
                        shown: bgra_image(shown),
                    });
                    (elements, still)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(inspector) = this.inspector.as_mut() else {
                    return;
                };
                inspector.loading = false;
                match read.0 {
                    Ok(elements) => {
                        let root = elements.first().map(|e| e.frame);
                        inspector.space = geometry::Space::of(root, orientation);
                        inspector.elements = elements;
                    }
                    Err(error) => inspector.error = Some(error),
                }
                if let Some(old) = std::mem::replace(&mut inspector.still, read.1) {
                    cx.drop_image(old.shown, None);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Picks the elements under `position`, in the tree's coordinates.
    fn pick(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let (points, orientation) = self.view();
        let Some(rect) = self
            .screen
            .get()
            .and_then(|slot| geometry::screen_rect(slot, points, orientation, self.zoom))
        else {
            return;
        };
        let Some(inspector) = self.inspector.as_mut() else {
            return;
        };
        let Some(point) = inspector.space.point(position, rect, points, orientation) else {
            return;
        };
        inspector.stack = inspect::stack_at(&inspector.elements, point.0, point.1);
        inspector.index = 0;
        cx.notify();
    }

    fn step(&mut self, behind: bool, cx: &mut Context<Self>) {
        let Some(inspector) = self.inspector.as_mut() else {
            return;
        };
        let last = inspector.stack.len().saturating_sub(1);
        inspector.index = if behind {
            (inspector.index + 1).min(last)
        } else {
            inspector.index.saturating_sub(1)
        };
        cx.notify();
    }

    fn add_to_chat(&mut self, cx: &mut Context<Self>) {
        let Some(inspector) = self.inspector.as_ref() else {
            return;
        };
        let Some(element) = inspector.chosen().cloned() else {
            return;
        };
        let Some(still) = inspector.still.as_ref() else {
            return;
        };
        let picture = match inspector.space {
            geometry::Space::Portrait => still.portrait.clone(),
            geometry::Space::Upright => still.upright.clone(),
        };
        let scale = still.scale;
        let note = inspector.note.read(cx).content.to_string();
        let device = self
            .device
            .as_ref()
            .map(|d| d.name.clone())
            .unwrap_or_default();
        let text = element.chat_text(&device, &note);
        cx.spawn(async move |this, cx| {
            let saved = cx
                .background_executor()
                .spawn(async move {
                    let crop = inspect::crop(&picture, element.frame, scale)
                        .ok_or("The element is off the screen")?;
                    inspect::save_crop(&crop)
                })
                .await;
            let _ = this.update(cx, |this, cx| match saved {
                Ok(image) => {
                    if let Some(inspector) = this.inspector.as_ref() {
                        inspector
                            .note
                            .update(cx, |input, cx| input.set_text(String::new(), cx));
                    }
                    cx.emit(SimulatorEvent::AddToChat { text, image });
                    this.say("Added to the chat", false, cx);
                }
                Err(error) => this.say(error, true, cx),
            });
        })
        .detach();
    }

    // Drawing.

    fn message(&self) -> Option<(SharedString, bool)> {
        let busy = |text: &'static str| Some((SharedString::from(text), true));
        match &self.status {
            Status::Looking => busy("Looking for a simulator…"),
            Status::Booting => busy("Booting the simulator…"),
            Status::Starting => busy(
                "Starting monoBOT's test runner on the device. The first time builds it, which takes about a minute.",
            ),
            Status::NoDevice => Some(("No iOS simulator is running.".into(), false)),
            Status::BootFailed(error) => Some((error.clone().into(), false)),
            Status::Failed(error) if self.frame.is_none() && self.source != Source::Native => {
                Some((error.clone().into(), false))
            }
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
        let drawer_button = |id: &'static str,
                             label: &'static str,
                             icon: &'static str,
                             drawer: Drawer| {
            let open = self.drawer == drawer;
            icon_action(id, label, icon, live)
                .aria_selected(open)
                .when(open, |d| d.bg(content(0.1)))
                .when(live, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| this.toggle_drawer(drawer, cx)))
                })
        };
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
            .child(drawer_button(
                "simulator-inspect",
                "Inspect",
                "pipette",
                Drawer::Inspector,
            ))
            .child(drawer_button(
                "simulator-history",
                "Actions",
                "clock",
                Drawer::History,
            ))
            .child(drawer_button(
                "simulator-logs",
                "Logs",
                "terminal",
                Drawer::Logs,
            ))
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

    /// The device: its bezel and screen, painted in one canvas; on a Mac the
    /// native view over the screen while nothing needs drawing on top.
    fn device_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let (points, orientation) = self.view();
        let zoom = self.zoom;
        let inspecting = self.inspector.as_ref();
        let still = inspecting.and_then(|i| i.still.as_ref().map(|s| s.shown.clone()));
        let outline = inspecting.and_then(|i| i.chosen().map(|e| e.frame));
        let space = inspecting.map(|i| i.space).unwrap_or_default();
        let others: Vec<(f32, f32, f32, f32)> = inspecting
            .map(|i| {
                i.stack
                    .iter()
                    .skip(1)
                    .filter_map(|&n| i.elements.get(n).map(|e| e.frame))
                    .collect()
            })
            .unwrap_or_default();
        let native_live = self.source == Source::Native && inspecting.is_none();
        let picture = if inspecting.is_some() {
            still
        } else if self.source == Source::Native {
            None
        } else {
            self.frame.clone()
        };
        let accent = accent_color();
        let bezel = content(0.12);
        let rim = content(0.2);
        let paint = canvas(
            |_, _, _| {},
            move |slot, _, window, _| {
                let Some(rect) = geometry::screen_rect(slot, points, orientation, zoom) else {
                    return;
                };
                let scale = f32::from(rect.size.width.min(rect.size.height))
                    / points.0.min(points.1).max(1.);
                let thickness = px(BEZEL * scale.clamp(0.35, 1.));
                let corner = f32::from(rect.size.width.min(rect.size.height)) * SCREEN_CORNER;
                let outer = rect.dilate(thickness);
                window.paint_quad(gpui::quad(
                    outer,
                    Corners::all(px(corner) + thickness),
                    bezel,
                    px(1.),
                    rim,
                    BorderStyle::Solid,
                ));
                window.paint_quad(
                    gpui::fill(rect, gpui::black()).corner_radii(Corners::all(px(corner))),
                );
                if let Some(picture) = picture.clone() {
                    let _ =
                        window.paint_image(rect, rect, Corners::all(px(corner)), picture, 0, false);
                }
                for frame in &others {
                    let shown = space.rect(*frame, rect, points, orientation);
                    window.paint_quad(gpui::outline(
                        shown,
                        accent.opacity(0.35),
                        BorderStyle::Dashed,
                    ));
                }
                if let Some(frame) = outline {
                    let shown = space.rect(frame, rect, points, orientation);
                    window.paint_quad(gpui::quad(
                        shown,
                        Corners::all(px(2.)),
                        accent.opacity(0.12),
                        px(2.),
                        accent,
                        BorderStyle::Solid,
                    ));
                }
            },
        )
        .absolute()
        .inset_0()
        .size_full();
        #[cfg(target_os = "macos")]
        let native_slot = if native_live && self.shown {
            Some(
                div()
                    .absolute()
                    .inset_0()
                    .child(self.native.slot(native::Placement {
                        points,
                        orientation,
                        zoom,
                    })),
            )
        } else {
            self.native.hide();
            None
        };
        #[cfg(not(target_os = "macos"))]
        let native_slot: Option<gpui::Div> = {
            let _ = native_live;
            None
        };
        div()
            .id("simulator-screen")
            .debug_selector(|| "simulator-screen".into())
            .role(Role::Image)
            .aria_label("Simulator screen")
            .relative()
            .flex_1()
            .min_h(px(120.))
            .w_full()
            .overflow_hidden()
            .p_4()
            .child(
                div()
                    .relative()
                    .size_full()
                    .child(self.screen.probe())
                    .child(paint)
                    .children(native_slot)
                    .when(self.drawer == Drawer::Inspector, |d| d.cursor_crosshair())
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up)),
            )
            .into_any_element()
    }

    /// The bezel's buttons, under the device.
    fn controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let live = self.device.is_some();
        let control = |id: &'static str,
                       label: &'static str,
                       icon_path: &'static str,
                       keys: &'static str,
                       active: bool|
         -> Stateful<gpui::Div> {
            div()
                .id(id)
                .debug_selector(move || id.into())
                .role(Role::Button)
                .aria_label(label)
                .tooltip(chrome_tip_keys(label, keys))
                .size(px(26.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .when(active, |d| d.bg(content(0.1)))
                .when(!live, |d| d.opacity(0.4))
                .when(live, |d| d.cursor_pointer().hover(|s| s.bg(content(0.1))))
                .child(icon(icon_path, 15.).text_color(if active {
                    content(0.9)
                } else {
                    content(0.6)
                }))
        };
        let recording = self.recording.as_ref().map(|r| r.elapsed().as_secs());
        let notice = self.notice.clone().or_else(|| {
            recording.map(|secs| {
                (
                    SharedString::from(format!("Recording · {}:{:02}", secs / 60, secs % 60)),
                    false,
                )
            })
        });
        let note = match &self.source {
            Source::Frames(Some(reason)) if notice.is_none() => Some((
                SharedString::from(format!("Showing snapshots: {reason}")),
                false,
            )),
            _ => None,
        };
        div()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .pb_2()
            .child(
                div()
                    .id("simulator-controls")
                    .role(Role::Toolbar)
                    .aria_label("Device controls")
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .px_1()
                    .py_0p5()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(content(0.08))
                    .child(
                        control("simulator-home", "Home", "circle-dot", "⌘⇧H", false)
                            .when(live, |d| {
                                d.on_click(cx.listener(|this, _, w, cx| this.home(&Home, w, cx)))
                            }),
                    )
                    .child(
                        control("simulator-rotate", "Rotate", "rotate-ccw", "⌘→", false).when(
                            live,
                            |d| {
                                d.on_click(cx.listener(|this, _, w, cx| {
                                    this.rotate_right(&RotateRight, w, cx)
                                }))
                            },
                        ),
                    )
                    .child(
                        control(
                            "simulator-appearance",
                            "Light or dark",
                            "palette",
                            "⌘⇧A",
                            false,
                        )
                        .when(live, |d| {
                            d.on_click(cx.listener(|this, _, w, cx| {
                                this.toggle_appearance(&ToggleAppearance, w, cx)
                            }))
                        }),
                    )
                    .child(
                        control(
                            "simulator-keyboard",
                            "Software keyboard",
                            "keyboard",
                            "⌘K",
                            !self.hardware_keyboard,
                        )
                        .when(live, |d| {
                            d.on_click(cx.listener(|this, _, w, cx| {
                                this.toggle_keyboard(&ToggleKeyboard, w, cx)
                            }))
                        }),
                    )
                    .child(div().w_px().h(px(16.)).mx_1().bg(content(0.1)))
                    .child(
                        control(
                            "simulator-screenshot",
                            "Screenshot",
                            "image-01",
                            "⌘S",
                            false,
                        )
                        .when(live, |d| {
                            d.on_click(
                                cx.listener(|this, _, w, cx| this.screenshot(&Screenshot, w, cx)),
                            )
                        }),
                    )
                    .child(
                        control(
                            "simulator-record",
                            if self.recording.is_some() {
                                "Stop recording"
                            } else {
                                "Record"
                            },
                            if self.recording.is_some() {
                                "square-fill"
                            } else {
                                "circle-dot"
                            },
                            "⌘R",
                            self.recording.is_some(),
                        )
                        .when(live, |d| {
                            d.on_click(cx.listener(|this, _, w, cx| this.record(&Record, w, cx)))
                        }),
                    )
                    .child(div().w_px().h(px(16.)).mx_1().bg(content(0.1)))
                    .child(
                        control(
                            "simulator-fit",
                            "Zoom to fit",
                            "window-restore",
                            "⌘0",
                            self.zoom == Zoom::Fit,
                        )
                        .when(live, |d| {
                            d.on_click(
                                cx.listener(|this, _, w, cx| this.zoom_to_fit(&ZoomToFit, w, cx)),
                            )
                        }),
                    )
                    .child(
                        control(
                            "simulator-actual",
                            "Actual size",
                            "window-maximize",
                            "⌘1",
                            self.zoom == Zoom::Actual,
                        )
                        .when(live, |d| {
                            d.on_click(
                                cx.listener(|this, _, w, cx| this.actual_size(&ActualSize, w, cx)),
                            )
                        }),
                    ),
            )
            .children(notice.or(note).map(|(text, error)| {
                div()
                    .id("simulator-notice")
                    .debug_selector(|| "simulator-notice".into())
                    .role(Role::Status)
                    .max_w_full()
                    .px_3()
                    .truncate()
                    .text_size(px(11.))
                    .text_color(if error { danger() } else { content(0.55) })
                    .child(text)
            }))
            .into_any_element()
    }

    fn drawer_view(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let body = match self.drawer {
            Drawer::None => return None,
            Drawer::History => self.history_view(cx),
            Drawer::Logs => self.logs_view(cx),
            Drawer::Inspector => self.inspector_view(cx),
        };
        Some(
            div()
                .id("simulator-drawer")
                .flex_shrink_0()
                .h(px(240.))
                .min_h_0()
                .flex()
                .flex_col()
                .border_t_1()
                .border_color(content(0.1))
                .child(body)
                .into_any_element(),
        )
    }

    fn history_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let udid = self.udid().unwrap_or_default();
        let stretch = history::last_stretch(&self.steps).len();
        let again: SharedString = match self.replaying {
            Some((done, total)) => format!("Running {} of {total}…", (done + 1).min(total)).into(),
            None if stretch == 1 => "Again · 1 action".into(),
            None => format!("Again · {stretch} actions").into(),
        };
        let can_again = stretch > 0 && self.replaying.is_none();
        let rows = self
            .steps
            .iter()
            .rev()
            .take(60)
            .enumerate()
            .map(|(n, step)| {
                let shot = step.screenshot(&udid);
                div()
                    .id(("simulator-step", n))
                    .role(Role::ListItem)
                    .aria_label(SharedString::from(step.title()))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .child(
                        div()
                            .w(px(28.))
                            .h(px(56.))
                            .flex_shrink_0()
                            .rounded(px(4.))
                            .overflow_hidden()
                            .bg(content(0.06))
                            .children(shot.map(|path| {
                                img(path).size_full().object_fit(gpui::ObjectFit::Cover)
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(content(0.85))
                                    .child(step.title()),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(content(0.45))
                                    .child(ago(step.at)),
                            ),
                    )
            });
        div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .child(div().flex_1().child(section_label("Agent actions")))
                    .child(
                        secondary_button("simulator-again", again, false, !can_again)
                            .debug_selector(|| "simulator-again".into())
                            .when(can_again, |d| d.on_click(cx.listener(|this, _, _, cx| this.replay(cx)))),
                    ),
            )
            .child(
                div()
                    .id("simulator-steps")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(self.steps.is_empty(), |d| {
                        d.child(
                            div()
                                .px_3()
                                .text_size(px(12.))
                                .text_color(content(0.5))
                                .child("When an agent taps, types or launches an app on this device, each step shows here with the screen after it."),
                        )
                    })
                    .children(rows),
            )
            .into_any_element()
    }

    fn logs_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let level = match self.log_filter.level {
            Level::Debug | Level::Info => "info",
            Level::Default => "notice",
            Level::Error => "error",
            Level::Fault => "fault",
        };
        let app = self.last_app();
        let scope = match &self.log_filter.scope {
            Scope::Device => "device",
            _ => "app",
        };
        let entity = cx.entity().downgrade();
        let pick_level = {
            let entity = entity.clone();
            move |option: &'static str, _: &mut Window, cx: &mut App| {
                let level = match option {
                    "notice" => Level::Default,
                    "error" => Level::Error,
                    "fault" => Level::Fault,
                    _ => Level::Info,
                };
                let _ = entity.update(cx, |this, cx| {
                    this.log_filter.level = level;
                    cx.notify();
                });
            }
        };
        let pick_scope = {
            let app = app.clone();
            move |option: &'static str, _: &mut Window, cx: &mut App| {
                let scope = match (option, &app) {
                    ("app", Some(bundle)) => Scope::Subsystem(bundle.clone()),
                    _ => Scope::Device,
                };
                let _ = entity.update(cx, |this, cx| {
                    this.log_filter.scope = scope;
                    cx.notify();
                });
            }
        };
        let shown: Vec<&logs::Line> = self
            .log_lines
            .iter()
            .rev()
            .filter(|line| self.log_filter.admits(line))
            .take(300)
            .collect();
        let rows = shown.into_iter().enumerate().map(|(n, line)| {
            let color = match line.level {
                Level::Fault | Level::Error => danger(),
                Level::Default => content(0.8),
                _ => content(0.6),
            };
            div()
                .id(("simulator-log", n))
                .flex()
                .gap_2()
                .px_3()
                .text_size(px(11.))
                .font_family(crate::fonts::mono())
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(content(0.4))
                        .child(line.time.clone()),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .w(px(90.))
                        .truncate()
                        .text_color(content(0.5))
                        .child(line.process.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(color)
                        .child(line.message.clone()),
                )
        });
        let scopes: Vec<(&'static str, &str)> = if app.is_some() {
            vec![("device", "Device"), ("app", "App")]
        } else {
            vec![("device", "Device")]
        };
        div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .child(segmented(
                        "simulator-log-level",
                        "Level",
                        level,
                        &[
                            ("info", "All"),
                            ("notice", "Notice"),
                            ("error", "Errors"),
                            ("fault", "Faults"),
                        ],
                        "simulator",
                        pick_level,
                    ))
                    .child(segmented(
                        "simulator-log-scope",
                        "Scope",
                        scope,
                        &scopes,
                        "simulator",
                        pick_scope,
                    ))
                    .child(div().flex_1().min_w(px(60.)).child(self.log_query.clone())),
            )
            .child(
                div()
                    .id("simulator-log-lines")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(self.log_lines.is_empty(), |d| {
                        d.child(
                            div()
                                .px_3()
                                .text_size(px(12.))
                                .text_color(content(0.5))
                                .child("Waiting for the device's log…"),
                        )
                    })
                    .children(rows),
            )
            .into_any_element()
    }

    fn inspector_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(inspector) = self.inspector.as_ref() else {
            return div().into_any_element();
        };
        let chosen = inspector.chosen();
        let depth = inspector.stack.len();
        let summary: SharedString = if inspector.loading {
            "Reading the screen…".into()
        } else if let Some(error) = &inspector.error {
            error.clone().into()
        } else if let Some(element) = chosen {
            element.summary.clone().into()
        } else {
            "Click an element on the screen.".into()
        };
        let can_add = chosen.is_some() && inspector.still.is_some();
        let (fill, ink) = primary_action(can_add).unwrap_or((content(0.1), content(1.)));
        div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .gap_2()
            .px_3()
            .py_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(div().flex_1().child(section_label("Inspector")))
                    .when(depth > 1, |d| {
                        d.child(
                            div()
                                .text_size(px(11.))
                                .text_color(content(0.45))
                                .child(format!("{} of {depth}", inspector.index + 1)),
                        )
                    })
                    .child(
                        icon_action(
                            "simulator-front",
                            "In front",
                            "chevron-up",
                            inspector.index > 0,
                        )
                        .debug_selector(|| "simulator-front".into())
                        .on_click(cx.listener(|this, _, _, cx| this.step(false, cx))),
                    )
                    .child(
                        icon_action(
                            "simulator-behind",
                            "Behind",
                            "chevron-down-2",
                            inspector.index + 1 < depth,
                        )
                        .debug_selector(|| "simulator-behind".into())
                        .on_click(cx.listener(|this, _, _, cx| this.step(true, cx))),
                    )
                    .child(
                        icon_action(
                            "simulator-reinspect",
                            "Read the screen again",
                            "refresh-cw",
                            !inspector.loading,
                        )
                        .debug_selector(|| "simulator-reinspect".into())
                        .on_click(cx.listener(|this, _, _, cx| this.inspect(cx))),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| "simulator-inspector-summary".into())
                    .text_size(px(12.))
                    .text_color(content(0.8))
                    .child(summary),
            )
            .children(chosen.map(|element| {
                let (x, y, w, h) = element.frame;
                div()
                    .text_size(px(11.))
                    .text_color(content(0.45))
                    .child(format!("x {x}, y {y} · {w}×{h} pt"))
            }))
            .child(
                div()
                    .px_2()
                    .py_1()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(content(0.1))
                    .child(inspector.note.clone()),
            )
            .child(
                div().flex().child(
                    div()
                        .id("simulator-add-to-chat")
                        .debug_selector(|| "simulator-add-to-chat".into())
                        .role(Role::Button)
                        .aria_label("Add to chat")
                        .h(px(28.))
                        .px_3()
                        .rounded_md()
                        .flex()
                        .items_center()
                        .text_size(px(12.))
                        .bg(fill)
                        .text_color(ink)
                        .map(|d| {
                            if can_add {
                                d.cursor_pointer()
                            } else {
                                d.opacity(0.4)
                            }
                        })
                        .when(can_add, |d| {
                            d.on_click(cx.listener(|this, _, _, cx| this.add_to_chat(cx)))
                        })
                        .child("Add to chat"),
                ),
            )
            .into_any_element()
    }

    fn placeholder(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (text, busy) = self.message()?;
        let empty = matches!(self.status, Status::NoDevice);
        let boot_failed = matches!(self.status, Status::BootFailed(_));
        let target = self.boot_target();
        let can_boot = (empty || boot_failed) && target.is_some();
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
                .child(div().w_full().text_center().child(text))
                .when(empty && target.is_none(), |d| {
                    d.child("Install an iOS runtime in Xcode → Settings → Components.")
                })
                .when(can_boot, |d| {
                    d.child(
                        div()
                            .id("simulator-devices")
                            .w_full()
                            .min_h_0()
                            .max_h(px(156.))
                            .overflow_y_scroll()
                            .children(self.available_devices.iter().enumerate().map(
                                |(index, device)| {
                                    let id = device.udid.clone();
                                    let selected = target == Some(id.as_str());
                                    let label = format!("{} · {}", device.name, device.runtime);
                                    div()
                                        .id(("simulator-device", index))
                                        .debug_selector(move || format!("simulator-device-{index}"))
                                        .aria_label(label.clone())
                                        .aria_selected(selected)
                                        .role(Role::Button)
                                        .tooltip(chrome_tip(label.clone(), None))
                                        .px_2()
                                        .py_1()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .rounded_md()
                                        .cursor_pointer()
                                        .when(selected, |d| {
                                            d.bg(content(0.1)).text_color(content(1.))
                                        })
                                        .hover(|s| s.bg(content(0.08)))
                                        .child(div().flex_1().min_w_0().truncate().child(label))
                                        .when(selected, |d| d.child(icon("check", 12.)))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.selected_device = Some(id.clone());
                                            cx.notify();
                                        }))
                                },
                            )),
                    )
                    .child(
                        primary_button(
                            "simulator-boot",
                            if boot_failed {
                                "Retry boot"
                            } else {
                                "Boot simulator"
                            },
                            false,
                        )
                        .debug_selector(|| "simulator-boot".into())
                        .on_click(cx.listener(|this, _, _, cx| this.boot(cx))),
                    )
                })
                .when(
                    boot_failed || matches!(self.status, Status::Failed(_)),
                    |d| {
                        d.child(
                            secondary_button(
                                "simulator-refresh-devices",
                                "Refresh devices",
                                false,
                                false,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.start(cx))),
                        )
                    },
                )
                .into_any_element(),
        )
    }
}

/// "12 s ago", "3 min ago", "2 h ago".
fn ago(at_ms: i64) -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    let secs = ((now - at_ms) / 1000).max(0);
    match secs {
        0..60 => format!("{secs} s ago"),
        60..3600 => format!("{} min ago", secs / 60),
        _ => format!("{} h ago", secs / 3600),
    }
}

impl Render for SimulatorPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.placeholder(cx) {
            Some(placeholder) => {
                #[cfg(target_os = "macos")]
                self.native.hide();
                placeholder
            }
            None => div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(self.device_view(cx))
                .child(self.controls(cx))
                .children(self.drawer_view(cx))
                .into_any_element(),
        };
        div()
            .id("simulator-pane")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::rotate_right))
            .on_action(cx.listener(Self::rotate_left))
            .on_action(cx.listener(Self::toggle_appearance))
            .on_action(cx.listener(Self::toggle_keyboard))
            .on_action(cx.listener(Self::screenshot))
            .on_action(cx.listener(Self::record))
            .on_action(cx.listener(Self::zoom_to_fit))
            .on_action(cx.listener(Self::actual_size))
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
mod tests;
