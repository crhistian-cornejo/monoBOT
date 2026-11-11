//! The Graph tab: members as nodes, @mentions as edges whose width grows
//! with their count, and a timeline that replays the board post by post or
//! follows it live. The newest post's edge carries one pulse when it lands;
//! with reduced motion it is drawn highlighted instead. Painting happens in a
//! single canvas; the scene is computed from the board in `render`.
use super::{SwarmPanel, live_color, post_header, rich_text};
use crate::{right_panel::tabular, theme::*};
use gpui::{
    AnyElement, App, BorderStyle, Bounds, Context, DispatchPhase, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels, Point, SharedString, Task, Window, canvas,
    div, fill, point, prelude::*, px, quad, size,
};
use monocode_core::orchestration::board::{self, Board, Edge};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

/// Where the last paint put each node, by member index.
type Nodes = Rc<RefCell<Vec<(Bounds<Pixels>, usize)>>>;

/// How long the newest edge's pulse travels.
const PULSE: Duration = Duration::from_millis(650);
/// One post per step when replaying at even pacing, at 1×.
const EVEN_STEP: Duration = Duration::from_millis(700);
const NODE_HEIGHT: f32 = 26.;

pub(crate) struct Timeline {
    /// Posts drawn: the first `cursor` of them.
    pub(crate) cursor: usize,
    /// Follow new posts as they land.
    pub(crate) live: bool,
    pub(crate) playing: bool,
    /// 1, 2 or 4.
    pub(crate) speed: u32,
    /// Replay with the real gaps between posts (clamped) instead of evenly.
    pub(crate) real_time: bool,
    pub(crate) hide_inactive: bool,
    pub(crate) expand_all: bool,
    player: Option<Task<()>>,
    /// When the latest step's edge started its pulse.
    pulse: Option<Instant>,
    slider: Rc<Cell<Option<Bounds<Pixels>>>>,
    dragging: bool,
    /// Where the last paint put each node, by member index, for clicks.
    nodes: Nodes,
    /// The member whose node is under the pointer.
    pub(crate) hover: Option<usize>,
}
impl Default for Timeline {
    fn default() -> Self {
        Self {
            cursor: 0,
            live: true,
            playing: false,
            speed: 1,
            real_time: false,
            hide_inactive: false,
            expand_all: true,
            player: None,
            pulse: None,
            slider: Rc::default(),
            dragging: false,
            nodes: Rc::default(),
            hover: None,
        }
    }
}
impl Timeline {
    /// The board now has `len` posts; a live timeline moves to the end.
    pub(crate) fn follow(&mut self, len: usize, new_post: bool, _: &mut App) {
        if self.live && !self.playing {
            if new_post && len > self.cursor {
                self.pulse = Some(Instant::now());
            }
            self.cursor = len;
        } else {
            self.cursor = self.cursor.min(len);
        }
    }
    /// Where the last paint put each member's node.
    #[cfg(test)]
    pub(crate) fn node_bounds(&self) -> Vec<(Bounds<Pixels>, usize)> {
        self.nodes.borrow().clone()
    }
    #[cfg(test)]
    pub(crate) fn sliding(&self) -> bool {
        self.dragging
    }
    pub(crate) fn pause(&mut self) {
        self.playing = false;
        self.player = None;
    }
    /// How long to hold the current post before the next one.
    pub(crate) fn delay(&self, board: &Board) -> Duration {
        let even = EVEN_STEP / self.speed.max(1);
        if !self.real_time || self.cursor == 0 {
            return even;
        }
        let gap = match (
            board.posts.get(self.cursor.saturating_sub(1)),
            board.posts.get(self.cursor),
        ) {
            (Some(a), Some(b)) => Duration::from_millis(b.at.saturating_sub(a.at)),
            _ => even,
        };
        (gap / self.speed.max(1)).clamp(Duration::from_millis(150), Duration::from_secs(3))
    }
}

/// What the canvas draws.
struct Scene {
    labels: Vec<SharedString>,
    active: Vec<bool>,
    silent: Vec<bool>,
    /// Member indices drawn, in layout order.
    visible: Vec<usize>,
    edges: Vec<Edge>,
    /// The post at the cursor: its edges are highlighted.
    current: u64,
    pulse: Option<Instant>,
    hover: Option<usize>,
    nodes: Nodes,
}

