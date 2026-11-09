//! The notch: a quiet strip around the camera housing (or a floating island)
//! that says what the agents are doing and what needs the user. Everything
//! here is plain data and rules, with no interface: how the notch looks and
//! which widgets its shelf holds (one saved value, always normalized), where
//! it sits on a screen, and which notice it shows.
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};

pub mod hooks;
pub mod system;

/// The setting that keeps [`Look`], as JSON.
pub const SETTING: &str = "notch.look";

/// Around the camera (or hanging from the top edge where there is none), or a
/// pill floating under the menu bar.
pub const STYLES: [&str; 2] = ["notch", "island"];
/// What opens the panel: staying over the notch, or a click.
pub const OPENS_ON: [&str; 2] = ["hover", "click"];
/// How wide the open panel is.
pub const WIDTHS: [&str; 3] = ["compact", "regular", "wide"];

/// The widgets the shelf can hold and the widths each may take, in columns.
pub const WIDGETS: [(&str, &[u32]); 8] = [
    ("agents", &[2, 3]),
    ("recent", &[2, 3]),
    ("asks", &[1, 2, 3]),
    ("today", &[1, 2]),
    ("call", &[1, 2]),
    ("clock", &[1]),
    ("system", &[1, 2]),
    ("new", &[1]),
];
/// The shelf's columns.
pub const COLUMNS: u32 = 3;
/// The most rows the shelf may fill.
pub const MAX_ROWS: u32 = 3;

/// The name a widget shows on the shelf and in its catalog.
pub fn widget_name(id: &str) -> &'static str {
    match id {
        "agents" => "Agents",
        "recent" => "Recent chats",
        "asks" => "Needs you",
        "today" => "Reminders",
        "call" => "Call",
        "clock" => "Clock",
        "system" => "System",
        "new" => "New chat",
        _ => "",
    }
}

/// The widths a widget may take; none for an id the shelf does not know.
pub fn widths(id: &str) -> Option<&'static [u32]> {
    WIDGETS
        .iter()
        .find(|(known, _)| *known == id)
        .map(|(_, widths)| *widths)
}

/// One widget on the shelf and how many columns it takes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Widget {
    pub id: String,
    pub span: u32,
}

impl Widget {
    pub fn new(id: &str, span: u32) -> Self {
        Self {
            id: id.to_owned(),
            span,
        }
    }
}

/// The widgets of each row, as indices into `shelf`: each goes into the
/// first row with room for it, as the shelf draws them.
pub fn layout(shelf: &[Widget]) -> Vec<Vec<usize>> {
    let mut rows: Vec<(u32, Vec<usize>)> = Vec::new();
    for (index, widget) in shelf.iter().enumerate() {
        match rows
            .iter_mut()
            .find(|(used, _)| *used + widget.span <= COLUMNS)
        {
            Some((used, row)) => {
                *used += widget.span;
                row.push(index);
            }
            None => rows.push((widget.span, vec![index])),
        }
    }
    rows.into_iter().map(|(_, row)| row).collect()
}

/// How many rows the shelf fills.
pub fn rows(shelf: &[Widget]) -> usize {
    layout(shelf).len()
}

/// Everything the user can change about the notch. A missing field, an
/// unknown word or a number out of range falls back to the default, so an old
/// or hand-edited value never breaks it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Look {
    pub style: String,
    pub opens_on: String,
    /// Whether a finished turn raises a notice, or only changes the count.
    pub finished_notices: bool,
    /// How long a notice that goes by itself stays, in seconds (2 to 12).
    pub notice_seconds: u32,
    /// The open panel's width: compact, regular or wide.
    pub width: String,
    /// A hairline along the notch's edge while it shows something.
    pub edge: bool,
    /// The count of agents at work beside the camera.
    pub show_count: bool,
    /// The volume and brightness keys answered by the notch instead of the
    /// system's own indicator.
    pub system_keys: bool,
    /// The shelf's rows of widgets (1 to 3).
    pub shelf_rows: u32,
    /// What the shelf shows, in order, and how wide each one is.
    pub shelf: Vec<Widget>,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            style: "notch".into(),
            opens_on: "hover".into(),
            finished_notices: true,
            notice_seconds: 4,
            width: "regular".into(),
            edge: true,
            show_count: true,
            system_keys: true,
            shelf_rows: 2,
            shelf: vec![
                Widget::new("agents", 2),
                Widget::new("clock", 1),
                Widget::new("recent", 2),
                Widget::new("new", 1),
            ],
        }
    }
}

