//! A turn that goes round in circles is stopped: a model that writes the
//! same long passage over and over would spend the user's plan on it, and
//! one that makes the very same call again and again is not getting
//! anywhere. Rules, not a model: they look at what the turn already says and
//! cost nothing. Conservative on purpose: only long verbatim repeats that
//! fill most of the end of the answer, and only the same call with the same
//! input and the same result many times in a row, trip them. Ported from
//! IVY's `agents/guard.rs`.
//!
//! A provider that takes a message mid-turn is reminded first, as DeepSeek
//! Harness's repeat reminder does: the third, fifth and eighth identical call
//! ask the agent to read the last result and change approach or finish, and
//! only the twelfth stops the turn. A reminder costs one short message; a
//! stop ends the turn, and the conversation offers to carry on
//! (`provider_retry`'s `Looping`).
//!
//! A call is judged once it has finished, never while it runs: a provider
//! announces a call before its input has streamed in, so judging it then
//! would take any run of calls to one tool for the same call. And it is the
//! same call only when its result is the same too.
//!
//! Waiting is not going round in circles: a call to a waiting tool, a command
//! that sleeps, or any call that took a while lets time pass, so it starts
//! the count over. An agent that checks on a slow type-check, sleeps and
//! checks again is never stopped for it.
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::tool::{ToolCall, ToolState};

/// The same call, with the same input and result, this many times in a row
/// stops a turn whose provider cannot be reminded mid-turn.
pub const SAME_CALLS: usize = 8;
/// When the provider can be reminded: the counts that remind it, and the one
/// that stops the turn.
const REMIND_AT: [usize; 3] = [3, 5, 8];
pub const SAME_CALLS_REMINDED: usize = 12;
/// A call that ran at least this long was waiting on something.
const WAITED: Duration = Duration::from_secs(15);
/// Tools whose job is to wait on other work (lowercase).
const WAITING_TOOLS: [&str; 8] = [
    "taskoutput",
    "bashoutput",
    "agentoutputtool",
    "monitor",
    "schedulewakeup",
    "wait",
    "sleep",
    "await",
];
/// Inputs that wait: shell sleeps and blocking reads of a background task.
const WAITING_INPUTS: [&str; 5] = [
    "sleep ",
    "start-sleep",
    "timeout /t",
    "\"block\": true",
    "\"block\":true",
];
/// Every reminder is sent as a steer with this id prefix, so the transcript
/// can tell it from a message the user wrote.
const REMINDER_ID: &str = "loop-guard-reminder-";
/// The answer is looked at once it is this long, and again every `EVERY`
/// more bytes.
const LOOKED_FROM: usize = 3000;
const EVERY: usize = 1000;
/// How much of the end of the answer is looked at, and how long a passage
/// must be to count as one.
const TAIL: usize = 4000;
const PASSAGE: usize = 60;
/// Why the turn was stopped.
pub const REPEATING: &str =
    "Stopped: the model kept writing the same text. Ask again, perhaps in other words.";
pub const LOOPING: &str = "Stopped: the agent made the same call and got the same result again and again. Continue to have it try another way.";

/// What to do about a call that repeats the one before it.
#[derive(Debug, PartialEq, Eq)]
pub enum Repeat {
    /// Send the agent this reminder and let the turn go on.
    Remind(String),
    /// Stop the turn, with this reason.
    Stop(&'static str),
}

/// What one turn has done so far, as the guards look at it.
#[derive(Debug, Default)]
pub struct Guard {
    /// When each running call was first seen.
    started: HashMap<String, Instant>,
    judged: HashSet<String>,
    last_call: String,
    same: usize,
    looked_at: usize,
    reminds: bool,
}

impl Guard {
    /// A guard that reminds the agent before it stops the turn, for a
    /// provider that takes a message mid-turn.
    pub fn reminding() -> Self {
        Self {
            reminds: true,
            ..Self::default()
        }
    }

    /// After the answer grew: why to stop it, if it goes round in circles.
    pub fn text(&mut self, text: &str) -> Option<&'static str> {
        if text.len() < LOOKED_FROM || text.len() < self.looked_at + EVERY {
            return None;
        }
        self.looked_at = text.len();
        repeating(text).then_some(REPEATING)
    }

    /// After any update to a call: once it has finished, whether to remind
    /// the agent or stop the turn, when it is the same call with the same
    /// result as the one before. Each call is judged once.
    pub fn call(&mut self, call: &ToolCall) -> Option<Repeat> {
        self.call_at(call, Instant::now())
    }

