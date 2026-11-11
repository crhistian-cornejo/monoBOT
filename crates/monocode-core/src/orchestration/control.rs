//! Lead-supervised orchestration (orchestration.ts, control.rs, control_cli.rs).
//! Once the user starts a reviewed plan, the lead drives its workers through
//! `monocode-gpui control ACTION`: list, delegate, get, wait, steer, message,
//! retry, cancel, respond, answer, review and finish. This module holds the
//! run, validates every action and decides what the app must do; the app owns
//! the sessions, worktrees and timers. The run is saved after every change so
//! an interrupted one can be resumed.
use super::isolation::{in_scope, Worker};
use super::{Summary, SummaryTask};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashSet};

pub const MAX_TASKS: usize = 40;
pub const MAX_WORKERS: usize = 4;
const MAX_REQUESTS: usize = 512;
pub const MAX_CONTINUATIONS: u32 = 20;
const RESULT_CHARS: usize = 20_000;

pub const ACTIONS: [&str; 15] = [
    "list", "delegate", "get", "wait", "steer", "message", "retry", "cancel", "respond", "answer",
    "review", "finish", "post", "board", "swarm",
];
/// What a worker may call: the board, and nothing that steers the run.
pub const WORKER_ACTIONS: [&str; 2] = ["post", "board"];

pub const HELP: &str = r#"monoBOT local control — supervise this orchestration run from the lead agent.

Usage: monocode-gpui control ACTION [--json JSON | --input FILE|-] [--request-id ID]

Actions, with the JSON object each one takes:
  list      {}
            The run, every task with its status and latest result, and the
            harness/model IDs you may assign.
  delegate  {"title":"Short title","harness":"<id from list>",
             "model":"<id from list>","prompt":"Self-contained instructions",
             "files":["src/feature"],"dependsOn":["<taskId>"],"handle":"verifier"}
            Queue a worker and return its taskId and board handle. "model" is
            optional and defaults to the first model list allows for that
            harness. "handle" is optional: a role name such as verifier or
            tester; workers are worker-1, worker-2... otherwise. For a
            handle the user pinned from /swarm, leave out harness and model.
            "files" is the write scope: project-relative paths, where a
            directory covers its descendants and ["."] reserves the whole
            checkout. "dependsOn" holds taskIds that must be reviewed first.
  get       {"taskId":"..."}
            One task, including its latest result.
  wait      {"timeoutSeconds":20}
            Block until a task changes state, or until the timeout (0-25).
            Returns at once when paused, stopped, or nothing is running or queued.
  respond   {"taskId":"...","requestId":"...","decision":"allow"|"deny"}
            Answer an approval an agent is blocked on. Agents never prompt the
            user; list, get and wait report the prompt as that task's
            "needsInput", and it stays stopped until you decide.
  answer    {"taskId":"...","requestId":"...","answers":{"<questionId>":["<option id or label>"]}}
            Answer a question an agent asked, or pass "skip":true instead of
            "answers". The question and its options come from needsInput.
  steer     {"taskId":"...","text":"..."}
            Redirect an agent that is still running, without discarding the
            work it has already done. message only lands once it has stopped.
  message   {"taskId":"...","text":"..."}
            Send a stopped worker another turn within its existing scope; it
            keeps its session, checkout and history.
  retry     {"taskId":"...","text":"...","files":["src/feature"]}
            Retry a stopped worker with corrected project-relative write
            scopes. Use this only when the additional files are required.
  cancel    {"taskId":"..."}
            Cancel a task, whether it is running or still queued.
  review    {"taskId":"..."}
            Accept a completed task's result. Its changes are applied from the
            worker's own worktree to your checkout; a write outside its scope
            is refused and the worktree kept.
  finish    {}
            End the run, once every task is accepted or cancelled.
  post      {"text":"@worker-2 the KeyRing API changed: use `Lookup(kid)`"}
            Post on the run's board, which the user watches live. Name
            members with @handle, or everyone with @all. A running member
            gets the post at once; a stopped one with its next turn; the
            coordinator when it is idle. Workers may call post and board.
  board     {"since":0,"limit":50}
            The members, their state, and the latest posts after "since".
  swarm     {"title":"Migrate auth to v2"}
            From an ordinary coding chat: turn this chat into the coordinator
            of a swarm for a large task, so you can delegate. Only when the
            user allows it in Settings > Chat; once a run is open, the other
            actions apply.

Usual loop: list -> delegate ... -> wait or get -> steer an agent that drifts,
unblock one with respond or answer -> inspect the changes yourself -> message
for corrections -> review each task -> finish.

When paused, list, get and wait still return the reason and recovery steps.
Do not keep polling or retry mutations. Explain the pause and ask the user to
click Resume in monoBOT. Resume continues interrupted workers in their
retained checkouts.

Output is one JSON line: {"ok":true,"result":...} or {"ok":false,"error":"..."}.
The exit code is 0 only when "ok" is true. Input must be a JSON object; unknown
fields are rejected rather than ignored. Every call carries a request ID and the
run applies each ID at most once: retry an uncertain call with --request-id ID.
Tasks run inside the monoBOT app; exiting this CLI never cancels them.
Credentials are already in your environment; never print them.
"#;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Queued,
    Running,
    Completed,
    Failed,
    Blocked,
    Interrupted,
    Cancelled,
}
impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
            Self::Interrupted => "interrupted",
            Self::Cancelled => "cancelled",
        }
    }
    fn stopped(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Blocked | Self::Interrupted
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    #[default]
    Active,
    Paused,
    Stopped,
    Finished,
}
impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Stopped => "stopped",
            Self::Finished => "finished",
        }
    }
}

/// A harness the lead may assign, with its model IDs. An empty list means the
/// harness's default model only.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Choice {
    pub provider: String,
    pub models: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Task {
    pub id: String,
    /// The plan card's assignment ID, for tasks the user approved.
    pub assignment: Option<String>,
    pub session: Option<i64>,
    pub title: String,
    pub provider: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub prompt: String,
    pub files: Vec<String>,
    pub depends_on: Vec<String>,
    /// The handle asked for on the board, such as "verifier".
    pub handle: Option<String>,
    pub status: Status,
    pub accepted: bool,
    pub result: String,
    pub error: Option<String>,
    /// The turn an interrupted worker resumes with.
    pub recovery: Option<String>,
    /// The lead has been told about the latest result.
    pub delivered: bool,
    /// The worker's own worktree; kept until its changes are applied or dropped.
    pub workspace: Option<Worker>,
    /// Counts dispatches: only the current one may settle the task.
    pub dispatch: u64,
    /// Files carried over to the lead when it was reviewed.
    pub integrated: Vec<String>,
}
impl Task {
    fn active(&self) -> bool {
        self.status == Status::Running
    }
}