fn word(choices: &[&str], saved: &str, fallback: &str) -> String {
    choices
        .iter()
        .copied()
        .find(|choice| *choice == saved)
        .unwrap_or(fallback)
        .to_owned()
}

impl Look {
    /// Every field within its range: unknown words go back to the default,
    /// numbers are clamped, and the shelf keeps known widgets, once each, at
    /// a width they allow, and only as many as fit in its rows.
    pub fn normalized(mut self) -> Self {
        let base = Self::default();
        self.style = word(&STYLES, &self.style, &base.style);
        self.opens_on = word(&OPENS_ON, &self.opens_on, &base.opens_on);
        self.width = word(&WIDTHS, &self.width, &base.width);
        self.notice_seconds = self.notice_seconds.clamp(2, 12);
        self.shelf_rows = self.shelf_rows.clamp(1, MAX_ROWS);
        let mut shelf: Vec<Widget> = Vec::new();
        for mut widget in std::mem::take(&mut self.shelf) {
            let Some(allowed) = widths(&widget.id) else {
                continue;
            };
            if shelf.iter().any(|kept| kept.id == widget.id) {
                continue;
            }
            if !allowed.contains(&widget.span) {
                widget.span = allowed
                    .iter()
                    .copied()
                    .filter(|width| *width <= widget.span)
                    .max()
                    .unwrap_or(allowed[0]);
            }
            shelf.push(widget);
            if rows(&shelf) > self.shelf_rows as usize {
                shelf.pop();
            }
        }
        self.shelf = shelf;
        self
    }

    pub fn island(&self) -> bool {
        self.style == "island"
    }

    pub fn opens_on_click(&self) -> bool {
        self.opens_on == "click"
    }

    /// The open panel's width in points.
    pub fn panel_width(&self) -> f32 {
        match self.width.as_str() {
            "compact" => 392.,
            "wide" => 496.,
            _ => PANEL_WIDTH,
        }
    }

    /// The shelf as it would be with `change`, if it still fits its rows.
    fn try_shelf(&mut self, change: impl FnOnce(&mut Vec<Widget>)) -> bool {
        let mut shelf = self.shelf.clone();
        change(&mut shelf);
        if rows(&shelf) > self.shelf_rows as usize {
            return false;
        }
        self.shelf = shelf;
        true
    }

    /// The widgets not on the shelf that would fit in it now.
    pub fn addable(&self) -> Vec<&'static str> {
        WIDGETS
            .iter()
            .filter(|(id, _)| !self.shelf.iter().any(|widget| widget.id == *id))
            .filter(|(id, allowed)| {
                let mut shelf = self.shelf.clone();
                shelf.push(Widget::new(id, allowed[0]));
                rows(&shelf) <= self.shelf_rows as usize
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// Puts a widget at the end of the shelf at its narrowest width.
    pub fn add(&mut self, id: &str) -> bool {
        let Some(allowed) = widths(id) else {
            return false;
        };
        if self.shelf.iter().any(|widget| widget.id == id) {
            return false;
        }
        self.try_shelf(|shelf| shelf.push(Widget::new(id, allowed[0])))
    }

    pub fn remove(&mut self, id: &str) {
        self.shelf.retain(|widget| widget.id != id);
    }

    /// Moves a widget one place earlier (`-1`) or later (`1`) on the shelf.
    pub fn shift(&mut self, id: &str, by: i32) -> bool {
        let Some(at) = self.shelf.iter().position(|widget| widget.id == id) else {
            return false;
        };
        let to = at as i64 + i64::from(by);
        if to < 0 || to >= self.shelf.len() as i64 {
            return false;
        }
        self.try_shelf(|shelf| shelf.swap(at, to as usize))
    }

    /// The next width the widget allows, wider (`1`) or narrower (`-1`), if
    /// the shelf still fits.
    pub fn resize(&mut self, id: &str, by: i32) -> bool {
        let Some(allowed) = widths(id) else {
            return false;
        };
        let Some(at) = self.shelf.iter().position(|widget| widget.id == id) else {
            return false;
        };
        let current = allowed
            .iter()
            .position(|width| *width == self.shelf[at].span)
            .unwrap_or(0) as i64;
        let next = current + i64::from(by);
        if next < 0 || next >= allowed.len() as i64 {
            return false;
        }
        let span = allowed[next as usize];
        self.try_shelf(|shelf| shelf[at].span = span)
    }

    /// Fewer rows drop the widgets that no longer fit, last first.
    pub fn set_rows(&mut self, rows: u32) {
        self.shelf_rows = rows.clamp(1, MAX_ROWS);
        *self = std::mem::take(self).normalized();
    }
}

/// The look saved in the setting; nothing saved, or something unreadable,
/// is the default.
pub fn read(saved: Option<&str>) -> Look {
    saved
        .and_then(|saved| serde_json::from_str::<Look>(saved).ok())
        .unwrap_or_default()
        .normalized()
}

pub fn to_json(look: &Look) -> String {
    serde_json::to_string(look).unwrap_or_default()
}

// Geometry, in points from the screen's top left.

/// A screen as the notch needs it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Screen {
    pub width: f32,
    /// The safe area's top inset: the camera housing's height, or 0.
    pub safe_top: f32,
    /// The menu bar's room left and right of the camera (macOS's auxiliary
    /// top areas); 0 where there are none.
    pub left_area: f32,
    pub right_area: f32,
    /// The menu bar's height, or 0 where there is none (Windows).
    pub menu_bar: f32,
}

/// The camera housing, where the screen has one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub center: f32,
    pub width: f32,
    pub height: f32,
}

