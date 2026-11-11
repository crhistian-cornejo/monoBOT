//! A swarm's message board: what the coordinator and its workers tell each
//! other, who the members are and what each one is doing, and the graph of
//! who talked to whom. The board follows the run (`sync`): dispatching,
//! finishing, accepting and cancelling a task post on it by themselves, so it
//! reads well even when no agent posts. Agents post with `control post`,
//! naming members with `@handle` or everyone with `@all`; what is said to a
//! member reaches it through `inbox`. Pure: the shell feeds it the run and
//! saves it.
use super::control::{Run, Status, Task};
use serde::{Deserialize, Serialize};

/// The lead's handle on the board.
pub const COORDINATOR: &str = "coordinator";
/// Posts kept; older ones are dropped first.
pub const MAX_POSTS: usize = 1_000;
/// Characters kept of each post.
pub const POST_CHARS: usize = 2_000;
/// Characters of a worker's result that its report post keeps.
const REPORT_CHARS: usize = 400;
/// The pixel mascots members wear, in order.
pub const MASCOTS: [&str; 10] = [
    "robot", "invader", "ghost", "cat", "crab", "dino", "frog", "rocket", "mushroom", "skull",
];

/// What a member is doing, as the Members tab groups it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    #[default]
    Queued,
    Working,
    /// Blocked on an approval or a question the coordinator must answer.
    NeedsInput,
    /// Its turn ended and it waits for review or @mentions.
    StandingBy,
    Done,
    Failed,
    Cancelled,
}
impl State {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Working => "Working",
            Self::NeedsInput => "Needs input",
            Self::StandingBy => "Standing by",
            Self::Done => "Done",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }
    /// Running right now: the green dot.
    pub fn active(self) -> bool {
        matches!(self, Self::Working | Self::NeedsInput)
    }
}

/// The Members tab's sections, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Section {
    Coordinator,
    Active,
    StandingBy,
    Finished,
}
impl Section {
    pub fn label(self) -> &'static str {
        match self {
            Self::Coordinator => "Coordinator",
            Self::Active => "Active",
            Self::StandingBy => "Standing by",
            Self::Finished => "Finished",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Member {
    pub handle: String,
    /// What it was asked to do: the task's title, or the run's request for
    /// the coordinator.
    pub assignment: String,
    /// The run's task ID; None for the coordinator.
    pub task: Option<String>,
    pub session: Option<i64>,
    pub provider: String,
    pub model: Option<String>,
    pub state: State,
    /// Its latest post, or what it is doing when it has not posted yet.
    pub latest: String,
    /// Sequence number of its latest post, 0 before any.
    pub latest_seq: u64,
    /// The task's dispatch and acceptance last seen, to notice transitions.
    pub dispatch: u64,
    pub accepted: bool,
    /// Posts up to this sequence number have reached it.
    pub delivered: u64,
}
impl Member {
    pub fn coordinator(&self) -> bool {
        self.task.is_none()
    }
    pub fn section(&self) -> Section {
        if self.coordinator() {
            return Section::Coordinator;
        }
        match self.state {
            State::Working | State::NeedsInput => Section::Active,
            State::Queued | State::StandingBy => Section::StandingBy,
            State::Done | State::Failed | State::Cancelled => Section::Finished,
        }
    }
}

/// Why a post was made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// An agent's own post.
    #[default]
    Message,
    /// The coordinator handed a member its task.
    Assign,
    /// A member's turn ended with a result.
    Report,
    /// A member's turn failed.
    Fail,
    /// The coordinator accepted a member's work.
    Accept,
    Cancel,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Post {
    pub seq: u64,
    /// Milliseconds since the epoch.
    pub at: u64,
    pub from: String,
    /// The members it names, in the order it names them.
    pub to: Vec<String>,
    /// It said @all.
    pub all: bool,
    pub text: String,
    pub kind: Kind,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Board {
    pub version: u32,
    /// The run's request, shortened: the panel's title.
    pub title: String,
    /// The run is still working.
    pub live: bool,
    pub posts: Vec<Post>,
    pub members: Vec<Member>,
    pub next_seq: u64,
}

/// The key a lead's board is saved under.
pub fn setting_key(lead: i64) -> String {
    format!("orchestration-board:{lead}")
}

/// One edge of the graph: posts from one member to another.
#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    /// Member indices.
    pub from: usize,
    pub to: usize,
    pub count: u32,
    /// Every post on it named the member only through @all.
    pub via_all: bool,
    /// The latest post on it.
    pub last: u64,
}

/// What the Graph tab draws at one point of the timeline.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Graph {
    pub edges: Vec<Edge>,
    /// Members that posted without naming anyone, by index.
    pub silent: Vec<bool>,
    /// Members that sent or received anything, by index.
    pub involved: Vec<bool>,
    /// @all posts so far.
    pub all_posts: u32,
}

fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((ix, _)) => format!("{}…", text[..ix].trim_end()),
        None => text.to_owned(),
    }
}

