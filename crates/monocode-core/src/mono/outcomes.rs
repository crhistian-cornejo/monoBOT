//! What a habit run leaves behind: its outcome and report status, and the
//! rule that stops a habit that keeps failing. Ports `monoHabits.ts`
//! (`reportStatus`, `failedInARow`, `afterRun`) and the failure notices of
//! `useMonoHabits.ts`.
use super::habits::{Habit, Run, REPORT_KEPT_CHARS, RUNS_KEPT};
use serde::{Deserialize, Serialize};

/// Failed runs in a row after which a habit pauses itself.
pub const FAILURES_TO_PAUSE: usize = 5;
/// What a failed run records when the provider gave no reason.
pub const DEFAULT_ERROR: &str = "The run did not finish.";
/// Tells a run how to report its status; goes in the habit run prompt.
pub const STATUS_INSTRUCTION: &str = "Start it with [needs_attention] when the user should act soon, [blocked] when you could not do the work (say what is missing), or [done] when the habit's goal is reached; otherwise start it plainly.";

/// Where a posted report stands. A run says so with a leading tag and plain
/// reports are progress. Attention and blocked reach the user at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
    Progress,
    Done,
    NeedsAttention,
    Blocked,
}

impl ReportStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Progress => "progress",
            Self::Done => "done",
            Self::NeedsAttention => "needs_attention",
            Self::Blocked => "blocked",
        }
    }
    fn from_tag(tag: &str) -> Option<Self> {
        [
            Self::Progress,
            Self::Done,
            Self::NeedsAttention,
            Self::Blocked,
        ]
        .into_iter()
        .find(|s| s.as_str().eq_ignore_ascii_case(tag))
    }
}

/// A report's leading status tag, taken off its text (`reportStatus`).
/// Without a tag the report is progress.
pub fn report_status(report: &str) -> (ReportStatus, String) {
    let rest = report.trim_start();
    if let Some(body) = rest.strip_prefix('[') {
        if let Some((tag, after)) = body.split_once(']') {
            if let Some(status) = ReportStatus::from_tag(tag) {
                return (status, after.trim().to_string());
            }
        }
    }
    (ReportStatus::Progress, report.trim().to_string())
}

/// A run's final reply as the report to post: `None` when it has nothing to
/// say (quiet marker, or only a status tag), otherwise the status and text.
pub fn tagged_report(reply: &str) -> Option<(ReportStatus, String)> {
    let (status, text) = report_status(&super::habits::report(reply)?);
    (!text.is_empty()).then_some((status, text))
}

/// How a run ended, as `habits.json` records it in `lastOutcome`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It told the user something.
    Posted,
    /// It found nothing worth saying.
    Quiet,
    Failed,
    /// It was interrupted (e.g. computer went to sleep or app closed).
    Interrupted,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Posted => "posted",
            Self::Quiet => "quiet",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
}

/// What a finished run produced.
#[derive(Clone, Debug)]
pub struct RunResult {
    pub outcome: Outcome,
    /// When the run started (`Run::at`).
    pub started_at: u64,
    pub report: Option<String>,
    pub status: Option<ReportStatus>,
    pub error: Option<String>,
}

impl RunResult {
    /// A run that finished: it posted when it has a report, else it was quiet.
    pub fn completed(started_at: u64, report: Option<(ReportStatus, String)>) -> Self {
        let (status, report) = match report {
            Some((s, r)) => (Some(s), Some(r)),
            None => (None, None),
        };
        Self {
            outcome: if report.is_some() {
                Outcome::Posted
            } else {
                Outcome::Quiet
            },
            started_at,
            report,
            status,
            error: None,
        }
    }
    pub fn failed(started_at: u64, error: Option<String>) -> Self {
        Self {
            outcome: Outcome::Failed,
            started_at,
            report: None,
            status: None,
            error: Some(error.unwrap_or_else(|| DEFAULT_ERROR.into())),
        }
    }
    pub fn interrupted(started_at: u64, error: Option<String>) -> Self {
        Self {
            outcome: Outcome::Interrupted,
            started_at,
            report: None,
            status: None,
            error: Some(error.unwrap_or_else(|| "Interrupted by system sleep or shutdown".into())),
        }
    }
}

/// What recording a run asks the caller to do next.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recorded {
    /// A line to post to the Mono's chat with status `blocked`: the first
    /// failure in a row, and the pause after `FAILURES_TO_PAUSE`.
    pub notice: Option<String>,
    /// The habit was a wake that has now run, and was removed from the list.
    pub removed: bool,
}

/// How many of the habit's latest runs failed in a row (`failedInARow`).
pub fn failed_in_a_row(habit: &Habit) -> usize {
    habit
        .runs
        .iter()
        .take_while(|run| run.outcome == "failed")
        .count()
}

