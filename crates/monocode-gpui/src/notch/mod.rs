//! The notch: a borderless GPUI window over the camera housing, or a pill
//! floating under the menu bar, that says what the agents are doing and what
//! needs the user. Closed it is the housing itself (or a slim handle on the
//! top edge where there is no camera); it grows ears while agents work,
//! hangs a notice when something asks, and opens a panel with pages: Home
//! (widgets the user arranges), Agents, Chat and its own Settings.
//!
//! The window never resizes while it moves: it takes the room of both shapes
//! once, the island springs between them inside it at the display's rate,
//! and the window shrinks back to the island when it settles. Its rules live
//! in `monocode_core::notch`; this module only paints and moves.
mod pages;
mod platform;
mod settings;
mod shelf;
#[cfg(test)]
pub(crate) mod tests;

use crate::input::{self, Input};
use crate::theme::*;
use gpui::{
    AnyElement, App, Bounds, Context, Entity, EventEmitter, Pixels, Role, Subscription, Task,
    Window, WindowHandle, canvas, div, point, prelude::*, px,
};
use monocode_core::agent::Decision;
use monocode_core::notch::{self as model, Feed, Frame, Look, Notice, Notices, Rest, Shape};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// How long the pointer stays over the closed notch before it opens.
const OPEN_DELAY: Duration = Duration::from_millis(90);
/// How long after the pointer leaves the panel closes.
const CLOSE_DELAY: Duration = Duration::from_millis(260);
/// The outward flare where the notch meets the top edge.
pub(crate) const SHOULDER: f32 = 4.;
/// The notch's corners: the app's square language, softened a little.
const RADIUS: f32 = 4.;
/// How much the closed notch swells under the pointer.
const PEEK: f32 = 14.;
/// Room around the island while it moves, for the spring's overshoot.
const MARGIN: f32 = 12.;
/// The pages' bodies, and the shelf's catalog of widgets.
const SETTINGS_HEIGHT: f32 = 248.;
const AGENTS_HEIGHT: f32 = 236.;
const CHAT_HEIGHT: f32 = 204.;
const CATALOG_HEIGHT: f32 = 26.;
/// The content fades in after the shape starts to move.
const FADE_DELAY: f32 = 0.04;
const FADE: f32 = 0.12;
/// The beat between widgets coming in.
const STAGGER: f32 = 0.03;
/// How far a new page rises as it fades in.
const RISE: f32 = 6.;

/// How the island springs: quicker than it settles, a little lively when it
/// opens and calm when it folds.
#[derive(Clone, Copy)]
struct Spring {
    response: f32,
    damping: f32,
}
const OPENING: Spring = Spring {
    response: 0.34,
    damping: 0.8,
};
const FOLDING: Spring = Spring {
    response: 0.26,
    damping: 1.,
};

/// What the notch asks the app to do. `Decision::Steer` makes the enum
/// large, as in the notifications' actions; boxing it would change the core
/// API for nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
pub(crate) enum NotchEvent {
    OpenSession(i64),
    Decide {
        session: i64,
        request: String,
        decision: Decision,
    },
    NewChat,
    /// Starts a chat with this prompt in the last project.
    Ask(String),
    HangUp,
}

/// The open panel's pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tab {
    Home,
    Agents,
    Chat,
    Settings,
}

impl Tab {
    pub(crate) const ALL: [Tab; 4] = [Tab::Home, Tab::Agents, Tab::Chat, Tab::Settings];

    fn name(self) -> &'static str {
        match self {
            Tab::Home => "Home",
            Tab::Agents => "Agents",
            Tab::Chat => "Chat",
            Tab::Settings => "Settings",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Tab::Home => "dashboard",
            Tab::Agents => "bot",
            Tab::Chat => "chatting",
            Tab::Settings => "sliders",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Tab::Home => "notch-tab-home",
            Tab::Agents => "notch-tab-agents",
            Tab::Chat => "notch-tab-chat",
            Tab::Settings => "notch-tab-settings",
        }
    }
}

/// Saves the look; the app writes it off the interface thread.
pub(crate) type Save = Rc<dyn Fn(String, &mut App)>;

/// The island's rectangle on its way to a shape.
#[derive(Clone, Copy)]
struct Motion {
    /// x, y, width and height, each with its velocity.
    axes: [(f32, f32); 4],
    from: Frame,
    target: Frame,
    spring: Spring,
    last: Instant,
}

impl Motion {
    fn frame(&self) -> Frame {
        Frame {
            x: self.axes[0].0,
            y: self.axes[1].0,
            width: self.axes[2].0,
            height: self.axes[3].0,
        }
    }

