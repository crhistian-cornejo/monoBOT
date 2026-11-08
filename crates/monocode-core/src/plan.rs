//! A local session's reviewable plan: the `plan` transcript row, how a plan
//! turn's answer becomes one, and the prompt that builds it.
//!
//! Ports `src/features/sessions/model/plan.ts` and `promoteLastAssistantToPlan`
//! from `src/integrations/harness/core/apply.ts`. A provider with a structured
//! plan (Claude's `ExitPlanMode`, Codex's `plan` item) writes the row itself.
//! Any other plan turn ends with its plan as the final answer, which becomes
//! the row once the turn has ended, and only if it reads like a plan.
use crate::transcript::Row;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// The transcript role that holds a [`PlanRow`] as JSON.
pub const ROLE: &str = "plan";
/// What the transcript shows for the turn that builds a plan.
pub const BUILD_MESSAGE: &str = "Build approved plan";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanRow {
    pub text: String,
    /// The turn's final answer is this plan, so the card stands in for it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub answer: bool,
}

/// The prompt a provider gets for "Build", with the plan as the source of truth.
pub fn build_prompt(plan: &str) -> String {
    [
        "The user reviewed and explicitly approved the following implementation plan. Implement it now, using this exact edited version as the source of truth.",
        "",
        "<approved_plan>",
        plan.trim(),
        "</approved_plan>",
    ]
    .join("\n")
}

fn regexes(
    source: &'static [&'static str],
    slot: &'static OnceLock<Vec<Regex>>,
) -> &'static [Regex] {
    slot.get_or_init(|| source.iter().map(|re| Regex::new(re).unwrap()).collect())
}

/// Provider messages that arrive as ordinary text although no work happened.
pub fn is_provider_failure_text(text: &str) -> bool {
    static RES: OnceLock<Vec<Regex>> = OnceLock::new();
    let value = text.trim();
    !value.is_empty()
        && regexes(
            &[
                r"(?im)(?:^|\n)\s*upgrade your plan to continue[.!]?\s*(?:$|\n)",
                r"(?im)(?:^|\n)\s*(?:you(?:'ve| have) )?reached (?:your )?(?:usage|request|spend) limit",
                r"(?im)(?:^|\n)\s*(?:usage|rate|request) limit (?:reached|exceeded)",
                r"(?im)(?:^|\n)\s*(?:authentication required|please (?:sign|log) in to continue)",
            ],
            &RES,
        )
        .iter()
        .any(|re| re.is_match(value))
}

/// A final answer that looks like an authored Markdown plan: a heading, or
/// at least two list steps.
pub fn is_reviewable(text: &str) -> bool {
    static HEADING: OnceLock<Regex> = OnceLock::new();
    static STEP: OnceLock<Regex> = OnceLock::new();
    let value = text.trim();
    if value.is_empty() || is_provider_failure_text(value) {
        return false;
    }
    HEADING
        .get_or_init(|| Regex::new(r"(?m)^\s{0,3}#{1,6}\s+\S").unwrap())
        .is_match(value)
        || STEP
            .get_or_init(|| Regex::new(r"(?m)^\s*(?:[-*+] |\d+[.)] )\S").unwrap())
            .find_iter(value)
            .count()
            >= 2
}

fn unwrap_markdown(value: &str) -> String {
    static IMAGE: OnceLock<Regex> = OnceLock::new();
    static LINK: OnceLock<Regex> = OnceLock::new();
    static MARKS: OnceLock<Regex> = OnceLock::new();
    let value = IMAGE
        .get_or_init(|| Regex::new(r"!\[[^\]]*]\([^)]*\)").unwrap())
        .replace_all(value, "");
    let value = LINK
        .get_or_init(|| Regex::new(r"\[([^\]]+)]\([^)]*\)").unwrap())
        .replace_all(&value, "$1");
    MARKS
        .get_or_init(|| Regex::new(r"[*_`]+").unwrap())
        .replace_all(&value, "")
        .trim()
        .to_owned()
}

fn heading(line: &str) -> Option<&str> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let body = trimmed.trim_start_matches('#');
    let hashes = trimmed.len() - body.len();
    ((1..=6).contains(&hashes) && body.starts_with([' ', '\t']) && !body.trim().is_empty())
        .then(|| body.trim())
}

/// The first Markdown heading, or the first prose line.
pub fn title(text: &str) -> String {
    if let Some(found) = text.lines().find_map(heading) {
        return unwrap_markdown(found);
    }
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("```") || trimmed == "---" {
            continue;
        }
        let title: String = unwrap_markdown(trimmed).chars().take(80).collect();
        return if title.is_empty() {
            "Plan".into()
        } else {
            title
        };
    }
    "Plan".into()
}