/// What a worker is blocked on, read from its live session by the app.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NeedsInput {
    /// "approval" or "question".
    pub kind: &'static str,
    pub request_id: String,
    pub label: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<Question>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    pub id: String,
    pub prompt: String,
    pub multiple: bool,
    pub allow_custom: bool,
    pub options: Vec<QuestionOption>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct QuestionOption {
    pub id: String,
    pub label: String,
}

/// Work the app performs after an action is recorded.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    None,
    Steer {
        session: i64,
        text: String,
    },
    Stop {
        session: i64,
    },
    Respond {
        session: i64,
        request_id: String,
        allow: bool,
    },
    /// Answers per question, as option labels or custom text; None skips.
    Answer {
        session: i64,
        request_id: String,
        answers: Option<Vec<Vec<String>>>,
    },
    /// Apply an isolated worker's changes, then call `Run::accept`.
    Integrate {
        task: String,
    },
    /// Remove the worktrees of accepted tasks, and of the others only when
    /// they hold no changes.
    Cleanup {
        accepted: Vec<Worker>,
        unreviewed: Vec<Worker>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Receipt {
    pub fingerprint: String,
    pub result: Value,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Run {
    pub version: u32,
    pub lead: i64,
    /// The project the sessions belong to.
    pub project: String,
    /// The lead's checkout, where accepted changes land.
    pub checkout: String,
    /// Workers get their own worktrees (a local Git checkout).
    pub isolated: bool,
    pub status: RunStatus,
    pub max_workers: usize,
    pub choices: Vec<Choice>,
    /// The user's request, repeated to the lead on resume.
    pub request: String,
    pub tasks: Vec<Task>,
    pub error: Option<String>,
    pub last_pause_reason: Option<String>,
    pub continuations: u32,
    pub requests: BTreeMap<String, Receipt>,
    /// needsInput keys already put to the lead.
    pub announced: Vec<String>,
    pub next_id: u64,
    /// Started with /swarm: the lead plans and delegates without a card.
    pub swarm: bool,
    /// Harness and model the user pinned to a role from `/swarm`.
    pub roles: Vec<Role>,
}

/// A role the user gave a harness (and optionally a model) from `/swarm`,
/// such as `@verifier=codex/gpt-5.5`: delegating that handle uses them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Role {
    pub handle: String,
    pub provider: String,
    pub model: Option<String>,
}

/// The `@role=harness[/model]` words at the start of a `/swarm` request,
/// and the request without them.
pub fn swarm_roles(request: &str) -> Result<(Vec<Role>, String), String> {
    let mut roles: Vec<Role> = Vec::new();
    let mut rest = request.trim_start();
    while let Some(word) = rest.split_whitespace().next() {
        let Some(pin) = word.strip_prefix('@') else {
            break;
        };
        let Some((handle, target)) = pin.split_once('=') else {
            break;
        };
        let handle = super::board::handle_from(handle);
        let (provider, model) = match target.split_once('/') {
            Some((provider, model)) => (provider, Some(model.to_owned())),
            None => (target, None),
        };
        if handle.is_empty()
            || handle == "all"
            || handle == super::board::COORDINATOR
            || provider.is_empty()
            || model.as_deref() == Some("")
        {
            return Err(format!(
                "Write a role as @role=harness or @role=harness/model, such as @verifier=codex; \"{word}\" is not one."
            ));
        }
        if roles.iter().any(|r| r.handle == handle) {
            return Err(format!("@{handle} is given twice."));
        }
        roles.push(Role {
            handle,
            provider: provider.to_owned(),
            model,
        });
        rest = rest[word.len()..].trim_start();
    }
    Ok((roles, rest.to_owned()))
}

/// The key a run is saved under.
pub fn setting_key(lead: i64) -> String {
    format!("orchestration-run:{lead}")
}

fn text(input: &Value, key: &str, max: usize) -> Result<String, String> {
    match input.get(key).and_then(Value::as_str).map(str::trim) {
        Some(value) if !value.is_empty() && value.chars().count() <= max => Ok(value.to_owned()),
        Some(value) if !value.is_empty() => {
            Err(format!("Invalid {key}: keep it under {max} characters"))
        }
        _ => Err(format!("Invalid {key}: provide a non-empty string")),
    }
}
fn strings(input: &Value, key: &str, max: usize) -> Result<Vec<String>, String> {
    let Some(value) = input.get(key) else {
        return Ok(Vec::new());
    };
    let items = value
        .as_array()
        .ok_or_else(|| format!("Invalid {key}: provide an array of strings"))?;
    if items.len() > max {
        return Err(format!("Invalid {key}: at most {max} entries"));
    }
    let mut out = Vec::new();
    for item in items {
        let item = item
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty() && s.len() <= 512)
            .ok_or_else(|| format!("Invalid {key}: provide non-empty strings"))?;
        if !out.iter().any(|s| s == item) {
            out.push(item.to_owned());
        }
    }
    Ok(out)
}
fn listed(values: &[String]) -> String {
    if values.len() > 12 {
        format!("{} (+{} more)", values[..12].join(", "), values.len() - 12)
    } else {
        values.join(", ")
    }
}
/// Project-relative scopes only: no absolute paths, `..` or drive prefixes.
pub fn check_scopes(files: &[String]) -> Result<(), String> {
    if files.is_empty() {
        return Err(
            "Declare at least one file/directory scope in files, or '.' for exclusive checkout access"
                .into(),
        );
    }
    let task = super::Task {
        id: "scope".into(),
        title: "scope".into(),
        prompt: "scope".into(),
        files: files.to_vec(),
        ..Default::default()
    };
    super::validate(&super::Plan {
        title: "scope".into(),
        summary: String::new(),
        tasks: vec![task],
    })
    .map_err(|e| format!("{e}. Write scopes must be project-relative paths without '..'"))
}
fn fields(action: &str) -> Option<&'static [&'static str]> {
    Some(match action {
        "list" | "finish" => &[],
        "delegate" => &[
            "title",
            "harness",
            "model",
            "prompt",
            "files",
            "dependsOn",
            "handle",
        ],
        "post" => &["text"],
        "board" => &["since", "limit"],
        "swarm" => &["title"],
        "get" | "cancel" | "review" => &["taskId"],
        "wait" => &["timeoutSeconds"],
        "steer" | "message" => &["taskId", "text"],
        "retry" => &["taskId", "text", "files"],
        "respond" => &["taskId", "requestId", "decision"],
        "answer" => &["taskId", "requestId", "answers", "skip"],
        _ => return None,
    })
}
/// A mistyped field must fail loudly rather than silently change the task.
pub fn check_fields(action: &str, input: &Value) -> Result<(), String> {
    let allowed = fields(action).ok_or_else(|| {
        format!(
            "Unknown action \"{action}\". Use one of: {}. Run the control CLI with --help.",
            ACTIONS.join(", ")
        )
    })?;
    let object = input.as_object().ok_or("Input must be a JSON object")?;
    let unknown: Vec<String> = object
        .keys()
        .filter(|k| !allowed.contains(&k.as_str()))
        .cloned()
        .collect();
    if unknown.is_empty() {
        return Ok(());
    }
    Err(format!(
        "Unknown {action} field{}: {}. {}",
        if unknown.len() > 1 { "s" } else { "" },
        unknown.join(", "),
        if allowed.is_empty() {
            format!("{action} takes no input.")
        } else {
            format!("{action} accepts: {}.", allowed.join(", "))
        }
    ))
}
/// Whether two scope lists can touch the same file.
pub fn overlap(a: &[String], b: &[String]) -> bool {
    a.iter().any(|x| in_scope(x, b)) || b.iter().any(|x| in_scope(x, a))
}