    /// One step of a damped spring per axis; true once every axis rests.
    fn step(&mut self, now: Instant) -> bool {
        let dt = now.duration_since(self.last).as_secs_f32().min(1. / 30.);
        self.last = now;
        let stiffness = (std::f32::consts::TAU / self.spring.response).powi(2);
        let friction = 4. * std::f32::consts::PI * self.spring.damping / self.spring.response;
        let target = [
            self.target.x,
            self.target.y,
            self.target.width,
            self.target.height,
        ];
        let mut rest = true;
        for (axis, goal) in self.axes.iter_mut().zip(target) {
            let mut left = dt;
            while left > 0. {
                let h = left.min(1. / 240.);
                let accel = -stiffness * (axis.0 - goal) - friction * axis.1;
                axis.1 += accel * h;
                axis.0 += axis.1 * h;
                left -= h;
            }
            if (axis.0 - goal).abs() > 0.4 || axis.1.abs() > 4. {
                rest = false;
            }
        }
        rest
    }
}

pub(crate) struct Notch {
    pub(crate) look: Look,
    feed: Feed,
    rest: Rest,
    notices: Notices,
    pub(crate) open: bool,
    pub(crate) tab: Tab,
    pub(crate) editing: bool,
    /// The settings' page.
    pub(crate) settings_page: settings::Page,
    hovered: bool,
    hover: Option<Task<()>>,
    expiry: Option<Task<()>>,
    /// Repaints the clock and elapsed times while the panel is open.
    ticker: Option<Task<()>>,
    /// The system's load, sampled while the panel shows the System widget.
    usage: Option<monocode_core::resources::Usage>,
    save: Save,
    /// The Chat page's field.
    pub(crate) field: Entity<Input>,
    /// The island where it rests, and its spring while it moves.
    shown: Option<Frame>,
    motion: Option<Motion>,
    /// The window's frame on the screen.
    placed: Option<Frame>,
    /// When the shown content last changed, for its fade.
    appeared: Option<(Shape, Tab, Instant)>,
    /// What was drawn last, and what stays drawn while the island folds
    /// away from it, fading out.
    drawn: Option<(Shape, Tab, Frame)>,
    leaving: Option<(Shape, Tab, Frame)>,
    /// Tests stand in for the screen.
    pub(crate) screen_override: Option<model::Screen>,
    /// The pointer's comings and goings where GPUI sees no hover.
    _pointer: Option<(platform::PointerWatch, Task<()>)>,
    /// Full pace while the notch is in use (hovered, open, moving or
    /// showing a notice).
    responsive: Option<platform::Responsive>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<NotchEvent> for Notch {}

/// The windows that hold a notch, so closing the workspace window still
/// quits the app.
#[derive(Default)]
struct NotchWindows(std::collections::HashSet<gpui::WindowId>);
impl gpui::Global for NotchWindows {}

/// Whether every open window is a notch (the app has nothing else left).
pub(crate) fn only_notches_left(cx: &App) -> bool {
    let notches = cx.try_global::<NotchWindows>();
    cx.windows()
        .iter()
        .all(|window| notches.is_some_and(|notches| notches.0.contains(&window.window_id())))
}

/// Opens the notch's window on the screen with the camera, or the main one.
pub(crate) fn open(look: Look, save: Save, cx: &mut App) -> Option<WindowHandle<Notch>> {
    let screen = platform::screen(cx);
    let frame = outer(&look, model::frame(&look, &screen.screen, Shape::Quiet));
    let bounds = Bounds {
        origin: point(screen.origin.x + px(frame.x), screen.origin.y + px(frame.y)),
        size: gpui::size(px(frame.width), px(frame.height)),
    };
    let handle = cx
        .open_window(
            gpui::WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                titlebar: None,
                focus: false,
                show: true,
                kind: gpui::WindowKind::PopUp,
                is_movable: false,
                is_resizable: false,
                is_minimizable: false,
                display_id: screen.display,
                window_background: gpui::WindowBackgroundAppearance::Transparent,
                // The spring keeps the display's pace while another app is active.
                inactive_frame_interval: None,
                app_id: Some("dev.usemono.notch".into()),
                ..Default::default()
            },
            move |window, cx| {
                platform::style(window);
                cx.new(|cx| {
                    let mut notch = Notch::new(look, save, cx);
                    notch.watch_pointer(window, cx);
                    notch
                })
            },
        )
        .ok()?;
    cx.default_global::<NotchWindows>()
        .0
        .insert(handle.window_id());
    Some(handle)
}

/// The room the island takes on screen: its shoulders on both sides.
fn outer(look: &Look, frame: Frame) -> Frame {
    if look.island() {
        return frame;
    }
    Frame {
        x: frame.x - SHOULDER,
        width: frame.width + 2. * SHOULDER,
        ..frame
    }
}

/// The smallest frame holding both, with `margin` around it (never above the
/// screen's top edge).
fn union(a: Frame, b: Frame, margin: f32) -> Frame {
    let left = a.x.min(b.x) - margin;
    let top = (a.y.min(b.y) - margin).max(0.);
    let right = (a.x + a.width).max(b.x + b.width) + margin;
    let bottom = (a.y + a.height).max(b.y + b.height) + margin;
    Frame {
        x: left.floor(),
        y: top.floor(),
        width: (right - left).ceil(),
        height: (bottom - top).ceil(),
    }
}