/// The first prose that is not the title, up to about 140 characters.
pub fn summary(text: &str) -> String {
    let title = title(text);
    let mut parts: Vec<String> = Vec::new();
    let mut fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fence = !fence;
            continue;
        }
        if fence || heading(line).is_some() {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "---" {
            continue;
        }
        let bullet = trimmed
            .strip_prefix(['-', '*', '+'])
            .filter(|rest| rest.starts_with(' '))
            .map_or(trimmed, str::trim_start);
        let next = unwrap_markdown(bullet);
        if next.is_empty() || next == title {
            continue;
        }
        parts.push(next);
        if parts.join(" ").chars().count() >= 140 {
            break;
        }
    }
    let summary = parts.join(" ");
    if summary.chars().count() > 140 {
        format!("{}…", summary.chars().take(139).collect::<String>())
    } else {
        summary
    }
}

/// Whether a tool call is Claude's way of handing back its plan.
pub fn is_exit_plan_mode(name: &str) -> bool {
    name.eq_ignore_ascii_case("ExitPlanMode") || name.eq_ignore_ascii_case("exit_plan_mode")
}

/// The plan an `ExitPlanMode` call carries in its input.
pub fn exit_plan_mode_plan(input: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(input)
        .ok()?
        .get("plan")?
        .as_str()
        .map(str::trim)
        .filter(|plan| !plan.is_empty())
        .map(str::to_owned)
}

/// The messages of `rows` as (first row, joined text), rows of one message
/// being those that continue it (`first == false`).
fn messages(rows: &[Row]) -> impl Iterator<Item = (usize, String)> + '_ {
    let mut at = 0;
    std::iter::from_fn(move || {
        while at < rows.len() && !rows[at].first {
            at += 1;
        }
        if at >= rows.len() {
            return None;
        }
        let start = at;
        let mut text = rows[at].text.clone();
        at += 1;
        while at < rows.len() && !rows[at].first {
            text.push_str(&rows[at].text);
            at += 1;
        }
        Some((start, text))
    })
}

/// After a plan turn has ended: its final answer as a plan, unless the turn
/// already has one or the answer does not read like a plan.
pub fn promote(rows: &[Row]) -> Option<PlanRow> {
    let turn = rows.iter().rposition(Row::is_prompt).map_or(0, |at| at + 1);
    let turn_rows = &rows[turn..];
    if turn_rows.iter().any(|row| row.role == ROLE) {
        return None;
    }
    let (_, text) = messages(turn_rows)
        .filter(|(start, text)| turn_rows[*start].role == "assistant" && !text.trim().is_empty())
        .last()?;
    is_reviewable(&text).then(|| PlanRow {
        text: text.trim().to_owned(),
        answer: true,
    })
}

/// The plan whose row starts at `at`, its text joined across rows.
pub fn row_at(rows: &[Row], at: usize) -> Option<PlanRow> {
    let first = rows.get(at).filter(|row| row.first && row.role == ROLE)?;
    let mut text = first.text.clone();
    for row in rows[at + 1..].iter().take_while(|row| !row.first) {
        text.push_str(&row.text);
    }
    serde_json::from_str(&text).ok()
}

/// Where the plan at `at` stands, from what came after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ready,
    /// The turn that builds it is running.
    Building,
    Built,
}

