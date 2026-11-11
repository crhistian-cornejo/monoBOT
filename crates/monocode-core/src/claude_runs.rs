//! What Claude Code's subagents and workflows are doing, read from the files
//! it writes under `projects/<cwd>/<session>/`: plain subagents in
//! `subagents/agent-<id>.jsonl` with a `.meta.json` beside them, workflow runs
//! in `subagents/workflows/<run>/` (a `journal.jsonl` of started and finished
//! agents, one transcript per agent) and each run's script in
//! `workflows/scripts/<name>-<run>.js`, whose `meta` names its phases.
//!
//! Claude streams none of this to the desktop, so the Agents tab reads it.
//! A [`Scanner`] keeps what it parsed per file and only rereads files whose
//! size or modification time changed. Runs on a background thread.
use serde_json::Value;
use std::{
    collections::HashMap,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// How much of an agent's transcript is read to find its latest step.
const TAIL: u64 = 256 * 1024;
/// How much of an agent's transcript is read to find when it started.
const HEAD: u64 = 16 * 1024;
/// An unfinished agent whose transcript has not moved for this long has
/// stopped: its session ended or it was killed.
const STALE_MS: u64 = 20 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Running,
    Done,
    Failed,
    /// Never finished, and nothing has moved for a while.
    Stopped,
}

impl State {
    pub fn active(self) -> bool {
        self == Self::Running
    }
}