impl Notch {
    pub(crate) fn new(look: Look, save: Save, cx: &mut Context<Self>) -> Self {
        let field = cx.new(|cx| {
            Input::field(
                "Ask an agent",
                "Ask anything — starts a chat in your last project",
                ink(0.4),
                cx,
            )
        });
        let submitted = cx.subscribe(&field, |this, _, _: &input::Submitted, cx| this.ask(cx));
        Self {
            look,
            feed: Feed::default(),
            rest: Rest::default(),
            notices: Notices::default(),
            open: false,
            tab: Tab::Home,
            editing: false,
            settings_page: settings::Page::default(),
            hovered: false,
            hover: None,
            expiry: None,
            ticker: None,
            usage: None,
            save,
            field,
            shown: None,
            motion: None,
            placed: None,
            appeared: None,
            drawn: None,
            leaving: None,
            screen_override: None,
            _pointer: None,
            responsive: None,
            _subscriptions: vec![submitted],
        }
    }

    fn watch_pointer(&mut self, window: &Window, cx: &mut Context<Self>) {
        let (hovered, changes) = async_channel::unbounded();
        let Some(watch) = platform::watch_pointer(window, hovered) else {
            return;
        };

        let task = cx.spawn(async move |this, cx| {
            while let Ok(hovered) = changes.recv().await {
                if this
                    .update(cx, |this, cx| this.hover_changed(hovered, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        self._pointer = Some((watch, task));
    }

    pub(crate) fn feed(&self) -> &Feed {
        &self.feed
    }

    #[cfg(test)]
    pub(crate) fn notice(&self) -> Option<&Notice> {
        self.notices.current()
    }

    /// What the app says now.
    pub(crate) fn update_feed(&mut self, feed: Feed, cx: &mut Context<Self>) {
        if feed == self.feed {
            return;
        }
        self.notices.update(&feed, &self.look, now_ms());
        self.rest = model::rest(&feed);
        self.feed = feed;
        self.schedule_expiry(cx);
        cx.notify();
    }

    fn schedule_expiry(&mut self, cx: &mut Context<Self>) {
        let Some(wait) = self.notices.tick(&self.look, now_ms()) else {
            self.expiry = None;
            return;
        };
        self.expiry = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(wait))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.schedule_expiry(cx);
                cx.notify();
            });
        }));
    }

    pub(crate) fn emit(&mut self, event: NotchEvent, cx: &mut Context<Self>) {
        cx.emit(event);
    }

    /// The Chat page's field: starts a chat with what was typed.
    fn ask(&mut self, cx: &mut Context<Self>) {
        let text = self.field.read(cx).content.trim().to_owned();
        if text.is_empty() {
            return;
        }
        self.field
            .update(cx, |field, cx| field.set_text(String::new(), cx));
        cx.emit(NotchEvent::Ask(text));
        self.set_open(false, cx);
    }

    /// Answers the notice's ask, or opens what it is about.
    pub(crate) fn act_on_notice(&mut self, event: NotchEvent, cx: &mut Context<Self>) {
        self.notices.dismiss(now_ms());
        self.schedule_expiry(cx);
        cx.emit(event);
        cx.notify();
    }

    pub(crate) fn dismiss_notice(&mut self, cx: &mut Context<Self>) {
        self.notices.dismiss(now_ms());
        self.schedule_expiry(cx);
        cx.notify();
    }

    /// A change from the settings or the shelf: kept and saved at once.
    pub(crate) fn change_look(&mut self, change: impl FnOnce(&mut Look), cx: &mut Context<Self>) {
        let mut look = self.look.clone();
        change(&mut look);
        let look = look.normalized();
        if look == self.look {
            return;
        }
        self.look = look;
        (self.save)(model::to_json(&self.look), cx);
        cx.notify();
    }

    pub(crate) fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.editing = false;
        cx.notify();
    }

    pub(crate) fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.open == open {
            return;
        }
        self.open = open;
        self.hover = None;
        if open {
            self.start_ticker(cx);
        } else {
            self.editing = false;
            self.tab = Tab::Home;
            self.ticker = None;
        }
        cx.notify();
    }

    /// The panel's clock: once a second for the clock and elapsed times, and
    /// the system's load right away and every other beat while its widget
    /// shows.
    fn start_ticker(&mut self, cx: &mut Context<Self>) {
        self.ticker = Some(cx.spawn(async move |this, cx| {
            let mut beat = 0u64;
            loop {
                let sample = this
                    .update(cx, |this, _| {
                        this.tab == Tab::Home
                            && this.look.shelf.iter().any(|widget| widget.id == "system")
                    })
                    .unwrap_or(false)
                    && beat.is_multiple_of(2);
                let usage = if sample {
                    Some(
                        cx.background_executor()
                            .spawn(async { monocode_core::resources::usage() })
                            .await,
                    )
                } else {
                    None
                };
                if this
                    .update(cx, |this, cx| {
                        if usage.is_some() {
                            this.usage = usage;
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
                cx.background_executor().timer(Duration::from_secs(1)).await;
                beat += 1;
            }
        }));
    }

    fn hover_changed(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.hovered == hovered && self.hover.is_some() {
            return;
        }
        self.hovered = hovered;
        // The closed notch swells under the pointer.
        cx.notify();
        self.keep_responsive();
        if hovered {
            // A notice is answered where it hangs; the panel opens on a click.
            if self.open || self.look.opens_on_click() || self.notices.current().is_some() {
                self.hover = None;
                return;
            }
            self.hover = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(OPEN_DELAY).await;
                let _ = this.update(cx, |this, cx| {
                    if this.pointer_over() {
                        this.set_open(true, cx);
                    }
                });
            }));
        } else if self.open {
            self.hover = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(CLOSE_DELAY).await;
                let _ = this.update(cx, |this, cx| {
                    if this.pointer_over() {
                        // Still there: whatever said it left was wrong.
                        this.hovered = true;
                    } else {
                        this.set_open(false, cx);
                    }
                });
            }));
        } else {
            self.hover = None;
        }
    }

    /// Whether the pointer is over the notch now: asked of the system where
    /// it is watched, so a stray event never opens or closes the panel.
    fn pointer_over(&self) -> bool {
        match &self._pointer {
            Some((watch, _)) => watch.inside(),
            None => self.hovered,
        }
    }

    fn screen(&self, cx: &App) -> platform::Display {
        match self.screen_override {
            Some(screen) => platform::Display {
                screen,
                ..platform::screen(cx)
            },
            None => platform::screen(cx),
        }
    }

    /// The open page's body.
    fn body_height(&self) -> f32 {
        match self.tab {
            Tab::Settings => SETTINGS_HEIGHT,
            Tab::Agents => AGENTS_HEIGHT,
            Tab::Chat => CHAT_HEIGHT,
            Tab::Home => {
                let rows = if self.editing {
                    self.look.shelf_rows as usize
                } else {
                    model::rows(&self.look.shelf).max(1)
                };
                model::shelf_height(rows)
                    + if self.editing {
                        CATALOG_HEIGHT + model::CELL_GAP
                    } else {
                        0.
                    }
            }
        }
    }

    pub(crate) fn shape(&self) -> Shape {
        if self.open {
            Shape::Open {
                body: self.body_height().ceil() as u32,
            }
        } else if self.notices.current().is_some() {
            Shape::Notice { lines: 2 }
        } else if self.rest.busy() {
            let (left, right) = self.rest.wings(self.look.show_count);
            Shape::Busy { left, right }
        } else {
            Shape::Quiet
        }
    }

    /// The island's frame on the screen for the current shape. Closed under
    /// the pointer it swells a little, to say it will open.
    pub(crate) fn target(&self, cx: &App) -> (platform::Display, Frame) {
        let display = self.screen(cx);
        let shape = self.shape();
        let mut frame = model::frame(&self.look, &display.screen, shape);
        if self.hovered && matches!(shape, Shape::Quiet | Shape::Busy { .. }) {
            frame.x -= PEEK / 2.;
            frame.width += PEEK;
            frame.height += 3.;
        }
        (display, frame)
    }

    /// Springs the island toward `target`; at once under reduced motion.
    fn animate(&mut self, target: Frame, window: &mut Window, cx: &App) -> Frame {
        let now = Instant::now();
        let Some(shown) = self.shown.filter(|_| !cx.reduce_motion()) else {
            self.shown = Some(target);
            self.motion = None;
            return target;
        };
        let mut motion = match self.motion {
            Some(motion) if motion.target == target => motion,
            Some(mut motion) => {
                // A new shape mid-flight keeps the island's speed.
                motion.from = motion.frame();
                motion.target = target;
                motion.spring = spring_for(motion.frame(), target);
                motion
            }
            None if shown == target => return target,
            None => Motion {
                axes: [
                    (shown.x, 0.),
                    (shown.y, 0.),
                    (shown.width, 0.),
                    (shown.height, 0.),
                ],
                from: shown,
                target,
                spring: spring_for(shown, target),
                last: now,
            },
        };
        if motion.step(now) {
            self.motion = None;
            self.shown = Some(target);
            return target;
        }
        window.request_animation_frame();
        self.motion = Some(motion);
        motion.frame()
    }

    /// Puts the window where the island can move: both shapes' room while it
    /// springs, the island's own once it rests. Only moves it when that room
    /// changes, never on every frame.
    fn place(
        &mut self,
        display: &platform::Display,
        target: Frame,
        window: &mut Window,
        cx: &mut App,
    ) -> Frame {
        let wanted = match self.motion {
            Some(motion) => union(
                outer(&self.look, motion.from),
                outer(&self.look, target),
                MARGIN,
            ),
            None => outer(&self.look, target),
        };
        let placed = match self.placed {
            // Mid-flight the room only grows; it shrinks when the island rests.
            Some(placed) if self.motion.is_some() => union(placed, wanted, 0.),
            _ => wanted,
        };
        if self.placed != Some(placed) {
            platform::place(window, cx, display, placed);
            self.placed = Some(placed);
        }
        placed
    }

    fn camera(&self, cx: &App) -> Option<model::Camera> {
        if self.look.island() {
            return None;
        }
        model::camera(&self.screen(cx).screen)
    }

    /// The content's opacity: it fades in just after the shape changes.
    fn fade(&mut self, shape: Shape, window: &mut Window, cx: &App) -> f32 {
        let now = Instant::now();
        let same = |s: Shape| std::mem::discriminant(&s) == std::mem::discriminant(&shape);
        let changed = !matches!(self.appeared, Some((s, t, _)) if same(s) && t == self.tab);
        if changed {
            let first = self.appeared.is_none();
            self.appeared = Some((
                shape,
                self.tab,
                if first {
                    now - Duration::from_secs(1)
                } else {
                    now
                },
            ));
        }
        if cx.reduce_motion() {
            return 1.;
        }
        let since = self
            .appeared
            .map_or(1., |(_, _, at)| now.duration_since(at).as_secs_f32());
        let opacity = ((since - FADE_DELAY) / FADE).clamp(0., 1.);
        // The shelf's widgets come in one after another, a little later.
        if since < FADE_DELAY + FADE + STAGGER * model::WIDGETS.len() as f32 {
            window.request_animation_frame();
        }
        opacity
    }

    /// A widget's opacity as the shelf comes in, the `index`th a beat later.
    pub(crate) fn reveal(&self, index: usize, cx: &App) -> f32 {
        if cx.reduce_motion() {
            return 1.;
        }
        let since = self
            .appeared
            .map_or(1., |(_, _, at)| at.elapsed().as_secs_f32());
        ((since - FADE_DELAY - STAGGER * index as f32) / FADE).clamp(0., 1.)
    }
}

