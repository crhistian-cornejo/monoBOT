//! What the notch paints while dictating into another app: the phase (right
//! wing: mic, level, "Dictating"), and the words as they arrive (left wing:
//! the newest that fit, with the ones new since the last frame marked so
//! they can slide in).
use super::hotkey::Os;
use super::paste::{paste_chord, Outcome};
use std::time::Duration;

/// Where a system dictation is.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Phase {
    #[default]
    Idle,
    /// Opening the microphone.
    Starting,
    /// `level` is the microphone's loudness, 0 to 1; `words` what was heard
    /// so far.
    Listening {
        level: f32,
        words: String,
    },
    /// Transcribing what is left after listening stopped.
    Finishing,
    Pasted,
    /// Nothing could take the text; it is on the clipboard.
    Copied,
    Error(String),
}

impl Phase {
    pub fn from_outcome(outcome: Outcome) -> Self {
        match outcome {
            Outcome::Pasted => Phase::Pasted,
            Outcome::Copied => Phase::Copied,
        }
    }

    /// The notch shows the dictation wings.
    pub fn visible(&self) -> bool {
        *self != Phase::Idle
    }

    /// The microphone is open or opening, so Escape cancels.
    pub fn cancellable(&self) -> bool {
        matches!(self, Phase::Starting | Phase::Listening { .. })
    }

    /// What the right wing says beside the mic.
    pub fn label(&self, os: Os) -> String {
        match self {
            Phase::Idle => String::new(),
            Phase::Starting => "Starting…".into(),
            Phase::Listening { .. } => "Dictating".into(),
            Phase::Finishing => "Finishing…".into(),
            Phase::Pasted => "Pasted".into(),
            Phase::Copied => format!("Copied · {} to paste", paste_chord(os).display(os)),
            Phase::Error(message) => message.clone(),
        }
    }

    /// The level bars' loudness; quiet outside listening.
    pub fn level(&self) -> f32 {
        match self {
            Phase::Listening { level, .. } if level.is_finite() => level.clamp(0., 1.),
            _ => 0.,
        }
    }

    /// What was heard so far.
    pub fn words(&self) -> &str {
        match self {
            Phase::Listening { words, .. } => words,
            _ => "",
        }
    }

    /// How long an ending stays on the notch before it goes back to idle;
    /// `None` while the dictation is still going.
    pub fn linger(&self) -> Option<Duration> {
        match self {
            Phase::Pasted => Some(Duration::from_millis(1_200)),
            Phase::Copied => Some(Duration::from_millis(3_000)),
            Phase::Error(_) => Some(Duration::from_millis(4_000)),
            _ => None,
        }
    }
}

/// The words the left wing shows this frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Wing {
    /// Oldest to newest. A last word longer than the whole budget is cut
    /// from the left, behind "…".
    pub words: Vec<String>,
    /// How many words at the end are new since the last frame (or changed:
    /// a partial that revised its last word counts it again).
    pub fresh: usize,
    /// Older words were left out, so the wing fades its left edge.
    pub clipped: bool,
}

