//! `/goal`: one objective the agent keeps working toward over several turns.
//! When a turn ends cleanly and the objective is not met, MonoBOT sends the
//! next turn itself, as DeepSeek Harness's goal round driver does, so a long
//! task runs on without the user typing "continue". It is bounded: at most
//! [`DEFAULT_ROUNDS`] automatic turns, never after a turn that failed or that
//! the user stopped, and a message the user queued goes first.
//!
//! The agent says where it stands on the last line of its final message:
//! `GOAL: complete`, or `GOAL: blocked: <what it needs>`. Any other ending
//! means "not done yet". The commands match T3 Code's `/goal`.
use serde::{Deserialize, Serialize};

/// Automatic turns one goal may take before it stops for the user.
pub const DEFAULT_ROUNDS: u32 = 8;
const MARKER: &str = "GOAL:";
/// An objective is a request, not a document.
pub const MAX_OBJECTIVE: usize = 4000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Active,
    Paused,
    Blocked,
    Complete,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Goal {
    pub objective: String,
    pub status: Status,
    /// Automatic turns sent so far.
    pub rounds: u32,
    pub limit: u32,
    /// Why it is paused or blocked, in the user's view.
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Show,
    Clear,
    Pause,
    Resume,
    Set(String),
}

/// Reads a `/goal` message. `None` when the text is something else.
pub fn parse(text: &str) -> Option<Command> {
    let rest = crate::composer::consume_command(text.trim(), "goal")?;
    let argument = rest.trim();
    Some(match argument.to_lowercase().as_str() {
        "" | "show" => Command::Show,
        "clear" => Command::Clear,
        "pause" => Command::Pause,
        "resume" => Command::Resume,
        _ => Command::Set(argument.to_owned()),
    })
}

/// What one turn's answer says about the goal.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Continue,
    Complete,
    Blocked(String),
}

/// The marker on the answer's last non-empty line, if any.
pub fn outcome(answer: &str) -> Outcome {
    let Some(line) = answer.lines().rev().map(str::trim).find(|l| !l.is_empty()) else {
        return Outcome::Continue;
    };
    let line = line.trim_matches(['`', '*', '_']).trim();
    let Some(rest) = line
        .get(..MARKER.len())
        .filter(|head| head.eq_ignore_ascii_case(MARKER))
        .map(|_| line[MARKER.len()..].trim())
    else {
        return Outcome::Continue;
    };
    let lower = rest.to_lowercase();
    if lower.starts_with("complete") || lower.starts_with("done") {
        Outcome::Complete
    } else if lower.starts_with("blocked") {
        let reason = rest["blocked".len()..]
            .trim_start_matches([':', ' ', '-', '—'])
            .trim();
        Outcome::Blocked(if reason.is_empty() {
            "The agent needs you to go on.".into()
        } else {
            reason.to_owned()
        })
    } else {
        Outcome::Continue
    }
}

/// What to do once a turn has ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Next {
    /// Send the next goal turn when the conversation is idle.
    Continue,
    /// Nothing more to send: the goal finished, stopped or waits for the user.
    Wait,
}