fn spring_for(from: Frame, to: Frame) -> Spring {
    if to.width * to.height >= from.width * from.height {
        OPENING
    } else {
        FOLDING
    }
}

fn now_ms() -> u64 {
    monocode_core::time::now_ms()
}

/// The notch's outline: a short flare out to the top edge and nearly square
/// corners below, or a box with every corner softened for the island.
fn outline(bounds: Bounds<Pixels>, island: bool) -> Vec<gpui::Point<Pixels>> {
    let (x0, y0) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let mut points = Vec::new();
    let mut arc = |cx: f32, cy: f32, r: f32, from: f32, to: f32| {
        const STEPS: usize = 4;
        for step in 0..=STEPS {
            let angle = (from + (to - from) * step as f32 / STEPS as f32).to_radians();
            points.push(point(
                px(x0 + cx + r * angle.cos()),
                px(y0 + cy + r * angle.sin()),
            ));
        }
    };
    let r = RADIUS.min(h / 2.).min(w / 2.);
    if island {
        arc(r, r, r, 180., 270.);
        arc(w - r, r, r, 270., 360.);
        arc(w - r, h - r, r, 0., 90.);
        arc(r, h - r, r, 90., 180.);
    } else {
        let s = SHOULDER;
        let r = r.min((w - 2. * s) / 2.);
        arc(0., s, s, 270., 360.);
        arc(s + r, h - r, r, 180., 90.);
        arc(w - s - r, h - r, r, 90., 0.);
        arc(w, s, s, 180., 270.);
    }
    points
}