    fn call_at(&mut self, call: &ToolCall, now: Instant) -> Option<Repeat> {
        match call.state {
            ToolState::Running => {
                self.started.entry(call.id.clone()).or_insert(now);
                return None;
            }
            ToolState::Cancelled => return None,
            ToolState::Completed | ToolState::Failed => {}
        }
        if !self.judged.insert(call.id.clone()) {
            return None;
        }
        let took_a_while = self
            .started
            .remove(&call.id)
            .is_some_and(|at| now.saturating_duration_since(at) >= WAITED);
        if took_a_while || waiting(call) {
            (self.last_call, self.same) = (String::new(), 0);
            return None;
        }
        let key = format!("{}\u{1f}{}\u{1f}{}", call.name, call.input, call.output);
        if key == self.last_call {
            self.same += 1;
        } else {
            (self.last_call, self.same) = (key, 1);
        }
        let stop = if self.reminds {
            SAME_CALLS_REMINDED
        } else {
            SAME_CALLS
        };
        if self.same >= stop {
            Some(Repeat::Stop(LOOPING))
        } else if self.reminds && REMIND_AT.contains(&self.same) {
            Some(Repeat::Remind(reminder(&call.name, self.same)))
        } else {
            None
        }
    }
}

/// Whether a call's job is to wait on other work.
fn waiting(call: &ToolCall) -> bool {
    let name = call.name.to_ascii_lowercase();
    let input = call.input.to_ascii_lowercase();
    WAITING_TOOLS.contains(&name.as_str()) || WAITING_INPUTS.iter().any(|w| input.contains(w))
}

/// What the agent reads when it repeats itself.
fn reminder(tool: &str, times: usize) -> String {
    format!(
        "monoBOT: you made the same {tool} call {times} times in a row and got the same result each time. Read that result before you call it again. If it did not help, change your approach; if you are waiting on something, wait longer between checks; if the work is done, finish and report."
    )
}

/// A fresh id for a reminder steer.
pub fn reminder_id(n: usize) -> String {
    format!("{REMINDER_ID}{n}")
}

/// Whether a steer is a reminder from this guard rather than the user's.
pub fn is_reminder(id: &str) -> bool {
    id.starts_with(REMINDER_ID)
}

fn floor(text: &str, at: usize) -> usize {
    (0..=at.min(text.len()))
        .rev()
        .find(|i| text.is_char_boundary(*i))
        .unwrap_or(0)
}