impl Goal {
    pub fn new(objective: &str) -> Self {
        Self {
            objective: objective.trim().to_owned(),
            status: Status::Active,
            rounds: 0,
            limit: DEFAULT_ROUNDS,
            note: None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.status == Status::Active
    }

    /// Weighs a turn that ended while the goal was active. `clean` is false
    /// when the turn failed or the user stopped it.
    pub fn after_turn(&mut self, answer: &str, clean: bool) -> Next {
        if !self.is_active() {
            return Next::Wait;
        }
        match outcome(answer) {
            Outcome::Complete => {
                self.status = Status::Complete;
                self.note = None;
                return Next::Wait;
            }
            Outcome::Blocked(reason) => {
                self.status = Status::Blocked;
                self.note = Some(reason);
                return Next::Wait;
            }
            Outcome::Continue => {}
        }
        if !clean {
            self.status = Status::Paused;
            self.note = Some("The last turn stopped or failed.".into());
            Next::Wait
        } else if self.rounds >= self.limit {
            self.status = Status::Paused;
            self.note = Some(format!(
                "It used its {} automatic turns without finishing.",
                self.limit
            ));
            Next::Wait
        } else {
            Next::Continue
        }
    }

    /// Counts the automatic turn about to be sent.
    pub fn begin_round(&mut self) {
        self.rounds += 1;
    }

    /// Resumes a paused or blocked goal with a fresh allowance of turns.
    pub fn resume(&mut self) {
        self.status = Status::Active;
        self.rounds = 0;
        self.note = None;
    }

    /// What the transcript shows for an automatic turn.
    pub fn round_label(&self) -> String {
        format!("Continue the goal ({} of {})", self.rounds, self.limit)
    }

    /// One line about where the goal stands, for the notice and `/goal`.
    pub fn summary(&self) -> String {
        let state = match self.status {
            Status::Active if self.rounds == 0 => "Goal".to_owned(),
            Status::Active => format!("Goal, turn {} of {}", self.rounds, self.limit),
            Status::Paused => "Goal paused".into(),
            Status::Blocked => "Goal blocked".into(),
            Status::Complete => "Goal complete".into(),
        };
        match &self.note {
            Some(note) => format!("{state}: {}. {note}", self.objective),
            None => format!("{state}: {}", self.objective),
        }
    }
}

/// The rules every goal turn carries, after its own text.
fn rules(limit: u32) -> String {
    format!(
        "This is a goal. MonoBOT sends you up to {limit} more turns on its own until it is done, so work toward it end to end and check your work (build, tests) as you go. End your final message with one of these lines:\n- `{MARKER} complete` when the goal is fully done and checked.\n- `{MARKER} blocked: <what you need>` when you cannot go on without the user (a decision, access, missing information).\nWith neither line, the next turn continues where you stopped."
    )
}

/// What the agent reads for the turn that sets the goal: the objective as
/// the composer prepared it (mentions, skills), then the rules.
pub fn first_prompt(request: &str, goal: &Goal) -> String {
    format!("{request}\n\n---\n{}", rules(goal.limit))
}

/// What the agent reads for an automatic turn.
pub fn continuation_prompt(goal: &Goal) -> String {
    format!(
        "Continue working toward the goal: {}\n\nThis is automatic turn {} of {}. Look at what is already done before you repeat any work.\n\n---\n{}",
        goal.objective,
        goal.rounds,
        goal.limit,
        rules(goal.limit.saturating_sub(goal.rounds))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_read_like_t3_codes() {
        assert_eq!(parse("/goal"), Some(Command::Show));
        assert_eq!(parse("  /goal  "), Some(Command::Show));
        assert_eq!(parse("/goal clear"), Some(Command::Clear));
        assert_eq!(parse("/goal PAUSE"), Some(Command::Pause));
        assert_eq!(parse("/goal resume"), Some(Command::Resume));
        assert_eq!(
            parse("/goal make the tests pass on Windows"),
            Some(Command::Set("make the tests pass on Windows".into()))
        );
        assert_eq!(parse("/goals"), None);
        assert_eq!(parse("fix /goal parsing"), None);
    }

    #[test]
    fn the_last_line_says_whether_the_goal_is_done() {
        assert_eq!(outcome("All green.\n\nGOAL: complete"), Outcome::Complete);
        assert_eq!(outcome("ok\n`goal: done`\n\n"), Outcome::Complete);
        assert_eq!(
            outcome("I need the key.\nGOAL: blocked: a staging API key"),
            Outcome::Blocked("a staging API key".into())
        );
        assert_eq!(
            outcome("GOAL: blocked"),
            Outcome::Blocked("The agent needs you to go on.".into())
        );
        assert_eq!(outcome("Fixed two of five tests."), Outcome::Continue);
        assert_eq!(
            outcome("GOAL: complete\nStill working on the rest."),
            Outcome::Continue,
            "only the last line counts"
        );
        assert_eq!(outcome(""), Outcome::Continue);
    }

    #[test]
    fn a_goal_continues_until_done_and_never_after_a_stop_or_past_its_limit() {
        let mut goal = Goal::new("  port the parser  ");
        assert_eq!(goal.objective, "port the parser");
        assert_eq!(goal.after_turn("Started.", true), Next::Continue);
        goal.begin_round();
        assert_eq!(goal.round_label(), "Continue the goal (1 of 8)");
        assert_eq!(goal.after_turn("Done.\nGOAL: complete", true), Next::Wait);
        assert_eq!(goal.status, Status::Complete);
        assert_eq!(goal.after_turn("more", true), Next::Wait, "finished stays");

        let mut goal = Goal::new("x");
        assert_eq!(goal.after_turn("half", false), Next::Wait);
        assert_eq!(goal.status, Status::Paused);
        assert!(goal.summary().contains("stopped or failed"));
        goal.resume();
        assert!(goal.is_active());
        assert_eq!(goal.note, None);

        let mut goal = Goal::new("x");
        for _ in 0..DEFAULT_ROUNDS {
            assert_eq!(goal.after_turn("not yet", true), Next::Continue);
            goal.begin_round();
        }
        assert_eq!(goal.after_turn("not yet", true), Next::Wait);
        assert_eq!(goal.status, Status::Paused);
        assert!(goal.note.as_deref().unwrap().contains("8 automatic turns"));

        let mut goal = Goal::new("x");
        assert_eq!(
            goal.after_turn("GOAL: blocked: the DB password", false),
            Next::Wait
        );
        assert_eq!(goal.status, Status::Blocked, "a blocker outranks the stop");
        assert_eq!(goal.summary(), "Goal blocked: x. the DB password");
    }

    #[test]
    fn every_goal_turn_carries_the_objective_and_the_rules() {
        let mut goal = Goal::new("ship the importer");
        let first = first_prompt(&goal.objective, &goal);
        assert!(first.starts_with("ship the importer\n"));
        assert!(first.contains("`GOAL: complete`"));
        assert!(first.contains("up to 8 more turns"));
        goal.begin_round();
        goal.begin_round();
        let next = continuation_prompt(&goal);
        assert!(next.starts_with("Continue working toward the goal: ship the importer"));
        assert!(next.contains("automatic turn 2 of 8"));
        assert!(next.contains("up to 6 more turns"));
        assert!(next.contains("`GOAL: blocked: <what you need>`"));
    }
}