/// The text with whitespace runs collapsed to single spaces, on one line.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A short title from the user's request.
pub fn title(request: &str) -> String {
    let line = request
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("Swarm");
    clip(line.trim_start_matches("/swarm").trim(), 80)
}

/// The closing paragraph of a worker's result, on one line: where workers
/// put their concise final report.
pub fn report(result: &str) -> String {
    let paragraph = result
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .last()
        .unwrap_or_default();
    let text = paragraph
        .lines()
        .map(|l| l.trim().trim_start_matches('#').trim())
        .collect::<Vec<_>>()
        .join(" ");
    clip(&one_line(&text), REPORT_CHARS)
}

/// The `@handle` words in `text`, lowercased, in order and without repeats.
/// An `@` inside a word (an e-mail address) does not count.
pub fn mention_words(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut ix = 0;
    while ix < bytes.len() {
        if bytes[ix] == b'@'
            && (ix == 0 || !(bytes[ix - 1].is_ascii_alphanumeric() || bytes[ix - 1] == b'_'))
        {
            let start = ix + 1;
            let mut end = start;
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'-' || bytes[end] == b'_')
            {
                end += 1;
            }
            // A trailing hyphen ends a sentence, not the handle.
            while end > start && bytes[end - 1] == b'-' {
                end -= 1;
            }
            if end > start {
                let word = text[start..end].to_ascii_lowercase();
                if !out.contains(&word) {
                    out.push(word);
                }
            }
            ix = end.max(start);
        } else {
            ix += 1;
        }
    }
    out
}

/// A handle from a role name: lowercase ASCII letters, digits and hyphens.
pub fn handle_from(role: &str) -> String {
    let mut out = String::new();
    for c in role.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    out.chars().take(32).collect()
}

/// Text split into plain and `code` runs, for drawing inline code.
pub fn segments(text: &str) -> Vec<(bool, &str)> {
    let mut out = Vec::new();
    let mut code = false;
    for (ix, part) in text.split('`').enumerate() {
        if ix > 0 {
            code = !code;
        }
        if !part.is_empty() {
            out.push((code, part));
        }
    }
    // An unmatched backtick leaves the rest as plain text.
    if text.matches('`').count() % 2 == 1 {
        if let Some(last) = out.last_mut() {
            last.0 = false;
        }
    }
    out
}

/// A post's local time, such as "2:14 PM".
pub fn clock(at: u64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(at as i64)
        .single()
        .map(|t| t.format("%-I:%M %p").to_string())
        .unwrap_or_default()
}

/// How long after `start` something happened, such as "+1m 48s".
pub fn since(start: u64, at: u64) -> String {
    let seconds = at.saturating_sub(start) / 1000;
    match seconds {
        0..=59 => format!("+{seconds}s"),
        60..=3599 => format!("+{}m {}s", seconds / 60, seconds % 60),
        _ => format!("+{}h {}m", seconds / 3600, seconds % 3600 / 60),
    }
}

/// Where each of `count` members sits, normalized to 0..1: the coordinator
/// at the top and the others evenly round an ellipse, clockwise.
pub fn layout(count: usize) -> Vec<(f32, f32)> {
    if count == 1 {
        return vec![(0.5, 0.15)];
    }
    (0..count)
        .map(|ix| {
            let angle =
                -std::f32::consts::FRAC_PI_2 + ix as f32 * std::f32::consts::TAU / count as f32;
            (0.5 + 0.40 * angle.cos(), 0.5 + 0.40 * angle.sin())
        })
        .collect()
}