/// The black body, with a hairline along its edge so it reads as a notch
/// over dark wallpapers too. At rest over the camera it has none: it is the
/// hardware.
fn body(island: bool, edge: bool) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let points = outline(bounds, island);
            let mut fill = gpui::PathBuilder::fill();
            fill.add_polygon(&points, true);
            if let Ok(path) = fill.build() {
                window.paint_path(path, black(1.));
            }
            if !edge {
                return;
            }
            let mut edge = gpui::PathBuilder::stroke(px(1.));
            // The island's whole edge; the notch's sides and bottom only.
            let edge_points: Vec<_> = if island {
                points
                    .iter()
                    .copied()
                    .chain(points.first().copied())
                    .collect()
            } else {
                points[4..points.len() - 4].to_vec()
            };
            edge.add_polygon(&edge_points, false);
            if let Ok(path) = edge.build() {
                window.paint_path(path, ink(0.12));
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Ink on the notch's black, in any theme.
pub(crate) fn ink(alpha: f32) -> gpui::Hsla {
    white(alpha)
}

/// A small text button on the notch's black.
pub(crate) fn pill(
    id: impl Into<gpui::ElementId>,
    label: &str,
    primary: bool,
) -> gpui::Stateful<gpui::Div> {
    let id = id.into();
    let selector = format!("{id}");
    div()
        .id(id)
        .debug_selector(move || selector.clone())
        .role(Role::Button)
        .aria_label(label.to_owned())
        .h(px(22.))
        .px(px(9.))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(11.5))
        .cursor_pointer()
        .when(primary, |pill| {
            pill.bg(accent_color())
                .text_color(white(1.))
                .hover(|style| style.opacity(0.88))
        })
        .when(!primary, |pill| {
            pill.bg(ink(0.1))
                .text_color(ink(0.85))
                .hover(|style| style.bg(ink(0.18)))
        })
        .child(label.to_owned())
}

