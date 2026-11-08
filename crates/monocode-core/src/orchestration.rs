//! Reviewable orchestration plans. The user edits and starts the lead's plan;
//! the lead then supervises its workers through `control`. Each worker gets
//! its own worktree (`isolation`), and overlapping scopes are serialized even
//! when they have no explicit dependency.
pub mod control;
pub mod isolation;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Component, Path},
};

/// orchestrationSummary.ts: what the sidebar shows of a lead's run, kept after
/// it ends so the lead still lists its agents.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Summary {
    pub tasks: Vec<SummaryTask>,
    /// The run is still dispatching or waiting on its workers.
    pub live: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SummaryTask {
    pub session: Option<i64>,
    pub title: String,
    pub provider: String,
    pub model: Option<String>,
    /// queued, running, review (completed, not yet accepted), completed,
    /// failed, blocked or cancelled.
    pub status: String,
}
impl SummaryTask {
    /// orchestrationTaskLabel, given whether the worker needs input.
    pub fn label(&self, summary: &Summary, needs_input: bool) -> &'static str {
        if needs_input {
            return "Needs input";
        }
        if !summary.live && matches!(self.status.as_str(), "running" | "queued") {
            return "Saved";
        }
        match self.status.as_str() {
            "queued" => "Queued",
            "running" => "Working",
            "review" => "Review",
            "completed" => "Done",
            "failed" => "Failed",
            "blocked" => "Needs review",
            "cancelled" => "Cancelled",
            _ => "Queued",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub prompt: String,
    pub files: Vec<String>,
    #[serde(default, alias = "dependsOn")]
    pub depends_on: Vec<String>,
    /// The harness, model and effort chosen on the card; the lead's otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "harness")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub title: String,
    pub summary: String,
    pub tasks: Vec<Task>,
}
pub const INSTRUCTIONS: &str = r#"Prepare an orchestration plan for the user's request. Inspect the project with read-only tools. Do not edit or launch workers yet. The user will review and start the plan in MonoBOT.
Return a short explanation followed by exactly one JSON block between <monocode_orchestration> and </monocode_orchestration>.
Schema: {"title":"short title","summary":"approach","tasks":[{"id":"task-1","title":"assignment title","prompt":"complete self-contained instructions, validation and expected report","files":["src/path"],"depends_on":[]}]}
Use 1 to 12 assignments, unique ASCII IDs, exact project-relative file or directory scopes ('.' for the whole project), and dependency IDs. Independent assignments may run concurrently (up to four), each in its own worktree; overlapping scopes run serially. Workers use this session's provider and model unless the user changes them on the card. After the user starts the plan you supervise the workers and review their changes. Include verification in the assignments. Never claim that workers have already started."#;
pub fn parse(text: &str) -> Result<Plan, String> {
    let body = text
        .split_once("<monocode_orchestration>")
        .and_then(|(_, s)| s.split_once("</monocode_orchestration>").map(|(s, _)| s))
        .ok_or("The response did not include a task plan. Retry Orchestrator to prepare one.")?;
    if body.len() > 256 * 1024 {
        return Err("Task plan is too large.".into());
    }
    let plan: Plan =
        serde_json::from_str(body.trim()).map_err(|e| format!("Invalid task plan: {e}"))?;
    validate(&plan)?;
    Ok(plan)
}
pub fn validate(plan: &Plan) -> Result<(), String> {
    if plan.title.trim().is_empty()
        || plan.title.len() > 160
        || plan.summary.len() > 4_000
        || plan.tasks.is_empty()
        || plan.tasks.len() > 12
    {
        return Err("Provide a title and 1–12 assignments.".into());
    }
    let mut ids = HashSet::new();
    for task in &plan.tasks {
        if task.id.is_empty()
            || task.id.len() > 64
            || !task
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || !ids.insert(task.id.as_str())
        {
            return Err(
                "Task IDs must be unique and contain only letters, numbers, '-' and '_'.".into(),
            );
        }
        if task.title.trim().is_empty()
            || task.title.len() > 160
            || task.prompt.trim().is_empty()
            || task.prompt.len() > 30_000
            || task.files.is_empty()
            || task.files.len() > 64
            || task.depends_on.len() > 12
        {
            return Err(format!(
                "Assignment {} needs a title, instructions and file scopes.",
                task.id
            ));
        }
        for file in &task.files {
            if file.trim().is_empty()
                || file.len() > 512
                || file.contains('\\')
                || file.contains(':')
                || Path::new(file).is_absolute()
                || Path::new(file).components().any(|c| {
                    matches!(
                        c,
                        Component::ParentDir | Component::RootDir | Component::Prefix(_)
                    )
                })
            {
                return Err(format!("Invalid file scope: {file}"));
            }
        }
    }
    let mut completed = HashSet::new();
    loop {
        let before = completed.len();
        for task in &plan.tasks {
            if task
                .depends_on
                .iter()
                .all(|id| completed.contains(id.as_str()))
            {
                completed.insert(task.id.as_str());
            }
        }
        if completed.len() == plan.tasks.len() {
            return Ok(());
        }
        if completed.len() == before {
            return Err("Task dependencies contain a cycle or unknown task ID.".into());
        }
    }
}
pub fn overlaps(a: &[String], b: &[String]) -> bool {
    a.iter().any(|a| {
        b.iter().any(|b| {
            a == "." || b == "." || Path::new(a).starts_with(b) || Path::new(b).starts_with(a)
        })
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn task(id: &str, deps: &[&str], file: &str) -> Task {
        Task {
            id: id.into(),
            title: id.into(),
            prompt: "Work".into(),
            files: vec![file.into()],
            depends_on: deps.iter().map(|s| (*s).into()).collect(),
            ..Default::default()
        }
    }
    #[test]
    fn rejects_cycles_unknown_dependencies_and_scope_escapes() {
        let mut plan = Plan {
            title: "Work".into(),
            summary: String::new(),
            tasks: vec![task("a", &["b"], "src"), task("b", &["a"], "tests")],
        };
        assert!(validate(&plan).is_err());
        plan.tasks[1].depends_on.clear();
        assert!(validate(&plan).is_ok());
        plan.tasks[1].files = vec!["../elsewhere".into()];
        assert!(validate(&plan).is_err());
        assert!(overlaps(&["src".into()], &["src/main.rs".into()]));
        assert!(!overlaps(&["src".into()], &["src2/main.rs".into()]));
    }
}