impl Board {
    pub fn new(request: &str) -> Self {
        Self {
            version: 1,
            title: title(request),
            live: true,
            members: vec![Member {
                handle: COORDINATOR.into(),
                assignment: clip(&one_line(request), 240),
                state: State::Working,
                ..Default::default()
            }],
            next_seq: 1,
            ..Default::default()
        }
    }

    pub fn member(&self, handle: &str) -> Option<&Member> {
        self.members
            .iter()
            .find(|m| m.handle.eq_ignore_ascii_case(handle))
    }
    fn index(&self, handle: &str) -> Option<usize> {
        self.members
            .iter()
            .position(|m| m.handle.eq_ignore_ascii_case(handle))
    }
    /// The member working on this session.
    pub fn handle_of(&self, session: i64) -> Option<&str> {
        self.members
            .iter()
            .find(|m| m.session == Some(session))
            .map(|m| m.handle.as_str())
    }

    /// A free handle: the role's, or `worker-N`.
    fn free_handle(&self, role: Option<&str>) -> String {
        let base = role
            .map(handle_from)
            .filter(|h| !h.is_empty() && h != "all" && !h.eq_ignore_ascii_case(COORDINATOR));
        match base {
            Some(base) => {
                if self.member(&base).is_none() {
                    return base;
                }
                (2..)
                    .map(|n| format!("{base}-{n}"))
                    .find(|h| self.member(h).is_none())
                    .unwrap()
            }
            None => (1..)
                .map(|n| format!("worker-{n}"))
                .find(|h| self.member(h).is_none())
                .unwrap(),
        }
    }

    /// Whether `handle` may be given to a new member.
    pub fn handle_available(&self, handle: &str) -> bool {
        let handle = handle_from(handle);
        !handle.is_empty() && handle != "all" && self.member(&handle).is_none()
    }

    fn push(&mut self, mut post: Post) -> u64 {
        post.seq = self.next_seq;
        self.next_seq += 1;
        post.text = clip(post.text.trim(), POST_CHARS);
        if let Some(member) = self.members.iter_mut().find(|m| m.handle == post.from) {
            member.latest = clip(&one_line(&post.text), 240);
            member.latest_seq = post.seq;
        }
        let seq = post.seq;
        self.posts.push(post);
        if self.posts.len() > MAX_POSTS {
            let excess = self.posts.len() - MAX_POSTS;
            self.posts.drain(..excess);
        }
        seq
    }