/// An icon button on the notch's black.
pub(crate) fn glyph(
    id: &'static str,
    label: &'static str,
    path: &'static str,
    on: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .role(Role::Button)
        .aria_label(label)
        .size(px(26.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .when(on, |button| button.bg(ink(0.14)))
        .hover(|style| style.bg(ink(0.1)))
        .child(icon(path, 14.).text_color(ink(if on { 1. } else { 0.55 })))
}

/// What the agents are doing, as the sidebar marks a session: the dot
/// matrix wave while they work (still under reduced motion), the attention
/// mark while one waits for the user.
pub(crate) fn work_mark(id: &'static str, asking: bool, cx: &App) -> AnyElement {
    if asking {
        return icon("alert-circle", 13.)
            .text_color(accent_color())
            .into_any_element();
    }
    if cx.reduce_motion() {
        return crate::session_mark::spinner_at(0.3).into_any_element();
    }
    crate::session_mark::spinner(id, crate::pulse::Rate::Smooth).into_any_element()
}

/// One session's mark: its work, its question, or its finish.
pub(crate) fn activity_mark(activity: &model::Activity, cx: &App) -> AnyElement {
    if activity.done {
        icon("check-circle-2", 13.)
            .text_color(ink(0.7))
            .into_any_element()
    } else if activity.needs_you {
        work_mark("notch-activity-ask", true, cx)
    } else {
        div()
            .id(("notch-activity", activity.id as u64))
            .child(work_mark("notch-activity-working", false, cx))
            .into_any_element()
    }
}

impl Notch {
    /// The ears beside the camera, or the bar where there is none.
    /// The wings beside the camera, or the bar where there is none: on the
    /// left the work mark and a face per session at work (and "+N"), on the
    /// right who leads, what it does and how far along its list it is.
    fn ears(&self, camera: Option<model::Camera>, width: f32, cx: &App) -> AnyElement {
        let rest = &self.rest;
        let mut faces = div().flex().items_center();
        for (at, provider) in rest.marks.iter().enumerate() {
            faces = faces.child(
                div()
                    .when(at > 0, |face| face.ml(px(-4.)))
                    .size(px(17.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(black(1.))
                    .child(provider_glyph(provider, 13.)),
            );
        }
        let left = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(work_mark("notch-working", rest.asking > 0, cx))
            .child(faces)
            .when(self.look.show_count && rest.more() > 0, |left| {
                left.child(
                    div()
                        .text_size(px(10.5))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(ink(0.6))
                        .child(format!("+{}", rest.more())),
                )
            });
        let right: AnyElement = match (&rest.lead_text, rest.lead_steps) {
            (Some(text), steps) => {
                let line = div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(11.))
                            .text_color(ink(0.88))
                            .child(text.clone()),
                    )
                    .children(steps.map(|(done, all)| {
                        div()
                            .flex_shrink_0()
                            .text_size(px(10.5))
                            .text_color(ink(0.5))
                            .child(format!("{done}/{all}"))
                    }));
                div()
                    .min_w_0()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(line)
                    .children(steps.map(|(done, all)| {
                        let share = done as f32 / all.max(1) as f32;
                        div()
                            .h(px(2.))
                            .w_full()
                            .bg(ink(0.12))
                            .child(div().h_full().w(gpui::relative(share)).bg(accent_color()))
                    }))
                    .into_any_element()
            }
            (None, _) => match (&rest.lead, rest.call) {
                (_, true) => icon("mic-01", 12.).text_color(ink(0.9)).into_any_element(),
                (Some(provider), _) => provider_glyph(provider, 13.),
                _ => div().into_any_element(),
            },
        };
        match camera {
            Some(camera) => {
                let ear = ((width - camera.width) / 2.).max(0.);
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .child(div().w(px(ear)).pl(px(12.)).flex().child(left))
                    .child(div().w(px(camera.width)).flex_shrink_0())
                    .child(
                        div()
                            .w(px(ear))
                            .min_w_0()
                            .pl(px(10.))
                            .pr(px(12.))
                            .flex()
                            .child(right),
                    )
                    .into_any_element()
            }
            None => div()
                .size_full()
                .px(px(12.))
                .flex()
                .items_center()
                .gap(px(10.))
                .when(rest.busy(), |bar| {
                    bar.child(div().flex_shrink_0().child(left))
                        .child(div().flex_1().min_w_0().flex().child(right))
                })
                .into_any_element(),
        }
    }

    fn notice_view(&self, notice: &Notice, cx: &mut Context<Self>) -> AnyElement {
        let (provider, title, label) = match notice {
            Notice::Ask(ask) => (
                ask.provider.as_str(),
                ask.title.clone(),
                if ask.approval {
                    format!("Wants to run {}", ask.label)
                } else {
                    ask.label.clone()
                },
            ),
            Notice::Done {
                provider, title, ..
            } => (provider.as_str(), title.clone(), "Done".to_owned()),
        };
        let actions = match notice.clone() {
            Notice::Ask(ask) if ask.approval => {
                let (allow, deny) = (ask.clone(), ask.clone());
                div()
                    .flex()
                    .gap(px(6.))
                    .child(pill("notch-deny", "Deny", false).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.act_on_notice(
                                NotchEvent::Decide {
                                    session: deny.session,
                                    request: deny.request.clone(),
                                    decision: Decision::Deny,
                                },
                                cx,
                            )
                        },
                    )))
                    .child(pill("notch-allow", "Allow", true).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.act_on_notice(
                                NotchEvent::Decide {
                                    session: allow.session,
                                    request: allow.request.clone(),
                                    decision: Decision::Allow,
                                },
                                cx,
                            )
                        },
                    )))
            }
            Notice::Ask(ask) => div().child(pill("notch-answer", "Answer", true).on_click(
                cx.listener(move |this, _, _, cx| {
                    this.act_on_notice(NotchEvent::OpenSession(ask.session), cx)
                }),
            )),
            Notice::Done { session, .. } => div()
                .flex()
                .gap(px(6.))
                .child(
                    pill("notch-dismiss", "Dismiss", false)
                        .on_click(cx.listener(|this, _, _, cx| this.dismiss_notice(cx))),
                )
                .child(pill("notch-open", "Open", true).on_click(cx.listener(
                    move |this, _, _, cx| this.act_on_notice(NotchEvent::OpenSession(session), cx),
                ))),
        };
        div()
            .debug_selector(|| "notch-notice".into())
            .px(px(14.))
            .pb(px(10.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .child(provider_glyph(provider, 14.))
                    .child(
                        div()
                            .min_w_0()
                            .flex_shrink_0()
                            .max_w(px(180.))
                            .truncate()
                            .text_size(px(12.5))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(ink(0.95))
                            .child(title),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(ink(0.55))
                            .child(label),
                    ),
            )
            .child(div().flex().justify_end().child(actions))
            .into_any_element()
    }

    /// The panel's header: the page's name, what is going on, and the pages.
    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        let summary = match (self.rest.working, self.rest.asking) {
            (0, 0) => String::new(),
            (working, 0) => format!("{working} working"),
            (0, asking) => format!("{asking} need you"),
            (working, asking) => format!("{working} working · {asking} need you"),
        };
        let title = match self.tab {
            Tab::Home if self.editing => "Arrange widgets",
            tab => tab.name(),
        };
        let mut tabs = div().flex().items_center().gap(px(2.));
        for tab in Tab::ALL {
            tabs = tabs.child(
                glyph(tab.id(), tab.name(), tab.icon(), self.tab == tab)
                    .on_click(cx.listener(move |this, _, _, cx| this.set_tab(tab, cx))),
            );
        }
        div()
            .h(px(model::PANEL_HEADER))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(px(13.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(ink(0.95))
                    .child(title),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.5))
                    .text_color(ink(0.45))
                    .child(summary),
            )
            .when(self.tab == Tab::Home, |header| {
                header.child(
                    glyph("notch-edit", "Arrange widgets", "pencil-edit", self.editing).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.editing = !this.editing;
                            cx.notify();
                        }),
                    ),
                )
            })
            .child(div().w(px(1.)).h(px(14.)).bg(ink(0.12)))
            .child(tabs)
            .into_any_element()
    }

    /// What the island holds for `shape`, laid out at its resting size.
    fn content(
        &mut self,
        shape: Shape,
        tab: Tab,
        size: Frame,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let camera = self.camera(cx);
        let top = camera.map_or(0., |camera| camera.height);
        match shape {
            Shape::Quiet => div().into_any_element(),
            Shape::Busy { .. } => self.ears(camera, size.width, cx),
            Shape::Notice { .. } => {
                let notice = self.notices.current().cloned();
                let ears = self.ears(camera, size.width, cx);
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(div().h(px(top.max(8.))).flex_shrink_0().child(ears))
                    .children(notice.map(|notice| self.notice_view(&notice, cx)))
                    .into_any_element()
            }
            Shape::Open { .. } => {
                let inner = size.width - 2. * model::PANEL_PAD;
                let page = match tab {
                    Tab::Home => self.shelf_view(size.width, cx),
                    Tab::Agents => self.agents_page(inner, cx),
                    Tab::Chat => self.chat_page(cx),
                    Tab::Settings => self.settings_view(cx),
                };
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(div().h(px(top)).flex_shrink_0())
                    .child(
                        div()
                            .px(px(model::PANEL_PAD))
                            .flex()
                            .flex_col()
                            .child(self.header(cx))
                            .child(page),
                    )
                    .into_any_element()
            }
        }
    }
}