/// The turn a worker receives: its task, plus rules the transcript keeps.
pub fn worker_prompt(prompt: &str, files: &[String], isolated: bool) -> String {
    let checkout = if isolated {
        "You work in your own copy of the project, made for this assignment; the lead applies your changes after review."
    } else {
        "Other workers may share this checkout; preserve their changes and never reset the repository."
    };
    format!(
        "{prompt}\n\n<monocode_assignment>\nYou are a worker managed by a monoBOT lead. {checkout} Your assigned write scope is: {}. Read other files as needed, but do not edit outside your scope. If another file or shared operation is needed, report the blocker and stop so the lead can expand or create a new assignment. Do not spawn agents, create worktrees, switch branches, stage, commit, push, install dependencies or run broad formatters/generators. Report focused checks, changed files, remaining issues and a concise final result.\n</monocode_assignment>",
        files.join(", ")
    )
}

/// The turn an interrupted worker resumes with.
pub fn recovery_turn(reason: &str) -> String {
    format!(
        "Continue the existing assignment from its retained worker checkout. The previous turn was stopped because the orchestration run was interrupted: {reason}\n\nInspect the current files and prior conversation before acting. Preserve completed work, do not repeat destructive or external operations, remain inside the assigned write scope, run the remaining focused checks, and report what was already done versus what you completed now."
    )
}

/// What the lead is told about supervising, appended to its turns.
pub fn lead_instructions(cli: &str) -> String {
    let cli = shell_path(cli);
    format!(
        "<monocode_orchestration>\nYou are the lead of a local monoBOT run. Coordinate the user's task using `{cli} control`. Run `{cli} control --help` before your first command; it documents every action, its exact JSON fields and the retry rule. Credentials are already in your environment; never print them.\nEach call prints one JSON line and exits non-zero unless \"ok\" is true; read the \"error\" text, it says what to do next. Unknown JSON fields are rejected. If a call fails before reaching monoBOT, retry it with the same --request-id so the work is never queued twice.\nUse list to discover allowed harness/model IDs. Delegate bounded tasks with project-relative files (directories reserve their descendants), self-contained prompts and dependsOn task IDs. You may read and plan; leave project file edits to workers. Never start workers outside this CLI. Workers with overlapping files are queued.\nAgents never prompt the user. When one needs an approval or answers a question, list, get and wait report it as needsInput on that task, and you decide with respond or answer; it stays stopped until you do. Put it to the user in this conversation only when the call is genuinely theirs.\nSteer a running agent with steer; use message once it has stopped. completed means a turn finished, not that the work passed review. Review the actual changes (each worker has its own worktree, reported as workspace in get), message a worker for fixes, and use review to accept each completed task: that applies its changes to your checkout. Call finish only when required work and combined validation are complete. You receive worker results automatically when idle; use bounded wait calls while supervising. If the run is paused, stop polling, report the reason and ask the user to click Resume.\nThe run has a live board the user watches: you are @coordinator, and each worker has a handle (delegate returns it). Use post to brief several workers at once, to connect a worker with the one whose work it depends on, or to hand finished work to a verifier; workers post there too, and what they address to you reaches you when you are idle.\n</monocode_orchestration>"
    )
}