/// The newest words of `current` that fit in `budget` characters (counting
/// one space between words), and which of them `previous`, the last frame's
/// text, did not end with.
pub fn wing(previous: &str, current: &str, budget: usize) -> Wing {
    let all: Vec<&str> = current.split_whitespace().collect();
    if all.is_empty() || budget == 0 {
        return Wing {
            clipped: !all.is_empty(),
            ..Wing::default()
        };
    }
    let kept = previous
        .split_whitespace()
        .zip(&all)
        .take_while(|(was, now)| was == *now)
        .count();

    let mut start = all.len();
    let mut used = 0;
    while start > 0 {
        let word = all[start - 1].chars().count();
        let need = word + usize::from(start < all.len());
        if used + need > budget {
            break;
        }
        used += need;
        start -= 1;
    }

    let mut words: Vec<String> = all[start..].iter().map(|w| (*w).to_owned()).collect();
    if start == all.len() {
        // The newest word alone is too long: show its end.
        start -= 1;
        let last = all[start];
        let tail: String = {
            let chars: Vec<char> = last.chars().collect();
            chars[chars.len() + 1 - budget..].iter().collect()
        };
        words = vec![format!("…{tail}")];
    }
    Wing {
        fresh: all.len() - kept.max(start),
        clipped: start > 0,
        words,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(wing: &Wing) -> Vec<&str> {
        wing.words.iter().map(String::as_str).collect()
    }

    #[test]
    fn the_right_wing_says_what_is_happening() {
        let listening = Phase::Listening {
            level: 0.4,
            words: "hola".into(),
        };
        assert_eq!(listening.label(Os::Windows), "Dictating");
        assert_eq!(listening.level(), 0.4);
        assert_eq!(listening.words(), "hola");
        assert!(listening.visible() && listening.cancellable());
        assert_eq!(Phase::Starting.label(Os::Mac), "Starting…");
        assert!(Phase::Starting.cancellable());
        assert_eq!(Phase::Finishing.label(Os::Mac), "Finishing…");
        assert!(!Phase::Finishing.cancellable());
        assert_eq!(Phase::Pasted.label(Os::Mac), "Pasted");
        assert_eq!(Phase::Copied.label(Os::Mac), "Copied · ⌘V to paste");
        assert_eq!(Phase::Copied.label(Os::Windows), "Copied · Ctrl+V to paste");
        assert_eq!(
            Phase::Error("No microphone".into()).label(Os::Linux),
            "No microphone"
        );
        assert!(!Phase::Idle.visible());
        assert_eq!(Phase::Idle.label(Os::Mac), "");
    }

    #[test]
    fn the_level_stays_between_zero_and_one() {
        let loud = Phase::Listening {
            level: 3.,
            words: String::new(),
        };
        assert_eq!(loud.level(), 1.);
        let broken = Phase::Listening {
            level: f32::NAN,
            words: String::new(),
        };
        assert_eq!(broken.level(), 0.);
        assert_eq!(Phase::Finishing.level(), 0.);
        assert_eq!(Phase::Finishing.words(), "");
    }

    #[test]
    fn endings_linger_and_then_leave() {
        assert_eq!(Phase::from_outcome(Outcome::Pasted), Phase::Pasted);
        assert_eq!(Phase::from_outcome(Outcome::Copied), Phase::Copied);
        let pasted = Phase::Pasted.linger().unwrap();
        let copied = Phase::Copied.linger().unwrap();
        assert!(copied > pasted, "Copied needs time to read the chord");
        assert!(Phase::Error("x".into()).linger().is_some());
        for going in [
            Phase::Idle,
            Phase::Starting,
            Phase::Finishing,
            Phase::Listening {
                level: 0.,
                words: String::new(),
            },
        ] {
            assert_eq!(going.linger(), None, "{going:?}");
        }
    }

    #[test]
    fn the_left_wing_keeps_the_newest_words_that_fit() {
        let wing = wing("", "uno dos tres cuatro", 12);
        // "tres cuatro" is 11 characters; "dos" would make 15.
        assert_eq!(words(&wing), ["tres", "cuatro"]);
        assert!(wing.clipped);
        let roomy = super::wing("", "uno dos", 40);
        assert_eq!(words(&roomy), ["uno", "dos"]);
        assert!(!roomy.clipped);
        // Exactly the budget fits.
        assert_eq!(words(&super::wing("", "ab cd", 5)), ["ab", "cd"]);
        assert_eq!(words(&super::wing("", "ab cd", 4)), ["cd"]);
    }

    #[test]
    fn counts_characters_not_bytes() {
        // "acción" is 6 characters and 7 bytes.
        let wing = wing("", "una acción", 10);
        assert_eq!(words(&wing), ["una", "acción"]);
    }

    #[test]
    fn a_word_longer_than_the_wing_shows_its_end() {
        let wing = wing("", "hola supercalifragilístico", 8);
        assert_eq!(words(&wing), ["…lístico"]);
        assert_eq!(wing.words[0].chars().count(), 8 + 1 - 1);
        assert!(wing.clipped);
        assert_eq!(wing.fresh, 1);
        assert_eq!(words(&super::wing("", "abc", 1)), ["…"]);
    }

    #[test]
    fn marks_the_words_that_arrived_since_the_last_frame() {
        let first = wing("", "hola", 40);
        assert_eq!(first.fresh, 1);
        let second = wing("hola", "hola qué tal", 40);
        assert_eq!(second.fresh, 2);
        let same = wing("hola qué tal", "hola qué tal", 40);
        assert_eq!(same.fresh, 0);
        // The recognizer revised its last word: it slides in again.
        let revised = wing("hola qué tal", "hola qué talento", 40);
        assert_eq!(revised.fresh, 1);
        // Spacing alone is not a change.
        assert_eq!(wing("hola  qué", " hola qué ", 40).fresh, 0);
    }

    #[test]
    fn fresh_words_never_outnumber_the_visible_ones() {
        let wing = wing("", "uno dos tres cuatro cinco", 12);
        assert_eq!(words(&wing), ["cuatro", "cinco"]);
        assert_eq!(wing.fresh, 2);
        let later = super::wing("uno dos tres", "uno dos tres cuatro cinco", 12);
        assert_eq!(later.fresh, 2);
        let older = super::wing("uno dos tres cuatro", "uno dos tres cuatro cinco", 12);
        assert_eq!(older.fresh, 1);
    }

    #[test]
    fn nothing_heard_shows_nothing() {
        assert_eq!(wing("", "", 20), Wing::default());
        assert_eq!(wing("hola", "  ", 20), Wing::default());
        let none = wing("", "hola", 0);
        assert!(none.words.is_empty() && none.clipped && none.fresh == 0);
    }
}