/// The housing between the two auxiliary areas. A screen without a safe
/// inset, or with areas that leave no plausible housing, has none.
pub fn camera(screen: &Screen) -> Option<Camera> {
    if screen.safe_top <= 0. || screen.left_area <= 0. || screen.right_area <= 0. {
        return None;
    }
    let width = screen.width - screen.left_area - screen.right_area;
    if width <= 0. || width > screen.width / 2. {
        return None;
    }
    let height = if screen.menu_bar > 0. {
        screen.safe_top.min(screen.menu_bar)
    } else {
        screen.safe_top
    };
    Some(Camera {
        center: screen.left_area + width / 2.,
        width,
        height,
    })
}

/// A rectangle on the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// The gap between the menu bar (or the top edge) and a floating island.
pub const ISLAND_GAP: f32 = 6.;
/// The closed notch where there is no camera: a slim handle on the top edge.
pub const HANDLE: (f32, f32) = (120., 8.);
/// The closed island while nothing happens.
pub const ISLAND_REST: (f32, f32) = (132., 30.);
/// The height of the closed notch with something to say, where there is no
/// camera to match.
pub const BAR_HEIGHT: f32 = 30.;
/// Each side's room for the count and the provider while agents work.
pub const EAR: f32 = 38.;
pub const NOTICE_WIDTH: f32 = 380.;
pub const PANEL_WIDTH: f32 = 432.;
/// The shelf's widget height and the gap between widgets.
pub const CELL_HEIGHT: f32 = 92.;
pub const CELL_GAP: f32 = 8.;
/// The panel's padding, its header and its row of tabs.
pub const PANEL_PAD: f32 = 14.;
pub const PANEL_HEADER: f32 = 32.;

/// What the notch shows, and so how big it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// Nothing to say.
    Quiet,
    /// Wings beside the camera, `left` and `right` points wide: who works
    /// on the left, what the lead does on the right.
    Busy { left: u32, right: u32 },
    /// A notice of `lines` lines under the camera.
    Notice { lines: u32 },
    /// The panel, `body` points tall under its header.
    Open { body: u32 },
}