/// What a worker of a run is told about its board, appended to its turns.
pub fn board_instructions(cli: &str, handle: &str, members: &[String]) -> String {
    let cli = shell_path(cli);
    format!(
        "<monocode_board>\nYou are @{handle} on this run's board, which the user and the other agents watch live. Members: {}. Post short, concrete updates others need (an interface you changed, a blocker, a finding for the verifier) with `{cli} control post --json '{{\"text\":\"@handle ...\"}}'`, naming members with @handle or everyone with @all, and read it with `{cli} control board`. Posts addressed to you arrive with your turns. Do not post routine progress; your final report reaches @coordinator by itself. Credentials are already in your environment; never print them.\n</monocode_board>",
        members
            .iter()
            .map(|m| format!("@{m}"))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// The lead's first turn of a /swarm run: the user's request, with the
/// rules for coordinating it.
pub fn swarm_prompt(request: &str, cli: &str, roles: &[Role]) -> String {
    format!(
        "{request}\n\nThe user started a swarm for this request.\n\n{}\n\n{}",
        swarm_rules(roles),
        lead_instructions(cli)
    )
}

/// What a coding chat is told once when the user lets models start swarms:
/// it may turn itself into a coordinator for a large task.
pub fn swarm_offer(cli: &str) -> String {
    let cli = shell_path(cli);
    format!(
        "<monocode_swarm_option>\nFor a large task that splits into independent parts (several areas of the code, or building and verifying in parallel), you may turn this chat into the coordinator of a swarm: run `{cli} control swarm --json '{{\"title\":\"Short task title\"}}'`. It answers with the rules for delegating to parallel workers, each in its own worktree, on a board the user watches. Do it only when parallel agents clearly beat working alone; a small or sequential task stays here. Credentials are already in your environment; never print them.\n</monocode_swarm_option>"
    )
}

/// What the `swarm` action answers once the chat is a coordinator: the
/// rules of the run, so the model can delegate in the same turn.
pub fn swarm_started(title: &str, cli: &str, roles: &[Role]) -> Value {
    json!({
        "started": true,
        "title": title,
        "handle": super::board::COORDINATOR,
        "instructions": format!("{}\n\n{}", swarm_rules(roles), lead_instructions(cli)),
    })
}

/// How the coordinator should run the swarm, with any roles the user pinned.
fn swarm_rules(roles: &[Role]) -> String {
    let pinned = if roles.is_empty() {
        String::new()
    } else {
        format!(
            " The user pinned these roles: {}. Delegate each with its handle; its harness and model are filled in, so omit them.",
            roles
                .iter()
                .map(|r| match &r.model {
                    Some(model) => format!("@{} on {} with {model}", r.handle, r.provider),
                    None => format!("@{} on {}", r.handle, r.provider),
                })
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    format!(
        "<monocode_swarm>\nYou are this swarm's coordinator. Inspect the project with read-only tools, then split the work into bounded assignments with disjoint write scopes and delegate them now through the control CLI; do not ask the user to approve a plan. Give each worker a handle that says its role (for example implementer, tester, verifier), run independent work in parallel (up to {MAX_WORKERS} at once) and add a verifier that runs the tests across the combined result once the builders are reviewed.{pinned} Each worker works in its own temporary branch and worktree; review applies its changes to your checkout and removes them, so nothing is left behind. Leave project edits to the workers. When everything is reviewed and verified, call finish and summarize what changed.\n</monocode_swarm>"
    )
}

/// Quote the executable for a POSIX shell only when needed.
pub fn shell_path(path: &str) -> String {
    if path
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"._/:-".contains(&b))
    {
        path.to_owned()
    } else if path.starts_with('/') {
        format!("'{}'", path.replace('\'', r"'\''"))
    } else {
        format!("\"{}\"", path.replace('"', ""))
    }
}

impl Run {
    /// A run from the plan card the user approved. Every task is queued.
    #[allow(clippy::too_many_arguments)]
    pub fn from_plan(
        lead: i64,
        project: &str,
        checkout: &str,
        isolated: bool,
        request: &str,
        plan: &super::Plan,
        lead_provider: &str,
        lead_model: Option<&str>,
        choices: Vec<Choice>,
        max_workers: usize,
    ) -> Result<Self, String> {
        super::validate(plan)?;
        let mut run = Self {
            version: 1,
            lead,
            project: project.into(),
            checkout: checkout.into(),
            isolated,
            status: RunStatus::Active,
            max_workers: max_workers.clamp(1, MAX_WORKERS),
            choices,
            request: request.into(),
            ..Default::default()
        };
        let mut ids = BTreeMap::new();
        for task in &plan.tasks {
            ids.insert(task.id.clone(), run.new_id());
        }
        for task in &plan.tasks {
            let provider = task
                .provider
                .clone()
                .unwrap_or_else(|| lead_provider.to_owned());
            let model = task.model.clone().or_else(|| {
                (provider == lead_provider)
                    .then(|| lead_model.map(str::to_owned))
                    .flatten()
            });
            run.tasks.push(Task {
                id: ids[&task.id].clone(),
                assignment: Some(task.id.clone()),
                title: task.title.clone(),
                provider,
                model,
                effort: task.effort.clone(),
                prompt: task.prompt.clone(),
                files: task.files.clone(),
                depends_on: task.depends_on.iter().map(|d| ids[d].clone()).collect(),
                delivered: true,
                ..Default::default()
            });
        }
        Ok(run)
    }

    /// A run started with /swarm: no tasks yet; the lead plans and
    /// delegates them itself.
    pub fn swarm(
        lead: i64,
        project: &str,
        checkout: &str,
        isolated: bool,
        request: &str,
        choices: Vec<Choice>,
        max_workers: usize,
    ) -> Self {
        Self {
            version: 1,
            lead,
            project: project.into(),
            checkout: checkout.into(),
            isolated,
            status: RunStatus::Active,
            max_workers: max_workers.clamp(1, MAX_WORKERS),
            choices,
            request: request.into(),
            swarm: true,
            ..Default::default()
        }
    }

    /// Every pinned role names a harness and model this run may use.
    pub fn check_roles(&self) -> Result<(), String> {
        for role in &self.roles {
            let Some(choice) = self.choices.iter().find(|c| c.provider == role.provider) else {
                return Err(format!(
                    "@{} asks for \"{}\", which is not available here. Harnesses: {}.",
                    role.handle,
                    role.provider,
                    listed(
                        &self
                            .choices
                            .iter()
                            .map(|c| c.provider.clone())
                            .collect::<Vec<_>>()
                    )
                ));
            };
            if let Some(model) = role.model.as_ref().filter(|m| !choice.models.contains(m)) {
                return Err(format!(
                    "@{} asks for {} with \"{model}\". Its models: {}.",
                    role.handle,
                    role.provider,
                    if choice.models.is_empty() {
                        "only its default; leave the model out".into()
                    } else {
                        listed(&choice.models)
                    }
                ));
            }
        }
        Ok(())
    }

    fn new_id(&mut self) -> String {
        self.next_id += 1;
        format!("t{}", self.next_id)
    }
    pub fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }
    pub fn task_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }
    pub fn worker(&self, session: i64) -> Option<&Task> {
        self.tasks.iter().find(|t| t.session == Some(session))
    }
    /// Live: dispatching or with a worker still running.
    pub fn live(&self) -> bool {
        self.status == RunStatus::Active || self.tasks.iter().any(Task::active)
    }

    fn target(&self, input: &Value) -> Result<&Task, String> {
        let id = text(input, "taskId", 128)?;
        self.task(&id).ok_or_else(|| {
            "Task does not belong to this run. Use a taskId returned by delegate or list.".into()
        })
    }

    /// The result recorded for this request ID, if it was already applied.
    pub fn receipt(&self, request_id: &str, fingerprint: &str) -> Option<Result<Value, String>> {
        let receipt = self.requests.get(request_id)?;
        Some(if receipt.fingerprint == fingerprint {
            Ok(receipt.result.clone())
        } else {
            Err("Request ID was already used with different input".into())
        })
    }
    pub fn record(&mut self, request_id: &str, fingerprint: &str, result: &Value) {
        self.requests.insert(
            request_id.into(),
            Receipt {
                fingerprint: fingerprint.into(),
                result: result.clone(),
            },
        );
    }

    pub fn inactive_reason(&self) -> String {
        if self.status == RunStatus::Paused {
            format!(
                "This run is paused. {} list, get and wait remain available for inspection. Do not retry mutations or keep polling: explain the pause and ask the user to click Resume in monoBOT. Resume continues interrupted tasks from their retained worker checkouts.",
                self.error.as_deref().unwrap_or("Work was interrupted.")
            )
        } else {
            format!(
                "This run is {}. Inspect results with list or get; do not keep retrying commands for this run.",
                self.status.as_str()
            )
        }
    }

    pub fn waiting_for(&self, task: &Task) -> Option<String> {
        if task.status != Status::Queued {
            return None;
        }
        if let Some(dep) = self
            .tasks
            .iter()
            .find(|t| task.depends_on.contains(&t.id) && !t.accepted)
        {
            return Some(format!("Waiting for review: {}", dep.title));
        }
        if let Some(owner) = self
            .tasks
            .iter()
            .find(|t| t.active() && overlap(&t.files, &task.files))
        {
            return Some(format!("Waiting for files: {}", owner.title));
        }
        Some("Waiting for a worker slot".into())
    }

    fn task_view(&self, task: &Task, needs: Option<NeedsInput>, prompt: bool) -> Value {
        let mut view = json!({
            "taskId": task.id,
            "title": task.title,
            "harness": task.provider,
            "model": task.model,
            "files": task.files,
            "dependsOn": task.depends_on,
            "status": task.status.as_str(),
            "accepted": task.accepted,
            "result": task.result,
            "sessionId": task.session,
        });
        let map = view.as_object_mut().unwrap();
        let mut put = |key: &str, value: Value| {
            if !value.is_null() {
                map.insert(key.into(), value);
            }
        };
        put("assignmentId", json!(task.assignment));
        put("effort", json!(task.effort));
        put("error", json!(task.error));
        put("waitingFor", json!(self.waiting_for(task)));
        put(
            "needsInput",
            serde_json::to_value(needs).unwrap_or(Value::Null),
        );
        put(
            "workspace",
            json!(task.workspace.as_ref().map(|w| w.checkout.clone())),
        );
        if !task.integrated.is_empty() {
            put("integratedFiles", json!(task.integrated));
        }
        if prompt {
            put("prompt", json!(task.prompt));
        }
        view
    }

    /// What list and wait return.
    pub fn view(&self, needs: &dyn Fn(&Task) -> Option<NeedsInput>) -> Value {
        let mut view = json!({
            "status": self.status.as_str(),
            "checkout": self.checkout,
            "isolatedWorkers": self.isolated,
            "maxWorkers": self.max_workers,
            "tasks": self.tasks.iter().map(|t| self.task_view(t, needs(t), false)).collect::<Vec<_>>(),
        });
        if self.status != RunStatus::Active {
            view["recovery"] = self.inactive_reason().into();
        }
        view
    }

    /// Whether `wait` should block: workers can still progress without the lead.
    pub fn waitable(&self, needs: &dyn Fn(&Task) -> Option<NeedsInput>) -> bool {
        self.status == RunStatus::Active
            && self.blocked_keys(needs).is_empty()
            && self
                .tasks
                .iter()
                .any(|t| t.active() || t.status == Status::Queued)
    }

    pub fn blocked_keys(&self, needs: &dyn Fn(&Task) -> Option<NeedsInput>) -> Vec<String> {
        self.tasks
            .iter()
            .filter_map(|t| needs(t).map(|n| format!("{}:{}", t.id, n.request_id)))
            .collect()
    }

    /// Apply one lead action. Reads (list, get) work on any run; mutations
    /// need an active one. `wait` is the app's: it blocks on run changes.
    pub fn execute(
        &mut self,
        action: &str,
        input: &Value,
        needs: &dyn Fn(&Task) -> Option<NeedsInput>,
    ) -> Result<(Value, Effect), String> {
        check_fields(action, input)?;
        if self.status != RunStatus::Active && !matches!(action, "list" | "get" | "wait") {
            return Err(self.inactive_reason());
        }
        if !matches!(action, "list" | "get" | "wait") && self.requests.len() >= MAX_REQUESTS {
            return Err(
                "Run command limit reached. Stop the run and start another after reviewing the files."
                    .into(),
            );
        }
        match action {
            "list" => Ok((
                json!({
                    "run": self.view(needs),
                    "harnesses": self.choices.iter().map(|c| json!({"harness":c.provider,"models":c.models})).collect::<Vec<_>>(),
                }),
                Effect::None,
            )),
            "wait" => {
                let seconds = input.get("timeoutSeconds").map_or(Some(20.), Value::as_f64);
                if !seconds.is_some_and(|s| (0.0..=25.0).contains(&s)) {
                    return Err("timeoutSeconds must be 0 to 25".into());
                }
                Ok((self.view(needs), Effect::None))
            }
            "get" => {
                let task = self.target(input)?;
                let mut view = self.task_view(task, needs(task), true);
                view["runStatus"] = self.status.as_str().into();
                if self.status != RunStatus::Active {
                    view["recovery"] = self.inactive_reason().into();
                }
                Ok((view, Effect::None))
            }
            "delegate" => self.delegate(input),
            "message" | "retry" => {
                let id = self.target(input)?.id.clone();
                let task = self.task(&id).unwrap();
                if task.active() || task.status == Status::Queued {
                    return Err(format!(
                        "Wait for this worker or cancel it before sending a new turn; {} is {}.",
                        task.title,
                        task.status.as_str()
                    ));
                }
                if task.status == Status::Cancelled {
                    return Err(format!(
                        "{} was cancelled; delegate a new task instead.",
                        task.title
                    ));
                }
                if self.tasks.iter().any(|t| {
                    t.depends_on.contains(&id)
                        && !matches!(t.status, Status::Queued | Status::Cancelled)
                }) {
                    return Err("A dependent task has already started; create a separate correction task after it finishes".into());
                }
                let prompt = text(input, "text", 30_000)?;
                let files = if action == "retry" {
                    let files = strings(input, "files", 64)?;
                    check_scopes(&files)?;
                    Some(files)
                } else {
                    None
                };
                let task = self.task_mut(&id).unwrap();
                task.prompt = prompt;
                if let Some(files) = files.clone() {
                    task.files = files;
                }
                task.status = Status::Queued;
                task.accepted = false;
                task.result.clear();
                task.error = None;
                task.recovery = None;
                task.delivered = true;
                let mut result = json!({"taskId": id, "status": "queued"});
                if let Some(files) = files {
                    result["files"] = json!(files);
                }
                Ok((result, Effect::None))
            }
            "steer" => {
                let task = self.target(input)?;
                if task.status != Status::Running {
                    return Err(format!(
                        "Only a running agent can be steered; {} is {}. {}",
                        task.title,
                        task.status.as_str(),
                        if task.status == Status::Queued {
                            "It has not started yet, so edit it with message instead."
                        } else {
                            "Send it a fresh turn with message."
                        }
                    ));
                }
                let session = task
                    .session
                    .ok_or("This agent is still starting; retry shortly.")?;
                let id = task.id.clone();
                let guidance = text(input, "text", 30_000)?;
                Ok((
                    json!({"taskId": id, "steered": true}),
                    Effect::Steer {
                        session,
                        text: guidance,
                    },
                ))
            }
            "cancel" => {
                let id = self.target(input)?.id.clone();
                let session = self.cancel(&id);
                Ok((
                    json!({"cancelled": true}),
                    session.map_or(Effect::None, |session| Effect::Stop { session }),
                ))
            }
            "respond" => {
                let task = self.target(input)?;
                let pending = needs(task).filter(|n| n.kind == "approval").ok_or_else(|| {
                    format!("{} is not waiting on an approval. Read needsInput from list or wait before responding.", task.title)
                })?;
                if request_id(input)? != pending.request_id {
                    return Err(format!(
                        "Stale requestId. {} is waiting on {}.",
                        task.title, pending.request_id
                    ));
                }
                let decision = text(input, "decision", 16)?;
                let allow = match decision.as_str() {
                    "allow" => true,
                    "deny" => false,
                    _ => return Err("decision must be \"allow\" or \"deny\"".into()),
                };
                Ok((
                    json!({"taskId": task.id, "decision": decision}),
                    Effect::Respond {
                        session: task.session.ok_or("This agent has no session")?,
                        request_id: pending.request_id,
                        allow,
                    },
                ))
            }
            "answer" => {
                let task = self.target(input)?;
                let pending = needs(task).filter(|n| n.kind == "question").ok_or_else(|| {
                    format!("{} is not waiting on a question. Read needsInput from list or wait before answering.", task.title)
                })?;
                if request_id(input)? != pending.request_id {
                    return Err(format!(
                        "Stale requestId. {} is waiting on {}.",
                        task.title, pending.request_id
                    ));
                }
                let answers = if input.get("skip") == Some(&Value::Bool(true)) {
                    None
                } else {
                    Some(question_answers(input.get("answers"), &pending.questions)?)
                };
                Ok((
                    json!({"taskId": task.id, "answered": answers.is_some()}),
                    Effect::Answer {
                        session: task.session.ok_or("This agent has no session")?,
                        request_id: pending.request_id,
                        answers,
                    },
                ))
            }
            "review" => {
                let task = self.target(input)?;
                if task.status != Status::Completed {
                    return Err(format!(
                        "Only a completed result can be accepted; {} is {}. {}",
                        task.title,
                        task.status.as_str(),
                        if matches!(
                            task.status,
                            Status::Failed | Status::Blocked | Status::Interrupted
                        ) {
                            "Send it another turn with message, retry it with corrected scope when necessary, or drop it with cancel."
                        } else {
                            "Wait for it to finish, or cancel it."
                        }
                    ));
                }
                if task.accepted {
                    return Ok((
                        json!({"accepted": true, "integrated": !task.integrated.is_empty()}),
                        Effect::None,
                    ));
                }
                if task.workspace.is_some() {
                    return Ok((
                        Value::Null,
                        Effect::Integrate {
                            task: task.id.clone(),
                        },
                    ));
                }
                let id = task.id.clone();
                Ok((self.accept(&id, Vec::new()), Effect::None))
            }
            "finish" => {
                let outstanding: Vec<String> = self
                    .tasks
                    .iter()
                    .filter(|t| t.status != Status::Cancelled && !t.accepted)
                    .map(|t| format!("{} ({})", t.title, t.status.as_str()))
                    .collect();
                if !outstanding.is_empty() {
                    return Err(format!(
                        "Review all remaining tasks before finishing. Outstanding: {}. Accept a completed task with review, correct a failed or blocked task with message/retry, or drop it with cancel.",
                        listed(&outstanding)
                    ));
                }
                let (accepted, unreviewed) = self.release_workers();
                self.status = RunStatus::Finished;
                Ok((
                    json!({"finished": true}),
                    Effect::Cleanup {
                        accepted,
                        unreviewed,
                    },
                ))
            }
            "post" | "board" => Err("The board is not available for this run.".into()),
            "swarm" => Err("This chat already coordinates a run; delegate within it.".into()),
            _ => Err("Unknown action. Run control --help.".into()),
        }
    }

    fn delegate(&mut self, input: &Value) -> Result<(Value, Effect), String> {
        if self.tasks.len() >= MAX_TASKS {
            return Err(format!("This run has reached its {MAX_TASKS}-task limit"));
        }
        let handle = match input.get("handle") {
            None | Some(Value::Null) => None,
            Some(_) => {
                let handle = super::board::handle_from(&text(input, "handle", 32)?);
                if handle.is_empty() || handle == "all" || handle == super::board::COORDINATOR {
                    return Err(
                        "Invalid handle: use a short role name such as verifier or tester".into(),
                    );
                }
                Some(handle)
            }
        };
        let pinned = handle
            .as_ref()
            .and_then(|h| self.roles.iter().find(|r| &r.handle == h))
            .cloned();
        let harness = match (&pinned, input.get("harness")) {
            (Some(role), None | Some(Value::Null)) => role.provider.clone(),
            (Some(role), Some(_)) => {
                let harness = text(input, "harness", 64)?;
                if harness != role.provider {
                    return Err(format!(
                        "The user pinned @{} to {}; omit harness or use it.",
                        role.handle, role.provider
                    ));
                }
                harness
            }
            (None, _) => text(input, "harness", 64)?,
        };
        let choice = self
            .choices
            .iter()
            .find(|c| c.provider == harness)
            .ok_or_else(|| {
                format!(
                    "Harness \"{harness}\" is not allowed in this run. Allowed: {}.",
                    listed(
                        &self
                            .choices
                            .iter()
                            .map(|c| c.provider.clone())
                            .collect::<Vec<_>>()
                    )
                )
            })?;
        let pinned_model = pinned.as_ref().and_then(|r| r.model.clone());
        let model = match (input.get("model"), pinned_model) {
            (None | Some(Value::Null), Some(model)) => Some(model),
            (None | Some(Value::Null), None) => choice.models.first().cloned(),
            (Some(_), pinned_model) => {
                let model = text(input, "model", 256)?;
                if let Some(pinned_model) = pinned_model.filter(|p| *p != model) {
                    return Err(format!(
                        "The user pinned @{} to {pinned_model}; omit model or use it.",
                        handle.as_deref().unwrap_or_default()
                    ));
                }
                if !choice.models.contains(&model) {
                    return Err(format!(
                        "Choose a model ID returned by list for {harness}: {}.",
                        if choice.models.is_empty() {
                            "omit model to use its default".into()
                        } else {
                            listed(&choice.models)
                        }
                    ));
                }
                Some(model)
            }
        };
        let title = text(input, "title", 160)?;
        let prompt = text(input, "prompt", 30_000)?;
        let files = strings(input, "files", 64)?;
        check_scopes(&files)?;
        let depends_on = strings(input, "dependsOn", 40)?;
        let missing: Vec<String> = depends_on
            .iter()
            .filter(|id| {
                !self
                    .tasks
                    .iter()
                    .any(|t| &&t.id == id && t.status != Status::Cancelled)
            })
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                "dependsOn must hold taskIds from this run; unknown or cancelled: {}.",
                listed(&missing)
            ));
        }
        let id = self.new_id();
        self.tasks.push(Task {
            id: id.clone(),
            title,
            provider: harness,
            model,
            prompt,
            files,
            depends_on,
            handle,
            delivered: true,
            ..Default::default()
        });
        Ok((json!({"taskId": id, "status": "queued"}), Effect::None))
    }

    /// Mark a reviewed task accepted, with the files applied to the lead.
    pub fn accept(&mut self, id: &str, integrated: Vec<String>) -> Value {
        let Some(task) = self.task_mut(id) else {
            return json!({"accepted": false});
        };
        task.accepted = true;
        let isolated = task.workspace.is_some();
        task.integrated = integrated;
        json!({"accepted": true, "integrated": isolated, "files": task.integrated})
    }

    /// Worktrees of stopped tasks, split by whether they were accepted.
    pub fn release_workers(&mut self) -> (Vec<Worker>, Vec<Worker>) {
        let mut accepted = Vec::new();
        let mut unreviewed = Vec::new();
        for task in self.tasks.iter_mut().filter(|t| !t.active()) {
            if let Some(worker) = task.workspace.take() {
                if task.accepted {
                    accepted.push(worker);
                } else {
                    unreviewed.push(worker);
                }
            }
        }
        (accepted, unreviewed)
    }

    /// Cancel a task; returns the session to stop when it was running.
    pub fn cancel(&mut self, id: &str) -> Option<i64> {
        let task = self.task_mut(id)?;
        if task.status == Status::Cancelled {
            return None;
        }
        let session = task.active().then_some(task.session).flatten();
        task.status = Status::Cancelled;
        task.delivered = true;
        // Queued dependants can never start now.
        let id = id.to_owned();
        let dependants: Vec<String> = self
            .tasks
            .iter()
            .filter(|t| t.status == Status::Queued && t.depends_on.contains(&id))
            .map(|t| t.id.clone())
            .collect();
        for dependant in dependants {
            if let Some(task) = self.task_mut(&dependant) {
                task.status = Status::Blocked;
                task.error = Some("A task it depends on was cancelled.".into());
                task.delivered = false;
            }
        }
        session
    }

    /// Tasks to start now: dependencies accepted, scopes free, within the
    /// worker limit. Each is marked running under a new dispatch.
    pub fn schedule(&mut self) -> Vec<String> {
        if self.status != RunStatus::Active {
            return Vec::new();
        }
        let accepted: HashSet<String> = self
            .tasks
            .iter()
            .filter(|t| t.accepted)
            .map(|t| t.id.clone())
            .collect();
        let mut occupied: Vec<Vec<String>> = self
            .tasks
            .iter()
            .filter(|t| t.active())
            .map(|t| t.files.clone())
            .collect();
        let mut started = Vec::new();
        for task in &mut self.tasks {
            if occupied.len() >= self.max_workers {
                break;
            }
            if task.status != Status::Queued
                || !task.depends_on.iter().all(|d| accepted.contains(d))
                || occupied.iter().any(|files| overlap(files, &task.files))
            {
                continue;
            }
            task.status = Status::Running;
            task.dispatch += 1;
            occupied.push(task.files.clone());
            started.push(task.id.clone());
        }
        started
    }

    /// Record a worker's session and worktree once it is prepared.
    pub fn prepared(
        &mut self,
        id: &str,
        dispatch: u64,
        session: i64,
        workspace: Option<Worker>,
    ) -> bool {
        match self.task_mut(id) {
            Some(task) if task.active() && task.dispatch == dispatch => {
                task.session = Some(session);
                if workspace.is_some() {
                    task.workspace = workspace;
                }
                true
            }
            _ => false,
        }
    }

    /// A worker's turn ended. Ignored unless it is the current dispatch.
    pub fn settle(
        &mut self,
        id: &str,
        dispatch: u64,
        success: bool,
        result: &str,
        error: Option<String>,
    ) -> bool {
        let Some(task) = self.task_mut(id) else {
            return false;
        };
        if !task.active() || task.dispatch != dispatch {
            return false;
        }
        task.status = if success {
            Status::Completed
        } else {
            Status::Failed
        };
        let start = result
            .char_indices()
            .rev()
            .nth(RESULT_CHARS)
            .map_or(0, |(ix, _)| ix);
        task.result = result[start..].to_owned();
        task.error = error;
        task.recovery = None;
        task.delivered = false;
        task.accepted = false;
        true
    }

    /// Pause on an interruption: running workers stop and resume later.
    /// Returns their sessions.
    pub fn pause(&mut self, reason: &str) -> Vec<i64> {
        if self.status != RunStatus::Active {
            return Vec::new();
        }
        self.status = RunStatus::Paused;
        self.error = Some(reason.into());
        self.last_pause_reason = Some(reason.into());
        let mut sessions = Vec::new();
        for task in &mut self.tasks {
            if task.active() {
                task.status = Status::Interrupted;
                task.delivered = true;
                task.recovery = Some(recovery_turn(reason));
                sessions.extend(task.session);
            }
        }
        sessions
    }

    /// Continue a paused run: interrupted workers are queued again with their
    /// recovery turn, in their retained worktrees.
    pub fn resume(&mut self) -> Result<(), String> {
        if self.status != RunStatus::Paused {
            return Err(format!("This run is {}, not paused.", self.status.as_str()));
        }
        let reason = self
            .error
            .clone()
            .unwrap_or_else(|| "the run was paused".into());
        for task in &mut self.tasks {
            if task.status == Status::Interrupted {
                task.status = Status::Queued;
                task.error = None;
                task.accepted = false;
                task.delivered = true;
                task.recovery.get_or_insert_with(|| recovery_turn(&reason));
            }
        }
        self.status = RunStatus::Active;
        self.error = None;
        self.continuations = 0;
        self.announced.clear();
        Ok(())
    }

    /// End the run without review. Returns running sessions to stop.
    pub fn stop(&mut self) -> Vec<i64> {
        let mut sessions = Vec::new();
        for task in &mut self.tasks {
            if task.active() {
                sessions.extend(task.session);
            }
            if matches!(
                task.status,
                Status::Running | Status::Queued | Status::Interrupted
            ) {
                task.status = Status::Cancelled;
            }
        }
        self.status = RunStatus::Stopped;
        sessions
    }

    /// The prompt the lead gets once idle: undelivered results and requests it
    /// has not seen. Marks them delivered. Err when the continuation limit
    /// stops the run.
    pub fn continuation(
        &mut self,
        needs: &dyn Fn(&Task) -> Option<NeedsInput>,
    ) -> Result<Option<String>, String> {
        if self.status != RunStatus::Active {
            return Ok(None);
        }
        let results: Vec<usize> = self
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.delivered && t.status.stopped())
            .map(|(ix, _)| ix)
            .collect();
        let waiting: Vec<(usize, NeedsInput, String)> = self
            .tasks
            .iter()
            .enumerate()
            .filter_map(|(ix, t)| {
                let pending = needs(t)?;
                let key = format!("{}:{}", t.id, pending.request_id);
                (!self.announced.contains(&key)).then_some((ix, pending, key))
            })
            .collect();
        if results.is_empty() && waiting.is_empty() {
            return Ok(None);
        }
        if self.continuations >= MAX_CONTINUATIONS {
            return Err("Automatic continuation limit reached. Its agents were stopped; review and resume the run.".into());
        }
        self.continuations += 1;
        let summary = results
            .iter()
            .map(|&ix| {
                let t = &mut self.tasks[ix];
                t.delivered = true;
                let tail_start = t
                    .result
                    .char_indices()
                    .rev()
                    .nth(4_000)
                    .map_or(0, |(i, _)| i);
                format!(
                    "{} — {}: {}\n{}\n{}",
                    t.id,
                    t.title,
                    t.status.as_str(),
                    t.error.clone().unwrap_or_default(),
                    &t.result[tail_start..]
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let asks = waiting
            .iter()
            .map(|(ix, pending, key)| {
                self.announced.push(key.clone());
                let t = &self.tasks[*ix];
                format!(
                    "{} — {} needs {} (requestId {}): {}\n{}{}",
                    t.id,
                    t.title,
                    if pending.kind == "approval" {
                        "an approval"
                    } else {
                        "an answer"
                    },
                    pending.request_id,
                    pending.label,
                    pending.detail.chars().take(2_000).collect::<String>(),
                    if pending.questions.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "\n{}",
                            serde_json::to_string(&pending.questions).unwrap_or_default()
                        )
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let mut parts = Vec::new();
        if let Some(reason) = self.last_pause_reason.take() {
            parts.push(format!("Previous interruption: {reason}\nInterrupted workers were continued from their retained checkouts. Inspect their results normally before review."));
        }
        if !summary.is_empty() {
            parts.push(format!("Worker results are ready. Review the work, request corrections through the CLI when needed, and finish the original task.\n\n{summary}"));
        }
        if !asks.is_empty() {
            parts.push(format!("These agents are blocked waiting on you. Decide each one with respond or answer; they stay stopped until you do. Ask the user in this conversation only when the call is genuinely theirs to make.\n\n{asks}"));
        }
        Ok(Some(parts.join("\n\n")))
    }

    /// The lead's first supervising turn after the user starts the card.
    pub fn start_prompt(&self, cli: &str) -> String {
        let tasks: Vec<Value> = self
            .tasks
            .iter()
            .map(|t| json!({"taskId":t.id,"title":t.title,"harness":t.provider,"model":t.model,"files":t.files,"dependsOn":t.depends_on}))
            .collect();
        format!(
            "The user confirmed the orchestration card, including any edits. The app has already queued the exact assignments below; do not delegate duplicates. Supervise them through the control CLI, review their changes, request corrections when needed, and finish the original request.\n\nOriginal request:\n{}\n\nApproved assignments:\n{}\n\n{}",
            self.request,
            serde_json::to_string(&tasks).unwrap_or_default(),
            lead_instructions(cli)
        )
    }

    /// The lead's turn after the user resumes a paused run.
    pub fn resume_prompt(&self, cli: &str) -> String {
        format!(
            "The user resumed the orchestration run. Interrupted workers are continuing from their retained checkouts. Check the run with list, keep supervising and finish the original request.\n\nOriginal request:\n{}\n\n{}",
            self.request,
            lead_instructions(cli)
        )
    }

    /// What the sidebar lists of this run.
    pub fn summary(&self) -> Summary {
        Summary {
            live: self.live(),
            tasks: self
                .tasks
                .iter()
                .filter(|t| t.status != Status::Cancelled || t.session.is_some())
                .map(|t| SummaryTask {
                    session: t.session,
                    title: t.title.clone(),
                    provider: t.provider.clone(),
                    model: t.model.clone(),
                    status: match t.status {
                        Status::Queued => "queued",
                        Status::Running => "running",
                        Status::Completed if t.accepted => "completed",
                        Status::Completed => "review",
                        Status::Failed => "failed",
                        Status::Blocked | Status::Interrupted => "blocked",
                        Status::Cancelled => "cancelled",
                    }
                    .into(),
                })
                .collect(),
        }
    }

    /// One line per task, for the lead's card.
    pub fn progress(&self) -> String {
        let mut lines: Vec<String> = self
            .tasks
            .iter()
            .map(|t| {
                format!(
                    "{}: {}{}",
                    t.title,
                    match t.status {
                        Status::Completed if t.accepted => "accepted",
                        Status::Completed => "awaiting review",
                        s => s.as_str(),
                    },
                    t.waiting_note(self)
                )
            })
            .collect();
        if self.status == RunStatus::Paused {
            lines.insert(
                0,
                format!(
                    "Paused: {}",
                    self.error.as_deref().unwrap_or("the run was interrupted")
                ),
            );
        }
        lines.join("\n")
    }
}
impl Task {
    fn waiting_note(&self, run: &Run) -> String {
        run.waiting_for(self)
            .map(|w| format!(" · {}", w.to_lowercase()))
            .unwrap_or_default()
    }
}

fn request_id(input: &Value) -> Result<String, String> {
    match input.get("requestId") {
        Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
        Some(Value::Number(n)) => Ok(n.to_string()),
        _ => Err("Invalid requestId: pass the requestId from needsInput".into()),
    }
}

/// Map the lead's chosen option IDs or labels onto the worker's question.
fn question_answers(
    value: Option<&Value>,
    questions: &[Question],
) -> Result<Vec<Vec<String>>, String> {
    let object: &Map<String, Value> = value
        .and_then(Value::as_object)
        .ok_or("answers must be an object of questionId -> [optionId], or pass {\"skip\":true}")?;
    for key in object.keys() {
        if !questions.iter().any(|q| &q.id == key) {
            return Err(format!(
                "Unknown question \"{key}\". Ask for: {}.",
                listed(&questions.iter().map(|q| q.id.clone()).collect::<Vec<_>>())
            ));
        }
    }
    let mut out = Vec::new();
    let mut any = false;
    for question in questions {
        let chosen = strings(
            &json!({"v": object.get(&question.id).cloned().unwrap_or(json!([]))}),
            "v",
            16,
        )
        .map_err(|_| format!("answers.{} must be an array of option IDs", question.id))?;
        let mut labels = Vec::new();
        for choice in &chosen {
            match question
                .options
                .iter()
                .find(|o| &o.id == choice || &o.label == choice)
            {
                Some(option) => labels.push(option.label.clone()),
                None if question.allow_custom => labels.push(choice.clone()),
                None => {
                    return Err(format!(
                        "Unknown option for \"{}\": {choice}. Choose from: {}.",
                        question.id,
                        listed(
                            &question
                                .options
                                .iter()
                                .map(|o| o.id.clone())
                                .collect::<Vec<_>>()
                        )
                    ))
                }
            }
        }
        if !question.multiple && labels.len() > 1 {
            return Err(format!("\"{}\" takes one answer", question.id));
        }
        any |= !labels.is_empty();
        out.push(labels);
    }
    if !any {
        return Err("Answer at least one question, or pass {\"skip\":true}".into());
    }
    Ok(out)
}

/// `monocode-gpui control ...`: the same authenticated channel as app access,
/// with each action under `control.`.
pub fn client(args: &[String]) -> Result<Value, String> {
    if args
        .first()
        .is_none_or(|s| matches!(s.as_str(), "help" | "--help" | "-h"))
    {
        return Ok(json!({"help": HELP}));
    }
    if args.iter().any(|a| a == "--help") {
        return Ok(json!({"help": HELP}));
    }
    let mut forwarded = args.to_vec();
    forwarded[0] = format!("control.{}", args[0]);
    crate::mono::bridge::client(&forwarded)
}

#[cfg(test)]
mod tests;