impl Notch {
    /// In use, the app keeps its full pace; at rest it lets the system nap.
    fn keep_responsive(&mut self) {
        let in_use =
            self.hovered || self.open || self.motion.is_some() || self.notices.current().is_some();
        if in_use && self.responsive.is_none() {
            self.responsive = platform::responsive();
        } else if !in_use {
            self.responsive = None;
        }
    }
}

impl Render for Notch {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (display, target) = self.target(cx);
        let island = self.animate(target, window, cx);
        self.keep_responsive();
        let room = self.place(&display, target, window, cx);
        let shape = self.shape();
        // While the island folds, what it showed stays and fades out with
        // the first half of the fold, instead of vanishing at once.
        let folding = self.motion.is_some_and(|motion| {
            motion.target.width * motion.target.height < motion.from.width * motion.from.height
        });
        if !folding {
            self.leaving = None;
        } else if self.leaving.is_none() {
            self.leaving = self.drawn.filter(|(drawn, _, _)| *drawn != shape);
        }
        let (drawn_shape, drawn_tab, size, opacity, rise) = match self.leaving {
            Some((old, tab, size)) => {
                let span = (size.height - target.height).max(1.);
                let folded = ((size.height - island.height) / span).clamp(0., 1.);
                (old, tab, size, (1. - folded / 0.55).clamp(0., 1.), 0.)
            }
            None => {
                let opacity = self.fade(shape, window, cx);
                self.drawn = Some((shape, self.tab, target));
                (shape, self.tab, target, opacity, (1. - opacity) * RISE)
            }
        };
        let content = self.content(drawn_shape, drawn_tab, size, cx);
        let target = size;
        let is_island = self.look.island();
        let shoulder = if is_island { 0. } else { SHOULDER };
        // The island in the window, and its content centered in it at the
        // size it will rest at, so nothing reflows while it springs.
        let left = island.x - room.x - shoulder;
        let top = island.y - room.y;
        let clickable = !self.open && self.notices.current().is_none();
        div()
            .size_full()
            .relative()
            .font_family(crate::fonts::UI)
            .child(
                div()
                    .id("notch")
                    .debug_selector(|| "notch".into())
                    .role(Role::Group)
                    .aria_label("MonoBOT notch")
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(island.width + 2. * shoulder))
                    .h(px(island.height))
                    // Where the system's pointer is watched, GPUI's own hover
                    // is stale while another app is active: it would reopen
                    // the panel when the window moves.
                    .when(self._pointer.is_none(), |notch| {
                        notch.on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                            this.hover_changed(*hovered, cx)
                        }))
                    })
                    .when(clickable, |notch| {
                        notch
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| this.set_open(true, cx)))
                    })
                    .child(body(
                        is_island,
                        self.look.edge && (shape != Shape::Quiet || is_island),
                    ))
                    .child(
                        div()
                            .absolute()
                            .left(px(shoulder))
                            .top_0()
                            .w(px(island.width))
                            .h(px(island.height))
                            .overflow_hidden()
                            .child(
                                div()
                                    .absolute()
                                    .left(px((island.width - target.width) / 2.))
                                    .top(px(rise))
                                    .w(px(target.width))
                                    .h(px(target.height))
                                    .opacity(opacity)
                                    .child(content),
                            ),
                    ),
            )
    }
}

/// The notch the app shows, so the app can feed it.
#[derive(Clone)]
pub(crate) struct Handle {
    pub entity: Entity<Notch>,
}

impl Handle {
    pub(crate) fn from_window(window: WindowHandle<Notch>, cx: &mut App) -> Option<Self> {
        let entity = window.update(cx, |_, _, cx| cx.entity()).ok()?;
        Some(Self { entity })
    }
}

/// Reads the system's reduced motion setting into the app, so every view
/// draws its final state when the user asked for less motion.
pub(crate) fn follow_reduced_motion(cx: &mut App) {
    if let Some(reduce) = platform::reduce_motion() {
        cx.set_reduce_motion(reduce);
    }
}