fn unit(v: Point<f32>) -> Point<f32> {
    let len = (v.x * v.x + v.y * v.y).sqrt().max(0.001);
    point(v.x / len, v.y / len)
}

/// Distance from a node's centre to its edge along `dir`.
fn exit(half: Point<f32>, dir: Point<f32>) -> f32 {
    let tx = if dir.x.abs() > 1e-4 {
        half.x / dir.x.abs()
    } else {
        f32::MAX
    };
    let ty = if dir.y.abs() > 1e-4 {
        half.y / dir.y.abs()
    } else {
        f32::MAX
    };
    tx.min(ty)
}

fn quad_at(a: Point<f32>, c: Point<f32>, b: Point<f32>, t: f32) -> Point<f32> {
    let u = 1. - t;
    point(
        u * u * a.x + 2. * u * t * c.x + t * t * b.x,
        u * u * a.y + 2. * u * t * c.y + t * t * b.y,
    )
}

fn px_point(p: Point<f32>) -> Point<Pixels> {
    point(px(p.x), px(p.y))
}

fn paint(bounds: Bounds<Pixels>, scene: &Scene, window: &mut Window, cx: &mut App) {
    let count = scene.visible.len();
    if count == 0 {
        return;
    }
    let mut font = window.text_style().font();
    font.weight = gpui::FontWeight::MEDIUM;
    let font_size = px(12.);
    let lines: Vec<_> = scene
        .visible
        .iter()
        .map(|&ix| {
            window.text_system().shape_line(
                scene.labels[ix].clone(),
                font_size,
                &[gpui::TextRun {
                    len: scene.labels[ix].len(),
                    font: font.clone(),
                    color: content(0.9),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
                None,
            )
        })
        .collect();
    let widths: Vec<f32> = lines.iter().map(|l| f32::from(l.width) + 34.).collect();
    let widest = widths.iter().copied().fold(0., f32::max);
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let (pad_x, pad_y) = ((widest / 2. + 6.).min(w / 2.), NODE_HEIGHT);
    let origin = point(f32::from(bounds.left()), f32::from(bounds.top()));
    let layout = board::layout(count);
    let center = |slot: usize| {
        let (nx, ny) = layout[slot];
        point(
            origin.x + pad_x + nx * (w - 2. * pad_x).max(0.),
            origin.y + pad_y + ny * (h - 2. * pad_y).max(0.),
        )
    };
    let slot_of = |member: usize| scene.visible.iter().position(|v| *v == member);
    let half = |slot: usize| point(widths[slot] / 2. + 3., NODE_HEIGHT / 2. + 3.);

    let progress = scene.pulse.map(|start| {
        let t = start.elapsed().as_secs_f32() / PULSE.as_secs_f32();
        if t < 1. && !cx.reduce_motion() {
            window.request_animation_frame();
        }
        t
    });

    for edge in &scene.edges {
        let (Some(from), Some(to)) = (slot_of(edge.from), slot_of(edge.to)) else {
            continue;
        };
        let (a, b) = (center(from), center(to));
        let d = point(b.x - a.x, b.y - a.y);
        let len = (d.x * d.x + d.y * d.y).sqrt().max(1.);
        // Both directions between two members bow to opposite sides.
        let normal = point(-d.y / len, d.x / len);
        let ctrl = point(
            (a.x + b.x) / 2. + normal.x * len * 0.16,
            (a.y + b.y) / 2. + normal.y * len * 0.16,
        );
        let out = unit(point(ctrl.x - a.x, ctrl.y - a.y));
        let inward = unit(point(ctrl.x - b.x, ctrl.y - b.y));
        let start = point(
            a.x + out.x * exit(half(from), out),
            a.y + out.y * exit(half(from), out),
        );
        let end = point(
            b.x + inward.x * exit(half(to), inward),
            b.y + inward.y * exit(half(to), inward),
        );
        let current = edge.last == scene.current;
        let width = 1. + (edge.count.min(6) as f32 - 1.) * 0.7 + if current { 0.8 } else { 0. };
        let color = content(if current { 0.85 } else { 0.24 });
        let mut path = PathBuilder::stroke(px(width));
        if edge.via_all {
            path = path.dash_array(&[px(4.), px(4.)]);
        }
        path.move_to(px_point(start));
        path.curve_to(px_point(end), px_point(ctrl));
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
        }
        // Arrowhead along the curve's last tangent.
        let dir = unit(point(end.x - ctrl.x, end.y - ctrl.y));
        let side = point(-dir.y, dir.x);
        let back = point(end.x - dir.x * 7., end.y - dir.y * 7.);
        let mut head = PathBuilder::fill();
        head.move_to(px_point(end));
        head.line_to(px_point(point(
            back.x + side.x * 3.5,
            back.y + side.y * 3.5,
        )));
        head.line_to(px_point(point(
            back.x - side.x * 3.5,
            back.y - side.y * 3.5,
        )));
        head.close();
        if let Ok(head) = head.build() {
            window.paint_path(head, color);
        }
        if current && let Some(t) = progress.filter(|t| *t < 1. && !cx.reduce_motion()) {
            let at = quad_at(start, ctrl, end, t);
            let dot = px(4.);
            window.paint_quad(
                fill(
                    Bounds::new(px_point(at) - point(dot, dot), size(dot * 2., dot * 2.)),
                    content(0.95),
                )
                .corner_radii(dot),
            );
        }
    }

    let surface = pane_fill();
    let mut nodes = scene.nodes.borrow_mut();
    nodes.clear();
    for (slot, &member) in scene.visible.iter().enumerate() {
        let c = center(slot);
        let (bw, bh) = (widths[slot], NODE_HEIGHT);
        let node = Bounds::new(
            point(px(c.x - bw / 2.), px(c.y - bh / 2.)),
            size(px(bw), px(bh)),
        );
        nodes.push((node, member));
        let involved = scene
            .edges
            .iter()
            .any(|e| e.last == scene.current && (e.from == member || e.to == member));
        let hovered = scene.hover == Some(member);
        window.paint_quad(quad(
            node,
            px(bh / 2.),
            if hovered { content(0.08) } else { surface },
            px(1.),
            content(if involved || hovered { 0.85 } else { 0.25 }),
            BorderStyle::Solid,
        ));
        if scene.silent[member] {
            let ring = Bounds::new(
                node.origin - point(px(4.), px(4.)),
                node.size + size(px(8.), px(8.)),
            );
            window.paint_quad(quad(
                ring,
                px(bh / 2. + 4.),
                gpui::transparent_black(),
                px(1.),
                content(0.3),
                BorderStyle::Dashed,
            ));
        }
        let dot = px(3.5);
        let dot_center = point(node.left() + px(13.), node.top() + px(bh / 2.));
        let dot_bounds = Bounds::new(dot_center - point(dot, dot), size(dot * 2., dot * 2.));
        if scene.active[member] {
            window.paint_quad(fill(dot_bounds, live_color()).corner_radii(dot));
        } else {
            window.paint_quad(quad(
                dot_bounds,
                dot,
                gpui::transparent_black(),
                px(1.2),
                content(0.55),
                BorderStyle::Solid,
            ));
        }
        let line = &lines[slot];
        let _ = line.paint(
            point(node.left() + px(22.), node.top() + px((bh - 16.) / 2.)),
            px(16.),
            gpui::TextAlign::Left,
            None,
            window,
            cx,
        );
    }
}

fn chip(id: &'static str, label: impl Into<SharedString>, on: bool) -> gpui::Stateful<gpui::Div> {
    let label = label.into();
    div()
        .id(id)
        .debug_selector(move || id.into())
        .role(gpui::Role::Switch)
        .aria_label(label.clone())
        .aria_toggled(if on {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .cursor_pointer()
        .flex_shrink_0()
        .px_2()
        .py(px(3.))
        .rounded(px(6.))
        .text_size(px(11.5))
        .text_color(content(if on { 0.95 } else { 0.5 }))
        .when(on, |d| d.bg(content(0.1)))
        .hover(|s| s.text_color(content(1.)))
        .child(label)
}

impl SwarmPanel {
    fn step(&mut self, delta: isize, cx: &mut Context<Self>) -> bool {
        let len = self.board.posts.len();
        let to = (self.timeline.cursor as isize + delta).clamp(0, len as isize) as usize;
        let moved = to != self.timeline.cursor;
        if to > self.timeline.cursor {
            self.timeline.pulse = Some(Instant::now());
        }
        self.timeline.cursor = to;
        self.timeline.live = to == len && delta > 0 && !self.timeline.playing;
        cx.notify();
        moved
    }

    fn seek(&mut self, cursor: usize, cx: &mut Context<Self>) {
        self.timeline.pause();
        let len = self.board.posts.len();
        self.timeline.cursor = cursor.min(len);
        self.timeline.live = self.timeline.cursor == len;
        self.timeline.pulse = None;
        cx.notify();
    }

    fn go_live(&mut self, cx: &mut Context<Self>) {
        self.timeline.pause();
        self.timeline.live = true;
        self.timeline.cursor = self.board.posts.len();
        cx.notify();
    }

    fn toggle_play(&mut self, cx: &mut Context<Self>) {
        if self.timeline.playing {
            self.timeline.pause();
            cx.notify();
            return;
        }
        let len = self.board.posts.len();
        if len == 0 {
            return;
        }
        if self.timeline.cursor >= len {
            self.timeline.cursor = 0;
        }
        self.timeline.live = false;
        self.timeline.playing = true;
        self.timeline.player = Some(cx.spawn(async move |this, cx| {
            loop {
                let Ok(delay) = this.update(cx, |this, _| this.timeline.delay(&this.board)) else {
                    return;
                };
                cx.background_executor().timer(delay).await;
                let more = this
                    .update(cx, |this, cx| {
                        if !this.timeline.playing {
                            return false;
                        }
                        this.step(1, cx);
                        let done = this.timeline.cursor >= this.board.posts.len();
                        if done {
                            this.timeline.playing = false;
                            this.timeline.live = true;
                        }
                        !done
                    })
                    .unwrap_or(false);
                if !more {
                    return;
                }
            }
        }));
        cx.notify();
    }

    fn slide_to(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let Some(bounds) = self.timeline.slider.get() else {
            return;
        };
        let width = f32::from(bounds.size.width).max(1.);
        let fraction = (f32::from(x - bounds.left()) / width).clamp(0., 1.);
        let len = self.board.posts.len();
        self.seek((fraction * len as f32).round() as usize, cx);
    }

    fn scene(&self) -> Scene {
        let board = &self.board;
        let cursor = self.timeline.cursor.min(board.posts.len());
        let graph = board.graph(cursor, self.timeline.expand_all);
        let visible = (0..board.members.len())
            .filter(|ix| *ix == 0 || !self.timeline.hide_inactive || graph.involved[*ix])
            .collect();
        Scene {
            labels: board
                .members
                .iter()
                .map(|m| SharedString::from(m.handle.clone()))
                .collect(),
            active: board.members.iter().map(|m| m.state.active()).collect(),
            silent: graph.silent,
            visible,
            edges: graph.edges,
            current: cursor
                .checked_sub(1)
                .and_then(|ix| board.posts.get(ix))
                .map_or(0, |p| p.seq),
            pulse: self.timeline.pulse,
            hover: self.timeline.hover,
            nodes: self.timeline.nodes.clone(),
        }
    }

    /// The member whose node holds `position`, as last painted.
    fn node_at(&self, position: Point<Pixels>) -> Option<usize> {
        self.timeline
            .nodes
            .borrow()
            .iter()
            .find(|(bounds, _)| bounds.contains(&position))
            .map(|(_, member)| *member)
    }

    fn hover_node(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let hover = self.node_at(position);
        if self.timeline.hover != hover {
            self.timeline.hover = hover;
            cx.notify();
        }
    }

    /// A click on a node opens that member's chat.
    fn click_node(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if let Some(handle) = self
            .node_at(position)
            .and_then(|ix| self.board.members.get(ix))
            .map(|m| m.handle.clone())
        {
            self.open_member(&handle, cx);
        }
    }

    fn stop_sliding(&mut self, cx: &mut Context<Self>) {
        if self.timeline.dragging {
            self.timeline.dragging = false;
            cx.notify();
        }
    }

    pub(super) fn graph_tab(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let len = self.board.posts.len();
        let cursor = self.timeline.cursor.min(len);
        let current = cursor
            .checked_sub(1)
            .and_then(|ix| self.board.posts.get(ix));
        let first = self.board.posts.first().map_or(0, |p| p.at);
        let info = match current {
            Some(post) => format!(
                "{cursor} of {len} · #{} at {} ({})",
                post.seq,
                board::clock(post.at),
                board::since(first, post.at)
            ),
            None => format!("0 of {len}"),
        };
        let fraction = if len == 0 {
            0.
        } else {
            cursor as f32 / len as f32
        };
        let slider = self.timeline.slider.clone();
        let panel = cx.entity().downgrade();
        let dragging = self.timeline.dragging;
        let playing = self.timeline.playing;
        let live = self.timeline.live && cursor == len;
        let scene = self.scene();
        let all_posts = self.board.graph(cursor, false).all_posts;
        let controls = div()
            .px_3()
            .pt_2()
            .flex()
            .items_center()
            .gap_1()
            .child(
                icon_button("swarm-first", "First post", "media-previous")
                    .on_click(cx.listener(|this, _, _, cx| this.seek(0, cx))),
            )
            .child(
                icon_button("swarm-back", "Previous post", "chevron-left").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.timeline.pause();
                        this.step(-1, cx);
                    },
                )),
            )
            .child(
                icon_button(
                    "swarm-play",
                    if playing { "Pause" } else { "Replay" },
                    if playing { "media-pause" } else { "media-play" },
                )
                .bg(content(0.06))
                .on_click(cx.listener(|this, _, _, cx| this.toggle_play(cx))),
            )
            .child(
                icon_button("swarm-next", "Next post", "chevron-right").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.timeline.pause();
                        this.step(1, cx);
                    },
                )),
            )
            .child(
                div()
                    .id("swarm-timeline")
                    .debug_selector(|| "swarm-timeline".into())
                    .role(gpui::Role::Slider)
                    .aria_label("Timeline")
                    .flex_1()
                    .min_w(px(40.))
                    .h(px(20.))
                    .mx_1()
                    .cursor_pointer()
                    .child(
                        canvas(
                            move |bounds, _, _| slider.set(Some(bounds)),
                            move |bounds, _, window, _| {
                                let mid = bounds.top() + bounds.size.height / 2.;
                                let track = Bounds::new(
                                    point(bounds.left(), mid - px(1.5)),
                                    size(bounds.size.width, px(3.)),
                                );
                                window.paint_quad(fill(track, content(0.12)).corner_radii(px(1.5)));
                                let filled = Bounds::new(
                                    track.origin,
                                    size(track.size.width * fraction, track.size.height),
                                );
                                window.paint_quad(fill(filled, content(0.7)).corner_radii(px(1.5)));
                                let knob = px(5.);
                                let x = bounds.left() + bounds.size.width * fraction;
                                window.paint_quad(
                                    fill(
                                        Bounds::new(
                                            point(x - knob, mid - knob),
                                            size(knob * 2., knob * 2.),
                                        ),
                                        content(0.95),
                                    )
                                    .corner_radii(knob),
                                );
                                // A drag follows the pointer past the track
                                // and ends wherever the button is let go.
                                if dragging {
                                    let moves = panel.clone();
                                    window.on_mouse_event(
                                        move |event: &MouseMoveEvent, phase, _, cx| {
                                            if phase != DispatchPhase::Capture {
                                                return;
                                            }
                                            let _ = moves.update(cx, |this, cx| {
                                                if event.pressed_button == Some(MouseButton::Left) {
                                                    this.slide_to(event.position.x, cx);
                                                } else {
                                                    this.stop_sliding(cx);
                                                }
                                            });
                                        },
                                    );
                                    let release = panel.clone();
                                    window.on_mouse_event(
                                        move |event: &MouseUpEvent, phase, _, cx| {
                                            if phase == DispatchPhase::Capture
                                                && event.button == MouseButton::Left
                                            {
                                                let _ = release
                                                    .update(cx, |this, cx| this.stop_sliding(cx));
                                            }
                                        },
                                    );
                                }
                            },
                        )
                        .size_full(),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            this.timeline.dragging = true;
                            this.slide_to(event.position.x, cx);
                        }),
                    ),
            )
            .child(
                div()
                    .id("swarm-go-live")
                    .debug_selector(|| "swarm-go-live".into())
                    .role(gpui::Role::Button)
                    .aria_label("Follow live")
                    .flex_shrink_0()
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .text_size(px(11.5))
                    .text_color(content(if live { 0.9 } else { 0.5 }))
                    .hover(|s| s.text_color(content(1.)))
                    .child(
                        div()
                            .size(px(6.))
                            .rounded_full()
                            .bg(if live && self.board.live {
                                live_color()
                            } else {
                                content(0.3)
                            }),
                    )
                    .child("Live")
                    .on_click(cx.listener(|this, _, _, cx| this.go_live(cx))),
            );
        let options = div()
            .px_3()
            .pt_1p5()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .child(
                div()
                    .flex_1()
                    .min_w(px(120.))
                    .font_features(tabular())
                    .text_size(px(11.5))
                    .text_color(content(0.55))
                    .child(info),
            )
            .child(
                chip(
                    "swarm-hide-inactive",
                    "Hide inactive",
                    self.timeline.hide_inactive,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.timeline.hide_inactive = !this.timeline.hide_inactive;
                    cx.notify();
                })),
            )
            .child(
                chip("swarm-expand-all", "Expand @all", self.timeline.expand_all).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.timeline.expand_all = !this.timeline.expand_all;
                        cx.notify();
                    }),
                ),
            )
            .child(
                chip(
                    "swarm-pacing",
                    if self.timeline.real_time {
                        "Real-time pacing"
                    } else {
                        "Even pacing"
                    },
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.timeline.real_time = !this.timeline.real_time;
                    cx.notify();
                })),
            )
            .child(
                chip("swarm-speed", format!("{}×", self.timeline.speed), false).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.timeline.speed = match this.timeline.speed {
                            1 => 2,
                            2 => 4,
                            _ => 1,
                        };
                        cx.notify();
                    }),
                ),
            );
        let legend = div()
            .px_3()
            .pb_1()
            .flex()
            .flex_wrap()
            .gap_x_3()
            .gap_y_1()
            .text_size(px(11.))
            .text_color(content(0.45))
            .child("→ @mention, width = count")
            .child("⇢ via @all")
            .child("◌ posted with no mention")
            .when(all_posts > 0, |d| d.child(format!("@all {all_posts}")));
        let card = current.map(|post| {
            div()
                .mx_3()
                .mb_3()
                .p_3()
                .rounded(px(10.))
                .bg(content(0.04))
                .border_1()
                .border_color(content(0.06))
                .flex()
                .flex_col()
                .gap_1()
                .child(post_header(post, true))
                .child(
                    div()
                        .text_size(px(12.5))
                        .line_height(px(18.))
                        .text_color(content(0.88))
                        .child(rich_text(&post.text)),
                )
        });
        div()
            .id("swarm-graph")
            .debug_selector(|| "swarm-graph".into())
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .child(controls)
            .child(options)
            .child(
                div()
                    .id("swarm-graph-canvas")
                    .debug_selector(|| "swarm-graph-canvas".into())
                    .flex_shrink_0()
                    .h(px(280.))
                    .mx_2()
                    .my_2()
                    .when(self.timeline.hover.is_some(), |d| d.cursor_pointer())
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                        this.hover_node(event.position, cx)
                    }))
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        if !*hovered && this.timeline.hover.take().is_some() {
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(|this, event: &gpui::ClickEvent, _, cx| {
                        this.click_node(event.position(), cx)
                    }))
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |bounds, _, window, cx| paint(bounds, &scene, window, cx),
                        )
                        .size_full(),
                    ),
            )
            .child(legend)
            .children(card)
            .into_any_element()
    }
}