/// Whether the end of `text` is one passage written again and again: the
/// last `PASSAGE` bytes, words and not only marks, come back at the same
/// distance at least four times, and those repeats cover most of the end.
pub fn repeating(text: &str) -> bool {
    let tail = &text[floor(text, text.len().saturating_sub(TAIL))..];
    let probe = &tail[floor(tail, tail.len().saturating_sub(PASSAGE))..];
    // A passage is words: a rule of dashes or a table's border may repeat.
    if probe.chars().filter(|c| c.is_alphabetic()).count() < PASSAGE / 3 {
        return false;
    }
    let at: Vec<usize> = tail.match_indices(probe).map(|(i, _)| i).collect();
    if at.len() < 4 {
        return false;
    }
    let period = at[at.len() - 1] - at[at.len() - 2];
    let steady = at
        .windows(2)
        .rev()
        .take_while(|pair| pair[1] - pair[0] == period)
        .count()
        + 1;
    steady >= 4 && steady * period.max(probe.len()) * 10 >= tail.len() * 6
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_passage_written_again_and_again_is_found_and_a_long_answer_is_not() {
        let passage = "The sample budget splits between transport, food and lodging. ";
        assert!(repeating(&format!(
            "A short intro.\n{}",
            passage.repeat(60)
        )));
        let varied: String = (0..200)
            .map(|n| format!("Point {n}: a different idea about topic number {n}.\n"))
            .collect();
        assert!(!repeating(&varied), "a long list of different lines");
        assert!(!repeating(&"-".repeat(5000)), "a rule of dashes");
        assert!(
            !repeating(&"| --- | --- | --- |\n".repeat(300)),
            "a table's border"
        );
        assert!(repeating(
            &"Hola ñandú, repito esta frase con acentos y eñes de ejemplo. ".repeat(80)
        ));
    }

    #[test]
    fn the_text_guard_looks_only_now_and_then() {
        let mut guard = Guard::default();
        assert_eq!(guard.text("short"), None);
        let looping = "A sentence that comes back again and again in this example. ".repeat(70);
        assert_eq!(guard.text(&looping[..3100]), Some(REPEATING));
        assert_eq!(
            guard.text(&looping[..3200]),
            None,
            "not before it grew enough"
        );
    }

    /// A finished call, with a fresh id each time as a provider gives it.
    fn finished(id: usize, name: &str, input: &str, output: &str) -> ToolCall {
        let mut call = ToolCall::new(&id.to_string(), name);
        call.input = input.into();
        call.output = output.into();
        call.state = ToolState::Completed;
        call
    }

    #[test]
    fn the_same_call_many_times_in_a_row_stops_the_turn() {
        let mut guard = Guard::default();
        let mut id = 0;
        let mut call = |input: &str| {
            id += 1;
            finished(id, "Bash", input, "same answer")
        };
        for _ in 0..SAME_CALLS - 1 {
            assert_eq!(guard.call(&call("ls folder")), None);
        }
        assert_eq!(
            guard.call(&call("ls other")),
            None,
            "another call restarts the count"
        );
        for n in 0..SAME_CALLS {
            let said = guard.call(&call("cat file"));
            assert_eq!(said, (n + 1 == SAME_CALLS).then_some(Repeat::Stop(LOOPING)));
        }
    }

    #[test]
    fn calls_are_judged_once_finished_not_while_their_input_streams() {
        let mut guard = Guard::reminding();
        for n in 0..SAME_CALLS_REMINDED * 2 {
            // Announced before its input arrives, then updated, then done.
            let mut call = ToolCall::new(&n.to_string(), "Bash");
            assert_eq!(guard.call(&call), None, "announced");
            call.input = format!("step {n}");
            assert_eq!(guard.call(&call), None, "still running");
            call.state = ToolState::Completed;
            assert_eq!(guard.call(&call), None, "different commands: {n}");
            assert_eq!(guard.call(&call), None, "judged once");
        }
        let mut cancelled = finished(99, "Bash", "step", "");
        cancelled.state = ToolState::Cancelled;
        assert_eq!(guard.call(&cancelled), None);
    }

    #[test]
    fn waiting_on_work_whose_answer_changes_is_not_a_loop() {
        let mut guard = Guard::reminding();
        for n in 0..SAME_CALLS_REMINDED * 2 {
            let output = format!("still checking… {n} files");
            let said = guard.call(&finished(n, "Bash", "tail build.log", &output));
            assert_eq!(said, None);
        }
    }

    #[test]
    fn checking_on_a_slow_job_between_waits_is_not_a_loop() {
        let mut guard = Guard::reminding();
        let mut id = 0;
        let mut next = |name: &str, input: &str| {
            id += 1;
            finished(id, name, input, "running")
        };
        for _ in 0..SAME_CALLS_REMINDED * 2 {
            // The same check with the same answer, a wait in between.
            assert_eq!(guard.call(&next("Bash", "ls out")), None);
            assert_eq!(guard.call(&next("Bash", "sleep 30")), None);
            assert_eq!(guard.call(&next("Bash", "ls out")), None);
            let wait = r#"{"task_id": "b1", "block": true}"#;
            assert_eq!(guard.call(&next("Bash", wait)), None);
            assert_eq!(guard.call(&next("PowerShell", "Start-Sleep 20")), None);
            assert_eq!(guard.call(&next("TaskOutput", "b1")), None);
            assert_eq!(guard.call(&next("Monitor", "b1")), None);
        }
    }

    #[test]
    fn a_call_that_took_a_while_starts_the_count_over() {
        let mut guard = Guard::reminding();
        let start = Instant::now();
        for n in 0..SAME_CALLS_REMINDED * 2 {
            let call = finished(n, "Bash", "pnpm exec tsc --noEmit", "");
            let mut running = call.clone();
            running.state = ToolState::Running;
            let at = start + WAITED * (2 * n as u32);
            assert_eq!(guard.call_at(&running, at), None);
            assert_eq!(guard.call_at(&call, at + WAITED), None, "{n}");
        }
        // The same quick call keeps counting.
        let at = start + WAITED * 100;
        for n in 1..=3 {
            let call = finished(1000 + n, "Bash", "pnpm exec tsc --noEmit", "");
            let mut running = call.clone();
            running.state = ToolState::Running;
            guard.call_at(&running, at);
            let said = guard.call_at(&call, at + Duration::from_secs(1));
            assert_eq!(said.is_some(), n == 3, "{n}: {said:?}");
        }
    }

    #[test]
    fn a_provider_that_takes_a_steer_is_reminded_before_the_turn_stops() {
        let mut guard = Guard::reminding();
        for times in 1..=SAME_CALLS_REMINDED {
            let said = guard.call(&finished(times, "Bash", "cargo test", "1 failed"));
            match times {
                3 | 5 | 8 => assert!(
                    matches!(&said, Some(Repeat::Remind(text))
                        if text.contains(&format!("same Bash call {times} times"))),
                    "{times}: {said:?}"
                ),
                SAME_CALLS_REMINDED => assert_eq!(said, Some(Repeat::Stop(LOOPING))),
                _ => assert_eq!(said, None, "{times}"),
            }
        }
        assert!(is_reminder(&reminder_id(3)));
        assert!(!is_reminder("message-1"));
    }
}