/// Where the notch sits and how big it is on `screen`, for `shape`.
pub fn frame(look: &Look, screen: &Screen, shape: Shape) -> Frame {
    let camera = camera(screen);
    let island = look.island();
    // The camera's height on top of anything that hangs from it.
    let top = match (island, camera) {
        (false, Some(camera)) => camera.height,
        _ => 0.,
    };
    let (width, height) = match shape {
        Shape::Quiet => match (island, camera) {
            (true, _) => ISLAND_REST,
            (false, Some(camera)) => (camera.width, camera.height),
            (false, None) => HANDLE,
        },
        Shape::Busy { left, right } => {
            let (left, right) = (left as f32, right as f32);
            match (island, camera) {
                (false, Some(camera)) => (camera.width + left + right, camera.height),
                _ => ((left + right + 12.).max(ISLAND_REST.0), BAR_HEIGHT),
            }
        }
        Shape::Notice { lines } => (
            NOTICE_WIDTH.max(camera.map_or(0., |c| c.width + 2. * EAR)),
            top + 30. + 20. * lines.max(1) as f32,
        ),
        Shape::Open { body } => (
            look.panel_width()
                .max(camera.map_or(0., |c| c.width + 2. * EAR)),
            top + PANEL_HEADER + body as f32 + PANEL_PAD,
        ),
    };
    // Wings may differ: the camera stays where it is and each wing grows
    // to what it says, so the words start right beside the camera.
    let center = match (island, camera, shape) {
        (false, Some(camera), Shape::Busy { left, right }) => {
            camera.center + (right as f32 - left as f32) / 2.
        }
        (false, Some(camera), _) => camera.center,
        _ => screen.width / 2.,
    };
    let y = if island {
        // Under the whole housing, which can reach past the menu bar.
        screen.menu_bar.max(screen.safe_top) + ISLAND_GAP
    } else {
        0.
    };
    Frame {
        x: (center - width / 2.).round(),
        y,
        width,
        height,
    }
}

/// The shelf's height for `rows` rows.
pub fn shelf_height(rows: usize) -> f32 {
    let rows = rows.max(1) as f32;
    rows * CELL_HEIGHT + (rows - 1.) * CELL_GAP
}

/// A widget's width for `span` columns of a panel `width` wide.
pub fn cell_width(width: f32, span: u32) -> f32 {
    let column = (width - 2. * PANEL_PAD - (COLUMNS - 1) as f32 * CELL_GAP) / COLUMNS as f32;
    column * span as f32 + (span.saturating_sub(1)) as f32 * CELL_GAP
}

// What the notch says.

/// A session at work, or one that finished and was not looked at yet.
#[derive(Clone, Debug, PartialEq)]
pub struct Activity {
    pub id: i64,
    pub provider: String,
    pub title: String,
    /// What it does now ("Reading files"), or "Done".
    pub step: String,
    pub started_at: Option<u64>,
    pub needs_you: bool,
    pub done: bool,
    /// The agent's name, for a Mono's chat; none for a coding session.
    pub agent: Option<String>,
    /// Its task list: steps done and steps in all.
    pub steps: Option<(u32, u32)>,
    /// The agent's own face, for a Mono's chat; a coding session shows its
    /// provider instead.
    pub face: Option<Face>,
}

/// A Mono's mascot and color.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Face {
    pub mascot: String,
    pub color: u32,
}

/// Who a wing shows: an agent's face, or a coding session's provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub provider: String,
    pub face: Option<Face>,
}

impl Activity {
    /// Who it is: the agent's name, or its provider's.
    pub fn who(&self) -> String {
        self.agent.clone().unwrap_or_else(|| {
            crate::provider::Kind::from_id(&self.provider)
                .label()
                .to_owned()
        })
    }
}

/// An approval or question waiting for the user.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub session: i64,
    pub request: String,
    pub provider: String,
    pub title: String,
    pub label: String,
    /// An approval can be answered from the notch; a question opens its chat.
    pub approval: bool,
}

/// A group call in progress.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub group: String,
    pub status: String,
}

/// A chat the user had lately.
#[derive(Clone, Debug, PartialEq)]
pub struct Recent {
    pub id: i64,
    pub provider: String,
    pub title: String,
    /// Unix milliseconds.
    pub updated_at: u64,
}

/// A reminder still to come.
#[derive(Clone, Debug, PartialEq)]
pub struct Upcoming {
    pub session: i64,
    pub title: String,
    /// Unix milliseconds.
    pub due_at: u64,
}

/// Everything the notch reads from the app.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Feed {
    pub activities: Vec<Activity>,
    pub asks: Vec<Ask>,
    pub call: Option<Call>,
    /// The latest chats, newest first.
    pub recent: Vec<Recent>,
    /// Reminders not yet due, soonest first.
    pub upcoming: Vec<Upcoming>,
}

/// "3m", "2h", "Yesterday" or a date: how long ago `at` was, short.
pub fn ago(at: u64, now: u64) -> String {
    let minutes = now.saturating_sub(at) / 60_000;
    match minutes {
        0 => "Now".into(),
        1..60 => format!("{minutes}m"),
        60..1440 => format!("{}h", minutes / 60),
        1440..2880 => "Yesterday".into(),
        _ => format!("{}d", minutes / 1440),
    }
}

