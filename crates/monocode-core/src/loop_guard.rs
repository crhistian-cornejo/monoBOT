//! A turn that goes round in circles is stopped: a model that writes the
//! same long passage over and over would spend the user's plan on it, and
//! one that makes the very same call again and again is not getting
//! anywhere. Rules, not a model: they look at what the turn already says and
//! cost nothing. Conservative on purpose: only long verbatim repeats that
//! fill most of the end of the answer, and only the same call with the same
//! input many times in a row, trip them. Ported from IVY's `agents/guard.rs`.
use crate::tool::ToolCall;

/// The same call, with the same input, this many times in a row.
pub const SAME_CALLS: usize = 6;
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
pub const LOOPING: &str = "Stopped: the agent kept repeating the same action without getting anywhere. Look at what it did and ask again with more detail.";

/// What one turn has done so far, as the guards look at it.
#[derive(Debug, Default)]
pub struct Guard {
    last_call: String,
    same: usize,
    looked_at: usize,
}

impl Guard {
    /// After the answer grew: why to stop it, if it goes round in circles.
    pub fn text(&mut self, text: &str) -> Option<&'static str> {
        if text.len() < LOOKED_FROM || text.len() < self.looked_at + EVERY {
            return None;
        }
        self.looked_at = text.len();
        repeating(text).then_some(REPEATING)
    }

    /// After a new call: why to stop the turn, if it is the same one again
    /// too many times.
    pub fn call(&mut self, call: &ToolCall) -> Option<&'static str> {
        let key = format!("{}\u{1f}{}", call.name, call.input);
        if key == self.last_call {
            self.same += 1;
        } else {
            (self.last_call, self.same) = (key, 1);
        }
        (self.same >= SAME_CALLS).then_some(LOOPING)
    }
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

    #[test]
    fn the_same_call_many_times_in_a_row_stops_the_turn() {
        let call = |input: &str| {
            let mut call = ToolCall::new("1", "Bash");
            call.input = input.into();
            call
        };
        let mut guard = Guard::default();
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
            assert_eq!(said, (n + 1 == SAME_CALLS).then_some(LOOPING));
        }
    }
}