/// Records a finished run on the habit (`afterRun`) and works out what to say.
/// A habit that has failed `FAILURES_TO_PAUSE` times in a row is switched off.
pub fn record_outcome(habit: &mut Habit, result: RunResult, now: u64) -> Recorded {
    let failed = result.outcome == Outcome::Failed;
    let error = result.error.clone();
    let run = Run {
        at: result.started_at,
        duration_ms: now.saturating_sub(result.started_at),
        outcome: result.outcome.as_str().into(),
        report: result
            .report
            .map(|r| r.chars().take(REPORT_KEPT_CHARS).collect()),
        status: result.status,
        error: error.clone(),
    };
    habit.runs.insert(0, run);
    habit.runs.truncate(RUNS_KEPT);
    habit.last_run_at = Some(now);
    habit.last_outcome = Some(result.outcome.as_str().into());
    habit.last_error = error.clone();
    habit.running_since = None;
    let streak = failed_in_a_row(habit);
    if streak >= FAILURES_TO_PAUSE {
        habit.enabled = false;
    }
    let notice = match (failed, streak) {
        (true, 1) => Some(format!(
            "Couldn't run \"{}\": {}",
            habit.name,
            error.unwrap_or_default()
        )),
        (true, FAILURES_TO_PAUSE) => Some(format!(
            "Paused \"{}\" after {FAILURES_TO_PAUSE} failed runs in a row. Last error: {}",
            habit.name,
            error.unwrap_or_default()
        )),
        _ => None,
    };
    Recorded {
        notice,
        removed: false,
    }
}

/// Records a finished run on the habit `id` in the list, and removes it if it
/// was a wake: a wake runs once and its report is already in the chat. Call it
/// inside `habits::update`. `None` when the habit is no longer in the list.
pub fn record_run(
    list: &mut Vec<Habit>,
    id: &str,
    result: RunResult,
    now: u64,
) -> Option<Recorded> {
    let at = list.iter().position(|h| h.id == id)?;
    let mut recorded = record_outcome(&mut list[at], result, now);
    if list[at].once {
        list.remove(at);
        recorded.removed = true;
    }
    Some(recorded)
}