/// What the closed notch shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rest {
    pub working: usize,
    pub asking: usize,
    /// The provider of the session that leads (asking first, then the
    /// longest running).
    pub lead: Option<String>,
    /// Who leads and what it does: "Claude · Reading files".
    pub lead_text: Option<String>,
    /// The lead's steps done and in all.
    pub lead_steps: Option<(u32, u32)>,
    /// The providers of the sessions at work, longest running first, as many
    /// as the left wing shows.
    pub marks: Vec<Mark>,
    pub call: bool,
}

/// How many sessions the left wing shows before "+N".
pub const MARKS: usize = 3;

impl Rest {
    pub fn busy(&self) -> bool {
        self.working > 0 || self.asking > 0 || self.call
    }

    /// The working sessions the left wing leaves out.
    pub fn more(&self) -> usize {
        self.working.saturating_sub(self.marks.len())
    }

    /// The wings' widths: a mark per session (and "+N") on the left, the
    /// lead's words and steps on the right.
    pub fn wings(&self, show_count: bool) -> (u32, u32) {
        let marks = self
            .marks
            .len()
            .max(usize::from(self.asking > 0 || self.call));
        let left = 14 + 18 * marks as u32 + if show_count && self.more() > 0 { 22 } else { 0 };
        // The lead's state word at about 6.2 points a letter and its steps.
        let right = match &self.lead_text {
            Some(word) => {
                let steps = if self.lead_steps.is_some() { 30. } else { 0. };
                (6.2 * word.chars().count() as f32 + steps + 24.) as u32
            }
            None => 36,
        };
        // Both wings as wide as the wider, so the notch stays centered on
        // the camera.
        let side = left.max(right).max(36);
        (side, side)
    }
}

/// What a session is doing, in one word, from its live step ("Read
/// src/main.rs", "Running cargo test"): the state the wings show.
pub fn state_word(step: &str) -> &'static str {
    let step = step.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|word| step.contains(word));
    if has(&["need", "approval", "waiting"]) {
        "Waiting"
    } else if has(&["edit", "update", "patch", "replace", "apply"]) {
        "Editing"
    } else if has(&["write", "create", "add "]) {
        "Writing"
    } else if has(&["read", "view", "open", "cat "]) {
        "Reading"
    } else if has(&["search", "grep", "glob", "find", "list", "explor"]) {
        "Searching"
    } else if has(&["fetch", "web", "brows", "url", "http"]) {
        "Browsing"
    } else if has(&["agent", "delegat", "task", "spawn"]) {
        "Delegating"
    } else if has(&[
        "run", "bash", "exec", "command", "test", "build", "shell", "terminal",
    ]) {
        "Running"
    } else if has(&["done", "finish"]) {
        "Done"
    } else {
        "Thinking"
    }
}

pub fn rest(feed: &Feed) -> Rest {
    let mut working: Vec<&Activity> = feed.activities.iter().filter(|a| !a.done).collect();
    working.sort_by_key(|activity| activity.started_at.unwrap_or(u64::MAX));
    let lead_activity = feed
        .asks
        .first()
        .and_then(|ask| working.iter().find(|a| a.id == ask.session).copied())
        .or_else(|| working.first().copied());
    let lead = feed
        .asks
        .first()
        .map(|ask| ask.provider.clone())
        .or_else(|| lead_activity.map(|activity| activity.provider.clone()));
    // A state, never the whole step: "Reading", "Editing", "Waiting".
    let lead_text = match (feed.asks.first(), lead_activity) {
        (Some(_), _) => Some("Waiting".to_owned()),
        (None, Some(activity)) => Some(state_word(&activity.step).to_owned()),
        (None, None) => feed.call.as_ref().map(|_| "In a call".to_owned()),
    };
    Rest {
        working: working.len(),
        asking: feed.asks.len(),
        lead,
        lead_text,
        lead_steps: lead_activity.and_then(|activity| activity.steps),
        // One mark per kind of session: two Claude Code chats are one Claude
        // mark and "+1", as the sidebar's group faces count the rest.
        marks: working
            .iter()
            .map(|activity| Mark {
                provider: activity.provider.clone(),
                face: activity.face.clone(),
            })
            .fold(Vec::<Mark>::new(), |mut marks, mark| {
                if !marks.contains(&mark) {
                    marks.push(mark);
                }
                marks
            })
            .into_iter()
            .take(MARKS)
            .collect(),
        call: feed.call.is_some(),
    }
}