/// A plan is built once a later turn was sent to build it.
pub fn status(rows: &[Row], at: usize, streaming: bool) -> Status {
    let Some(build) = rows
        .iter()
        .enumerate()
        .skip(at + 1)
        .find(|(_, row)| row.first && row.is_prompt() && row.text.trim() == BUILD_MESSAGE)
        .map(|(ix, _)| ix)
    else {
        return Status::Ready;
    };
    let last_prompt = rows.iter().rposition(|row| row.first && row.is_prompt());
    if streaming && last_prompt == Some(build) {
        Status::Building
    } else {
        Status::Built
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(role: &str, text: &str) -> Row {
        Row {
            role: role.into(),
            text: text.into(),
            first: true,
            sent_at: None,
        }
    }

    #[test]
    fn reviewable_plans_need_a_heading_or_two_steps() {
        assert!(is_reviewable("# Ship it\n\nDo the thing"));
        assert!(is_reviewable("1. Read the code\n2. Change it"));
        assert!(is_reviewable("- one\n- two"));
        assert!(!is_reviewable("I looked around and it is fine."));
        assert!(!is_reviewable("- only one step"));
        assert!(!is_reviewable("# Heading\nUpgrade your plan to continue."));
        assert!(!is_reviewable("You've reached your usage limit"));
    }

    #[test]
    fn titles_and_summaries_unwrap_markdown() {
        let plan = "# Add **dark** mode\n\nWire the `theme` toggle into [settings](x.md).\n\n## Steps\n\n- Read the palette\n```rust\nfn x() {}\n```";
        assert_eq!(title(plan), "Add dark mode");
        assert_eq!(
            summary(plan),
            "Wire the theme toggle into settings. Read the palette"
        );
        assert_eq!(title("\n---\n```rust\nFirst line here"), "First line here");
        assert_eq!(title(""), "Plan");
        let long = format!("# T\n\n{}", "word ".repeat(60));
        assert!(summary(&long).ends_with('…'));
        assert_eq!(summary(&long).chars().count(), 140);
    }

    #[test]
    fn the_build_prompt_wraps_the_plan_exactly() {
        let prompt = build_prompt("  # Plan\n- step  ");
        assert!(prompt.starts_with("The user reviewed and explicitly approved"));
        assert!(prompt.ends_with("<approved_plan>\n# Plan\n- step\n</approved_plan>"));
    }

    #[test]
    fn exit_plan_mode_carries_its_plan() {
        assert!(is_exit_plan_mode("ExitPlanMode"));
        assert!(!is_exit_plan_mode("Edit"));
        assert_eq!(
            exit_plan_mode_plan(r##"{"plan":" # P\n- a "}"##).as_deref(),
            Some("# P\n- a")
        );
        assert_eq!(exit_plan_mode_plan(r#"{"plan":"  "}"#), None);
        assert_eq!(exit_plan_mode_plan("not json"), None);
    }

    #[test]
    fn only_the_last_answer_of_a_plan_turn_is_promoted() {
        let mut long = row("assistant", "# Plan\n\n- read");
        let tail = Row {
            first: false,
            ..row("assistant", "\n- write")
        };
        let rows = vec![
            row("user", "make a plan"),
            row("assistant", "Looking around first."),
            row("tool", "{}"),
            {
                long.first = true;
                long
            },
            tail,
        ];
        assert_eq!(
            promote(&rows),
            Some(PlanRow {
                text: "# Plan\n\n- read\n- write".into(),
                answer: true
            })
        );
        // Commentary is never promoted.
        let rows = vec![row("user", "x"), row("assistant", "Done, nothing to plan.")];
        assert_eq!(promote(&rows), None);
        // A turn with its own plan row keeps it.
        let rows = vec![
            row("user", "x"),
            row(ROLE, r##"{"text":"# P"}"##),
            row("assistant", "# Plan\n- a\n- b"),
        ];
        assert_eq!(promote(&rows), None);
    }

    #[test]
    fn a_plan_is_built_by_a_later_build_turn() {
        let rows = vec![
            row("user", "plan"),
            row(ROLE, "{}"),
            row("user", BUILD_MESSAGE),
            row("assistant", "on it"),
        ];
        assert_eq!(status(&rows, 1, true), Status::Building);
        assert_eq!(status(&rows, 1, false), Status::Built);
        assert_eq!(status(&rows[..2], 1, false), Status::Ready);
        let later = [rows.clone(), vec![row("user", "thanks")]].concat();
        assert_eq!(status(&later, 1, true), Status::Built);
    }

    #[test]
    fn a_long_plan_row_is_read_back_whole() {
        let mut transcript = crate::transcript::Transcript::default();
        transcript.begin("user");
        transcript.append("plan");
        let text = format!("# Plan\n{}", "- step\n".repeat(600));
        let at = transcript.begin(ROLE);
        transcript.append(
            &serde_json::to_string(&PlanRow {
                text: text.clone(),
                answer: false,
            })
            .unwrap(),
        );
        assert!(transcript.rows.len() > 3);
        assert_eq!(row_at(&transcript.rows, at).unwrap().text, text);
        assert_eq!(row_at(&transcript.rows, 0), None);
    }

    #[test]
    fn plan_rows_round_trip_without_noise() {
        let row = PlanRow {
            text: "# P".into(),
            answer: false,
        };
        assert_eq!(serde_json::to_string(&row).unwrap(), r##"{"text":"# P"}"##);
        assert!(
            serde_json::from_str::<PlanRow>(r##"{"text":"# P","answer":true}"##)
                .unwrap()
                .answer
        );
    }
}