/// The latest thing an agent did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// The tool's name, or `None` for narration.
    pub tool: Option<String>,
    /// The command, file, pattern or sentence, on one line.
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agent {
    pub id: String,
    pub label: String,
    pub phase: Option<String>,
    pub state: State,
    pub model: Option<String>,
    pub started_at: Option<u64>,
    /// When its transcript last changed.
    pub updated_at: u64,
    pub last: Option<Step>,
    /// First line of what it reported back, once done.
    pub summary: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Phase {
    pub title: String,
    pub agents: Vec<Agent>,
}

impl Phase {
    pub fn state(&self) -> Option<State> {
        if self.agents.is_empty() {
            None
        } else if self.agents.iter().any(|a| a.state == State::Running) {
            Some(State::Running)
        } else if self.agents.iter().any(|a| a.state == State::Failed) {
            Some(State::Failed)
        } else if self.agents.iter().all(|a| a.state == State::Done) {
            Some(State::Done)
        } else {
            Some(State::Stopped)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// In the script's order; phases nobody started yet have no agents.
    pub phases: Vec<Phase>,
    pub updated_at: u64,
}

impl Run {
    pub fn agents(&self) -> impl Iterator<Item = &Agent> {
        self.phases.iter().flat_map(|phase| &phase.agents)
    }
    pub fn state(&self) -> State {
        let mut agents = self.agents().peekable();
        if agents.peek().is_none() {
            return State::Running;
        }
        let states: Vec<State> = self.agents().map(|a| a.state).collect();
        if states.contains(&State::Running) {
            State::Running
        } else if states.contains(&State::Failed) {
            State::Failed
        } else if states.contains(&State::Stopped) {
            State::Stopped
        } else {
            State::Done
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// Newest first.
    pub runs: Vec<Run>,
    /// Subagents started outside a workflow, newest first.
    pub agents: Vec<Agent>,
}

impl Snapshot {
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty() && self.agents.is_empty()
    }
    pub fn running(&self) -> usize {
        self.runs
            .iter()
            .flat_map(Run::agents)
            .chain(&self.agents)
            .filter(|a| a.state.active())
            .count()
    }
}

/// The folder Claude keeps session `id`'s extras in, found by its transcript
/// `projects/*/<id>.jsonl` so the cwd's folder name need not be rebuilt.
pub fn session_dir(home: &Path, id: &str) -> Option<PathBuf> {
    if id.is_empty() || id.contains(['/', '\\', '.']) {
        return None;
    }
    let name = format!("{id}.jsonl");
    fs::read_dir(home.join("projects"))
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|dir| dir.join(&name).is_file())
        .map(|dir| dir.join(id))
}

#[derive(Clone)]
struct Cached {
    stamp: (u64, u64),
    started_at: Option<u64>,
    last: Option<Step>,
    model: Option<String>,
}

/// Rereads only what changed between scans.
#[derive(Default)]
pub struct Scanner {
    transcripts: HashMap<PathBuf, Cached>,
    scripts: HashMap<PathBuf, ((u64, u64), Meta)>,
}

#[derive(Clone, Default, Debug, PartialEq, Eq)]
struct Meta {
    name: Option<String>,
    description: Option<String>,
    phases: Vec<String>,
}

impl Scanner {
    pub fn scan(&mut self, session: &Path, now: u64) -> Snapshot {
        let subagents = session.join("subagents");
        let mut agents = Vec::new();
        for path in files(&subagents, ".meta.json") {
            let Some(id) = file_stem(&path, ".meta.json") else {
                continue;
            };
            let meta: Value = read_json(&path).unwrap_or(Value::Null);
            let transcript = subagents.join(format!("{id}.jsonl"));
            let cached = self.transcript(&transcript);
            let updated_at = modified(&transcript).unwrap_or(0);
            let state = if meta["finished"] == true {
                State::Done
            } else if now.saturating_sub(updated_at) > STALE_MS {
                State::Stopped
            } else {
                State::Running
            };
            agents.push(Agent {
                id: id.trim_start_matches("agent-").into(),
                label: text(&meta["description"])
                    .or_else(|| text(&meta["agentType"]))
                    .unwrap_or("Subagent")
                    .into(),
                phase: None,
                state,
                model: text(&meta["model"])
                    .map(str::to_owned)
                    .or(cached.model.clone()),
                started_at: cached.started_at,
                updated_at,
                last: cached.last,
                summary: None,
            });
        }
        agents.sort_by_key(|a| std::cmp::Reverse(a.updated_at));

        let mut runs = Vec::new();
        for dir in dirs(&subagents.join("workflows")) {
            let Some(id) = dir.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
                continue;
            };
            let meta = self.script(&session.join("workflows").join("scripts"), &id);
            runs.push(self.run(&dir, id, meta, now));
        }
        runs.sort_by_key(|a| std::cmp::Reverse(a.updated_at));
        // Files that went away no longer need their cache.
        self.transcripts.retain(|path, _| path.exists());
        Snapshot { runs, agents }
    }

    fn run(&mut self, dir: &Path, id: String, meta: Meta, now: u64) -> Run {
        let journal = dir.join("journal.jsonl");
        let mut started: Vec<(String, String, Option<String>)> = Vec::new();
        let mut results: HashMap<String, Value> = HashMap::new();
        for line in fs::read_to_string(&journal).unwrap_or_default().lines() {
            let Ok(entry) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let Some(agent) = text(&entry["agentId"]) else {
                continue;
            };
            match entry["type"].as_str() {
                Some("started") if !started.iter().any(|(id, ..)| id == agent) => {
                    started.push((
                        agent.into(),
                        text(&entry["label"]).unwrap_or("Agent").into(),
                        text(&entry["phase"]).map(str::to_owned),
                    ));
                }
                Some("result") => {
                    results.insert(agent.into(), entry);
                }
                _ => {}
            }
        }
        let mut phases: Vec<Phase> = meta
            .phases
            .iter()
            .map(|title| Phase {
                title: title.clone(),
                agents: Vec::new(),
            })
            .collect();
        let mut updated_at = modified(&journal).unwrap_or(0);
        for (agent, label, phase) in started {
            let transcript = dir.join(format!("agent-{agent}.jsonl"));
            let cached = self.transcript(&transcript);
            let changed = modified(&transcript).unwrap_or(0);
            updated_at = updated_at.max(changed);
            let result = results.get(&agent);
            let state = match result {
                Some(entry) if failed(entry) => State::Failed,
                Some(_) => State::Done,
                None if now.saturating_sub(changed.max(updated_at)) > STALE_MS => State::Stopped,
                None => State::Running,
            };
            let title = phase.clone().unwrap_or_else(|| "Other".into());
            let at = match phases.iter().position(|p| p.title == title) {
                Some(at) => at,
                None => {
                    phases.push(Phase {
                        title,
                        agents: Vec::new(),
                    });
                    phases.len() - 1
                }
            };
            phases[at].agents.push(Agent {
                id: agent,
                label,
                phase,
                state,
                model: cached.model,
                started_at: cached.started_at,
                updated_at: changed,
                last: cached.last,
                summary: result.and_then(|entry| summary(&entry["result"])),
            });
        }
        Run {
            name: meta.name.unwrap_or_else(|| id.clone()),
            description: meta.description,
            id,
            phases,
            updated_at,
        }
    }

    fn transcript(&mut self, path: &Path) -> Cached {
        let stamp = stamp(path);
        if let Some(cached) = self.transcripts.get(path).filter(|c| c.stamp == stamp) {
            return cached.clone();
        }
        let (last, model) = latest(&read_tail(path, TAIL));
        let started_at = self
            .transcripts
            .get(path)
            .and_then(|c| c.started_at)
            .or_else(|| first_timestamp(&read_head(path, HEAD)));
        let cached = Cached {
            stamp,
            started_at,
            last,
            model,
        };
        self.transcripts.insert(path.to_owned(), cached.clone());
        cached
    }

    fn script(&mut self, scripts: &Path, run: &str) -> Meta {
        let suffix = format!("{run}.js");
        let Some(path) = files(scripts, ".js")
            .into_iter()
            .find(|path| path.to_string_lossy().ends_with(&suffix))
        else {
            return Meta::default();
        };
        let stamp = stamp(&path);
        if let Some((_, meta)) = self.scripts.get(&path).filter(|(s, _)| *s == stamp) {
            return meta.clone();
        }
        let meta = script_meta(&fs::read_to_string(&path).unwrap_or_default());
        self.scripts.insert(path, (stamp, meta.clone()));
        meta
    }
}

fn failed(entry: &Value) -> bool {
    !entry["error"].is_null()
        || entry["result"]["error"].is_string()
        || matches!(
            entry["status"].as_str(),
            Some("failed" | "error" | "cancelled")
        )
}

/// What an agent reported, on one line.
fn summary(result: &Value) -> Option<String> {
    let line = |s: &str| {
        s.lines()
            .map(|l| l.trim().trim_start_matches(['#', '*', '-', ' ']).trim())
            .find(|l| !l.is_empty())
            .map(|l| clip(l, 200))
    };
    if let Some(s) = result.as_str() {
        return line(s);
    }
    if let Some(s) = result["summary"].as_str() {
        return line(s);
    }
    // Structured output: say how many of each list came back.
    let counts: Vec<String> = result
        .as_object()?
        .iter()
        .filter_map(|(key, value)| Some(format!("{} {key}", value.as_array()?.len())))
        .collect();
    (!counts.is_empty()).then(|| counts.join(", "))
}

/// The newest step and model in a transcript's tail. The first line may be
/// cut in half; it simply fails to parse.
fn latest(tail: &str) -> (Option<Step>, Option<String>) {
    let mut model = None;
    for line in tail.lines().rev() {
        if !line.starts_with('{') || !line.contains("\"assistant\"") {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if entry["type"] != "assistant" {
            continue;
        }
        if model.is_none() {
            model = text(&entry["message"]["model"]).map(str::to_owned);
        }
        let blocks = entry["message"]["content"].as_array();
        for block in blocks.into_iter().flatten().rev() {
            match block["type"].as_str() {
                Some("tool_use") => {
                    let name = block["name"].as_str().unwrap_or("Tool");
                    return (
                        Some(Step {
                            tool: Some(name.into()),
                            text: tool_text(name, &block["input"]),
                        }),
                        model,
                    );
                }
                Some("text") => {
                    if let Some(line) = block["text"]
                        .as_str()
                        .and_then(|t| t.lines().map(str::trim).find(|l| !l.is_empty()))
                    {
                        return (
                            Some(Step {
                                tool: None,
                                text: clip(line, 200),
                            }),
                            model,
                        );
                    }
                }
                _ => {}
            }
        }
    }
    (None, model)
}

/// The part of a tool's input that says what it is doing.
fn tool_text(name: &str, input: &Value) -> String {
    let path = |key: &str| {
        text(&input[key]).map(|p| {
            Path::new(p)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.into())
        })
    };
    let found = match name {
        "Read" | "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
            path("file_path").or_else(|| path("notebook_path"))
        }
        "Grep" | "Glob" => text(&input["pattern"]).map(str::to_owned),
        "WebFetch" => text(&input["url"]).map(str::to_owned),
        "WebSearch" => text(&input["query"]).map(str::to_owned),
        _ => None,
    }
    .or_else(|| {
        ["command", "description", "prompt", "query", "path", "url"]
            .iter()
            .find_map(|key| text(&input[*key]).map(str::to_owned))
    })
    .unwrap_or_default();
    let line = found
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    clip(line, 200)
}

fn first_timestamp(head: &str) -> Option<u64> {
    head.lines().find_map(|line| {
        let at = line.find("\"timestamp\":\"")? + 13;
        let end = line[at..].find('"')?;
        iso_ms(&line[at..at + end])
    })
}

/// `2026-11-11T14:18:51.354Z` as Unix milliseconds.
fn iso_ms(text: &str) -> Option<u64> {
    let (date, time) = text.trim_end_matches('Z').split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.split(':');
    let (h, min) = (
        t.next()?.parse::<i64>().ok()?,
        t.next()?.parse::<i64>().ok()?,
    );
    let sec: f64 = t.next()?.parse().ok()?;
    // Days from the civil calendar (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let ms = ((days * 24 + h) * 60 + min) * 60_000 + (sec * 1000.).round() as i64;
    u64::try_from(ms).ok()
}

/// `name`, `description` and each phase `title` of `export const meta`.
fn script_meta(script: &str) -> Meta {
    let Some(start) = script.find("export const meta") else {
        return Meta::default();
    };
    let body = &script[start..];
    // The literal ends at the first line that closes it.
    let end = body
        .find("\n}")
        .map(|at| at + 2)
        .unwrap_or(body.len().min(16 * 1024));
    let body = &body[..end];
    let mut meta = Meta::default();
    let mut rest = body;
    while let Some((key, value, after)) = next_field(rest) {
        match key {
            "name" if meta.name.is_none() => meta.name = Some(value),
            "description" if meta.description.is_none() => meta.description = Some(value),
            "title" => meta.phases.push(value),
            _ => {}
        }
        rest = after;
    }
    meta
}

/// The next `key: 'value'` (any quote) in `text`, and what follows it.
fn next_field(text: &str) -> Option<(&str, String, &str)> {
    let mut search = text;
    loop {
        let colon = search.find(':')?;
        let key_start = search[..colon]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .map_or(0, |at| at + 1);
        let key = &search[key_start..colon];
        let after = search[colon + 1..].trim_start();
        let Some(quote) = after
            .chars()
            .next()
            .filter(|c| matches!(c, '\'' | '"' | '`'))
        else {
            search = &search[colon + 1..];
            continue;
        };
        let mut value = String::new();
        let mut escaped = false;
        for (at, c) in after[1..].char_indices() {
            if escaped {
                value.push(c);
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == quote {
                return Some((key, value, &after[1 + at + 1..]));
            } else {
                value.push(c);
            }
        }
        return None;
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.into();
    }
    let mut out: String = text.chars().take(max - 1).collect();
    out.push('…');
    out
}

fn text(value: &Value) -> Option<&str> {
    value.as_str().map(str::trim).filter(|s| !s.is_empty())
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn read_tail(path: &Path, bytes: u64) -> String {
    let Ok(mut file) = fs::File::open(path) else {
        return String::new();
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = file.seek(SeekFrom::Start(len.saturating_sub(bytes)));
    let mut buf = Vec::new();
    let _ = file.take(bytes).read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

fn read_head(path: &Path, bytes: u64) -> String {
    let Ok(file) = fs::File::open(path) else {
        return String::new();
    };
    let mut buf = Vec::new();
    let _ = file.take(bytes).read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

fn modified(path: &Path) -> Option<u64> {
    let time = fs::metadata(path).ok()?.modified().ok()?;
    Some(ms(time))
}

fn stamp(path: &Path) -> (u64, u64) {
    fs::metadata(path)
        .map(|m| (m.len(), m.modified().map(ms).unwrap_or(0)))
        .unwrap_or_default()
}

fn ms(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn files(dir: &Path, suffix: &str) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.to_string_lossy().ends_with(suffix))
                .collect()
        })
        .unwrap_or_default()
}

fn dirs(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default()
}

fn file_stem(path: &Path, suffix: &str) -> Option<String> {
    path.file_name()?
        .to_str()?
        .strip_suffix(suffix)
        .map(str::to_owned)
}

#[cfg(test)]
mod tests;