/// A notice that hangs under the notch for a while.
#[derive(Clone, Debug, PartialEq)]
pub enum Notice {
    /// Stays until it is answered or no longer pending.
    Ask(Ask),
    /// Goes by itself.
    Done {
        session: i64,
        provider: String,
        title: String,
    },
}

impl Notice {
    fn ask_key(&self) -> Option<(i64, &str)> {
        match self {
            Self::Ask(ask) => Some((ask.session, ask.request.as_str())),
            Self::Done { .. } => None,
        }
    }
}

/// The notices to show, one at a time: asks first, in the order they came,
/// then finished turns.
#[derive(Debug, Default)]
pub struct Notices {
    asked: HashSet<(i64, String)>,
    finished: HashSet<i64>,
    current: Option<(Notice, u64)>,
    queue: VecDeque<Notice>,
    /// Whether the first feed was seen: turns that had already finished
    /// then are not news.
    primed: bool,
}

impl Notices {
    pub fn current(&self) -> Option<&Notice> {
        self.current.as_ref().map(|(notice, _)| notice)
    }

    /// Takes in what the app says now: new asks and newly finished turns are
    /// queued, and asks no longer pending go away.
    pub fn update(&mut self, feed: &Feed, look: &Look, now: u64) {
        let pending: HashSet<(i64, String)> = feed
            .asks
            .iter()
            .map(|ask| (ask.session, ask.request.clone()))
            .collect();
        let gone = |notice: &Notice| {
            notice
                .ask_key()
                .is_some_and(|(session, request)| !pending.contains(&(session, request.to_owned())))
        };
        self.queue.retain(|notice| !gone(notice));
        if self
            .current
            .as_ref()
            .is_some_and(|(notice, _)| gone(notice))
        {
            self.current = None;
        }
        self.asked.retain(|key| pending.contains(key));
        let asks_queued = self
            .queue
            .iter()
            .filter(|notice| matches!(notice, Notice::Ask(_)))
            .count();
        let mut fresh = Vec::new();
        for ask in &feed.asks {
            if self.asked.insert((ask.session, ask.request.clone())) {
                fresh.push(Notice::Ask(ask.clone()));
            }
        }
        for (offset, notice) in fresh.into_iter().enumerate() {
            self.queue.insert(asks_queued + offset, notice);
        }
        let finished: HashSet<i64> = feed
            .activities
            .iter()
            .filter(|activity| activity.done)
            .map(|activity| activity.id)
            .collect();
        if look.finished_notices && self.primed {
            for activity in feed.activities.iter().filter(|a| a.done) {
                if !self.finished.contains(&activity.id) {
                    self.queue.push_back(Notice::Done {
                        session: activity.id,
                        provider: activity.provider.clone(),
                        title: activity.title.clone(),
                    });
                }
            }
        }
        self.finished = finished;
        self.primed = true;
        self.advance(now);
    }

    /// Lets a notice that goes by itself go once its time is up. Returns the
    /// milliseconds until the current one expires, if it does.
    pub fn tick(&mut self, look: &Look, now: u64) -> Option<u64> {
        let stay = u64::from(look.notice_seconds) * 1000;
        let expired =
            matches!(&self.current, Some((Notice::Done { .. }, shown)) if now >= shown + stay);
        if expired {
            self.current = None;
            self.advance(now);
        }
        match &self.current {
            Some((Notice::Done { .. }, shown)) => Some((shown + stay).saturating_sub(now)),
            _ => None,
        }
    }

    /// The user closed the notice, or acted on it.
    pub fn dismiss(&mut self, now: u64) {
        self.current = None;
        self.advance(now);
    }

    fn advance(&mut self, now: u64) {
        // An ask waiting behind a finished turn goes first.
        if matches!(self.current, Some((Notice::Done { .. }, _)))
            && matches!(self.queue.front(), Some(Notice::Ask(_)))
        {
            let (done, _) = self.current.take().expect("checked above");
            self.queue.push_back(done);
        }
        if self.current.is_none() {
            self.current = self.queue.pop_front().map(|next| (next, now));
        }
    }
}

#[cfg(test)]
mod tests;