    /// An agent's own post. Every `@handle` in it must name a member;
    /// `@all` reaches all of them.
    pub fn post(&mut self, from: &str, text: &str, at: u64) -> Result<Post, String> {
        let from = self
            .member(from)
            .map(|m| m.handle.clone())
            .ok_or("Only members of this run can post on its board.")?;
        let text = text.trim();
        if text.is_empty() {
            return Err("Invalid text: provide a non-empty string".into());
        }
        if text.chars().count() > POST_CHARS {
            return Err(format!(
                "Invalid text: keep posts under {POST_CHARS} characters; put long results in your final report."
            ));
        }
        let mut to = Vec::new();
        let mut all = false;
        let mut unknown = Vec::new();
        for word in mention_words(text) {
            if word == "all" {
                all = true;
            } else if let Some(member) = self.member(&word) {
                if member.handle != from && !to.contains(&member.handle) {
                    to.push(member.handle.clone());
                }
            } else {
                unknown.push(format!("@{word}"));
            }
        }
        if !unknown.is_empty() {
            return Err(format!(
                "Unknown member {}. Members: {}.",
                unknown.join(", "),
                self.members
                    .iter()
                    .map(|m| format!("@{}", m.handle))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let post = Post {
            at,
            from,
            to,
            all,
            text: text.to_owned(),
            kind: Kind::Message,
            ..Default::default()
        };
        let seq = self.push(post);
        Ok(self.posts.iter().rfind(|p| p.seq == seq).cloned().unwrap())
    }

    fn system(&mut self, from: &str, to: &str, kind: Kind, text: String, at: u64) {
        self.push(Post {
            at,
            from: from.into(),
            to: vec![to.into()],
            text,
            kind,
            ..Default::default()
        });
    }

    /// Follow the run: add members for new tasks, update their state and
    /// post what changed. `needs_input` tells whether a running task is
    /// blocked on the coordinator. Returns whether anything changed.
    pub fn sync(&mut self, run: &Run, needs_input: &dyn Fn(&Task) -> bool, at: u64) -> bool {
        let before = (
            self.next_seq,
            self.live,
            self.members
                .iter()
                .map(|m| (m.state, m.session))
                .collect::<Vec<_>>(),
        );
        self.live = run.live();
        if let Some(lead) = self.members.first_mut() {
            lead.state = if self.live {
                State::Working
            } else {
                State::Done
            };
            if lead.session.is_none() {
                lead.session = Some(run.lead);
            }
        }
        for task in &run.tasks {
            let ix = match self
                .members
                .iter()
                .position(|m| m.task.as_deref() == Some(&task.id))
            {
                Some(ix) => ix,
                None => {
                    let handle = self.free_handle(task.handle.as_deref());
                    self.members.push(Member {
                        handle,
                        task: Some(task.id.clone()),
                        latest: "Queued".into(),
                        // Everything said before it joined is history.
                        delivered: self.next_seq.saturating_sub(1),
                        ..Default::default()
                    });
                    self.members.len() - 1
                }
            };
            let state = match task.status {
                Status::Queued => State::Queued,
                Status::Running if needs_input(task) => State::NeedsInput,
                Status::Running => State::Working,
                Status::Completed if task.accepted => State::Done,
                Status::Completed | Status::Blocked | Status::Interrupted => State::StandingBy,
                Status::Failed => State::Failed,
                Status::Cancelled => State::Cancelled,
            };
            let member = &self.members[ix];
            let handle = member.handle.clone();
            let was = member.state;
            // A turn may start and end between two syncs: the dispatch count
            // still shows it started.
            let dispatched = task.dispatch > member.dispatch;
            if dispatched {
                let scope = task
                    .files
                    .iter()
                    .take(4)
                    .map(|f| format!("`{f}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let text = if member.dispatch == 0 {
                    format!("{} — scope {scope}", task.title)
                } else {
                    format!("Another turn on {}.", task.title)
                };
                self.system(COORDINATOR, &handle, Kind::Assign, text, at);
            }
            let ended = (was.active() || dispatched) && !state.active() && state != State::Queued;
            if ended {
                match task.status {
                    Status::Completed => {
                        let text = report(&task.result);
                        let text = if text.is_empty() {
                            format!("Finished {}.", task.title)
                        } else {
                            text
                        };
                        self.system(&handle, COORDINATOR, Kind::Report, text, at);
                    }
                    Status::Failed | Status::Blocked => {
                        let reason = task
                            .error
                            .as_deref()
                            .map(one_line)
                            .filter(|e| !e.is_empty())
                            .unwrap_or_else(|| "the turn did not finish".into());
                        self.system(
                            &handle,
                            COORDINATOR,
                            Kind::Fail,
                            format!("Failed: {}", clip(&reason, REPORT_CHARS)),
                            at,
                        );
                    }
                    _ => {}
                }
            }
            if task.accepted && !self.members[ix].accepted {
                let files = task.integrated.len();
                self.system(
                    COORDINATOR,
                    &handle,
                    Kind::Accept,
                    if files == 0 {
                        format!("Accepted {}.", task.title)
                    } else {
                        format!(
                            "Accepted {}: {files} file{} applied.",
                            task.title,
                            if files == 1 { "" } else { "s" }
                        )
                    },
                    at,
                );
            }
            if state == State::Cancelled && was != State::Cancelled {
                self.system(
                    COORDINATOR,
                    &handle,
                    Kind::Cancel,
                    format!("Cancelled {}.", task.title),
                    at,
                );
            }
            let member = &mut self.members[ix];
            member.state = state;
            member.assignment = task.title.clone();
            member.session = task.session.or(member.session);
            member.provider = task.provider.clone();
            member.model = task.model.clone();
            member.dispatch = task.dispatch;
            member.accepted = task.accepted;
            if member.latest_seq == 0 {
                member.latest = match state {
                    State::Queued => run.waiting_for(task).unwrap_or_else(|| "Queued".into()),
                    State::Working => "Starting…".into(),
                    other => other.label().into(),
                };
            }
        }
        let after = (
            self.next_seq,
            self.live,
            self.members
                .iter()
                .map(|m| (m.state, m.session))
                .collect::<Vec<_>>(),
        );
        before != after
    }

    /// Posts addressed to `handle` that have not reached it, as a turn's
    /// preamble; marks them delivered. Only agents' own posts count: the
    /// board's automatic ones already reach their members as tasks and
    /// results.
    pub fn inbox(&mut self, handle: &str) -> Option<String> {
        let ix = self.index(handle)?;
        let handle = self.members[ix].handle.clone();
        let since = self.members[ix].delivered;
        let lines: Vec<String> = self
            .posts
            .iter()
            .filter(|p| p.seq > since && Self::for_member(p, &handle))
            .map(|p| format!("#{} @{}: {}", p.seq, p.from, p.text))
            .collect();
        self.members[ix].delivered = self.next_seq.saturating_sub(1);
        (!lines.is_empty()).then(|| {
            format!(
                "Board posts for you (@{handle}):\n{}\nAnswer on the board with `control post` when it helps the others.",
                lines.join("\n")
            )
        })
    }

    /// Whether `handle` has posts it has not been given.
    pub fn pending(&self, handle: &str) -> bool {
        let Some(member) = self.member(handle) else {
            return false;
        };
        self.posts
            .iter()
            .any(|p| p.seq > member.delivered && Self::for_member(p, &member.handle))
    }

    fn for_member(post: &Post, handle: &str) -> bool {
        post.kind == Kind::Message
            && post.from != handle
            && (post.all || post.to.iter().any(|t| t == handle))
    }

    /// What `control board` returns: members and posts after `since`.
    pub fn view(&self, since: u64, limit: usize) -> serde_json::Value {
        let posts: Vec<&Post> = self.posts.iter().filter(|p| p.seq > since).collect();
        let skip = posts.len().saturating_sub(limit);
        serde_json::json!({
            "members": self.members.iter().map(|m| serde_json::json!({
                "handle": m.handle,
                "assignment": m.assignment,
                "state": m.state.label(),
                "taskId": m.task,
            })).collect::<Vec<_>>(),
            "posts": posts[skip..].iter().map(|p| serde_json::json!({
                "seq": p.seq,
                "from": p.from,
                "to": p.to,
                "all": p.all,
                "text": p.text,
            })).collect::<Vec<_>>(),
            "latest": self.next_seq.saturating_sub(1),
        })
    }

    /// Posts matching `query` (from, to or text, ignoring case).
    pub fn matches(post: &Post, query: &str) -> bool {
        let query = query.trim();
        if query.is_empty() {
            return true;
        }
        let query = query.trim_start_matches('@').to_lowercase();
        post.from.to_lowercase().contains(&query)
            || post.to.iter().any(|t| t.to_lowercase().contains(&query))
            || post.text.to_lowercase().contains(&query)
    }

    /// The graph after the first `upto` posts. `expand_all` draws an @all
    /// post as an edge to every other member.
    pub fn graph(&self, upto: usize, expand_all: bool) -> Graph {
        let n = self.members.len();
        let mut graph = Graph {
            edges: Vec::new(),
            silent: vec![false; n],
            involved: vec![false; n],
            all_posts: 0,
        };
        for post in self.posts.iter().take(upto) {
            let Some(from) = self.index(&post.from) else {
                continue;
            };
            let mut targets: Vec<(usize, bool)> = post
                .to
                .iter()
                .filter_map(|t| self.index(t))
                .map(|ix| (ix, false))
                .collect();
            if post.all {
                graph.all_posts += 1;
                if expand_all {
                    for ix in (0..n).filter(|ix| *ix != from) {
                        if !targets.iter().any(|(t, _)| *t == ix) {
                            targets.push((ix, true));
                        }
                    }
                }
            }
            if targets.is_empty() && !post.all {
                graph.silent[from] = true;
            }
            graph.involved[from] = true;
            for (to, via_all) in targets {
                graph.involved[to] = true;
                match graph
                    .edges
                    .iter_mut()
                    .find(|e| e.from == from && e.to == to)
                {
                    Some(edge) => {
                        edge.count += 1;
                        edge.via_all &= via_all;
                        edge.last = post.seq;
                    }
                    None => graph.edges.push(Edge {
                        from,
                        to,
                        count: 1,
                        via_all,
                        last: post.seq,
                    }),
                }
            }
        }
        graph
    }

    /// The mascot and colour index of a member.
    pub fn look(ix: usize) -> (&'static str, usize) {
        (MASCOTS[ix % MASCOTS.len()], ix)
    }
}

#[cfg(test)]
mod tests;