/// What the last posted report said, for the next run's prompt, so it reports
/// only what changed. Empty when no run has reported yet.
pub fn previous_report_note(habit: &Habit) -> String {
    let Some(report) = habit.runs.iter().find_map(|run| run.report.as_ref()) else {
        return String::new();
    };
    let shown = if report.chars().count() > 600 {
        format!("{}…", report.chars().take(599).collect::<String>())
    } else {
        report.clone()
    };
    format!(
        "\nLast time it told the user: {shown}\nDo not repeat it; report only what changed since."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mono::habits::Schedule;

    fn habit() -> Habit {
        Habit::new(
            "Bank review".into(),
            "Review the bank's emails.".into(),
            Schedule::default(),
            1_000,
        )
        .unwrap()
    }

    #[test]
    fn reads_a_status_tag_and_leaves_plain_reports_as_progress() {
        assert_eq!(
            report_status("[needs_attention] Card charged twice."),
            (ReportStatus::NeedsAttention, "Card charged twice.".into())
        );
        assert_eq!(
            report_status("[BLOCKED] Gmail is not connected"),
            (ReportStatus::Blocked, "Gmail is not connected".into())
        );
        assert_eq!(
            report_status("  [done]\n\nAll paid."),
            (ReportStatus::Done, "All paid.".into())
        );
        assert_eq!(
            report_status("Spent S/ 45 today."),
            (ReportStatus::Progress, "Spent S/ 45 today.".into())
        );
        assert_eq!(
            report_status("[urgent] not a status"),
            (ReportStatus::Progress, "[urgent] not a status".into())
        );
    }

    #[test]
    fn a_report_that_is_only_a_tag_or_the_quiet_marker_is_not_posted() {
        assert!(tagged_report("[done]").is_none());
        assert!(tagged_report("NOTHING_TO_REPORT").is_none());
        assert_eq!(
            tagged_report("[blocked] Needs a login").unwrap(),
            (ReportStatus::Blocked, "Needs a login".into())
        );
    }

    #[test]
    fn pauses_a_habit_after_failing_too_many_times_in_a_row() {
        let mut h = habit();
        for n in 1..FAILURES_TO_PAUSE as u64 {
            record_outcome(&mut h, RunResult::failed(n, Some("down".into())), 10 + n);
            assert!(h.enabled);
        }
        let last = record_outcome(&mut h, RunResult::failed(99, Some("down".into())), 100);
        assert_eq!(failed_in_a_row(&h), FAILURES_TO_PAUSE);
        assert!(!h.enabled);
        assert_eq!(
            last.notice.as_deref(),
            Some("Paused \"Bank review\" after 5 failed runs in a row. Last error: down")
        );
    }

    #[test]
    fn says_so_only_for_the_first_failure_and_the_pause() {
        let mut h = habit();
        let notices: Vec<_> = (1..=6)
            .map(|n| record_outcome(&mut h, RunResult::failed(n, Some("x".into())), n).notice)
            .collect();
        assert_eq!(
            notices[0].as_deref(),
            Some("Couldn't run \"Bank review\": x")
        );
        assert!(notices[1..4].iter().all(Option::is_none));
        assert!(notices[4].is_some());
        assert!(notices[5].is_none());
    }

    #[test]
    fn starts_counting_failures_again_after_a_good_run() {
        let mut h = habit();
        record_outcome(&mut h, RunResult::failed(1, Some("x".into())), 1);
        record_outcome(&mut h, RunResult::completed(2, None), 2);
        record_outcome(&mut h, RunResult::failed(3, Some("x".into())), 3);
        assert_eq!(failed_in_a_row(&h), 1);
        assert_eq!(h.last_outcome.as_deref(), Some("failed"));
    }

    #[test]
    fn interrupted_runs_do_not_count_towards_failure_streak_and_never_pause() {
        let mut h = habit();
        // 4 real failures in a row
        for n in 1..4 {
            record_outcome(&mut h, RunResult::failed(n, Some("network error".into())), 10 + n);
        }
        assert_eq!(failed_in_a_row(&h), 3);
        assert!(h.enabled);

        // Sleep interrupts the run
        let interrupted = record_outcome(
            &mut h,
            RunResult::interrupted(99, Some("Interrupted: computer went to sleep".into())),
            100,
        );
        // Interrupted does not produce a notice and does not count as failed
        assert!(interrupted.notice.is_none());
        assert_eq!(h.last_outcome.as_deref(), Some("interrupted"));
        assert_eq!(failed_in_a_row(&h), 0);
        assert!(h.enabled);

        // Even 10 interrupted runs in a row never pause the habit
        for n in 101..112 {
            let res = record_outcome(
                &mut h,
                RunResult::interrupted(n, Some("Interrupted: computer went to sleep".into())),
                n,
            );
            assert!(res.notice.is_none());
            assert!(h.enabled);
        }
        assert!(h.enabled);
    }

    #[test]
    fn a_failure_without_a_reason_records_the_default_one() {
        let mut h = habit();
        let r = record_outcome(&mut h, RunResult::failed(1, None), 2);
        assert_eq!(h.last_error.as_deref(), Some(DEFAULT_ERROR));
        assert_eq!(
            r.notice.as_deref(),
            Some("Couldn't run \"Bank review\": The run did not finish.")
        );
        // A later good run clears the error.
        record_outcome(&mut h, RunResult::completed(3, None), 4);
        assert_eq!(h.last_error, None);
    }

    #[test]
    fn keeps_a_runs_status_and_gives_the_next_run_the_last_report() {
        let mut h = habit();
        let report = "Spent S/ 320 on delivery, 80% of the cap.".to_string();
        record_outcome(
            &mut h,
            RunResult::completed(5, Some((ReportStatus::NeedsAttention, report))),
            9,
        );
        assert_eq!(h.last_outcome.as_deref(), Some("posted"));
        assert_eq!(h.last_run_at, Some(9));
        assert_eq!(h.runs[0].status, Some(ReportStatus::NeedsAttention));
        assert_eq!(h.runs[0].duration_ms, 4);
        let text = serde_json::to_string(&vec![h.clone()]).unwrap();
        assert!(text.contains(r#""status":"needs_attention""#));
        let back: Vec<Habit> = serde_json::from_str(&text).unwrap();
        assert_eq!(back[0].runs[0].status, Some(ReportStatus::NeedsAttention));
        let note = previous_report_note(&h);
        assert!(note.contains("Last time it told the user: Spent S/ 320"));
        assert!(note.contains("report only what changed"));
        assert_eq!(previous_report_note(&habit()), "");
    }

    #[test]
    fn long_reports_are_kept_up_to_four_thousand_characters_and_shown_in_six_hundred() {
        let mut h = habit();
        record_outcome(
            &mut h,
            RunResult::completed(1, Some((ReportStatus::Progress, "é".repeat(5000)))),
            2,
        );
        assert_eq!(h.runs[0].report.as_ref().unwrap().chars().count(), 4000);
        let note = previous_report_note(&h);
        assert!(note.contains(&format!("{}…\n", "é".repeat(599))));
    }

    #[test]
    fn keeps_the_latest_twenty_runs() {
        let mut h = habit();
        for n in 0..25 {
            record_outcome(&mut h, RunResult::completed(n, None), n);
        }
        assert_eq!(h.runs.len(), 20);
        assert_eq!(h.runs[0].at, 24);
    }

    #[test]
    fn an_unknown_run_status_in_habits_json_does_not_drop_the_run() {
        let run: Run =
            serde_json::from_str(r#"{"at":1,"outcome":"posted","report":"x","status":"urgent"}"#)
                .unwrap();
        assert_eq!(run.status, None);
        let tidy = serde_json::to_string(&run).unwrap();
        assert!(!tidy.contains("status"));
    }
}
