//! dictation.ts: what the composer's dictation shows and how its text lands
//! in the draft. The engine itself (microphone, Silero, Parakeet) is the
//! `monobot-dictation` crate, which the app links.
use std::path::PathBuf;

/// System dictation: the global shortcut, where its text goes, the paste
/// into the app in front, and what the notch shows meanwhile.
pub mod hotkey;
pub mod keycodes;
pub mod paste;
pub mod quiet;
pub mod route;
pub mod system;

/// Bars in the waveform, newest on the right.
pub const BARS: usize = 40;

/// useDictation's phases.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    /// The model could not be set up; the bar offers the download.
    NeedsModel,
    Installing,
    /// Opening the microphone.
    Starting,
    Listening,
    /// Transcribing what is left after listening stops.
    Finishing,
}

impl Phase {
    /// The mic can't be pressed while something is on its way.
    pub fn busy(self) -> bool {
        matches!(self, Phase::Starting | Phase::Finishing | Phase::Installing)
    }

    /// Escape drops a dictation while the microphone is open or opening.
    pub fn cancellable(self) -> bool {
        matches!(self, Phase::Starting | Phase::Listening)
    }

    /// The mic's tooltip and accessible name.
    pub fn button_label(self) -> &'static str {
        match self {
            Phase::Listening => "Stop dictation",
            Phase::Installing => "Setting up the speech model",
            _ => "Dictate",
        }
    }

    /// What the bar says beside the waveform, or `None` when it shows no
    /// waveform.
    pub fn caption(self, partial: &str) -> Option<&str> {
        match self {
            Phase::Starting => Some("Starting the microphone…"),
            Phase::Finishing if partial.is_empty() => Some("Finishing…"),
            Phase::Listening if partial.is_empty() => Some("Listening…"),
            Phase::Finishing | Phase::Listening => Some(partial),
            _ => None,
        }
    }
}

/// The draft after dictated text went in, and where the caret lands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dictated {
    pub value: String,
    /// A byte offset into `value`.
    pub cursor: usize,
}

/// Joins dictated text into a draft at the cursor (byte offsets `start` to
/// `end`, a selection it replaces), with one space between.
pub fn insert_dictated(value: &str, start: usize, end: usize, text: &str) -> Dictated {
    let spoken = text.trim();
    let end = floor_char_boundary(value, end.min(value.len()));
    let start = floor_char_boundary(value, start.min(end));
    if spoken.is_empty() {
        return Dictated {
            value: value.to_owned(),
            cursor: end,
        };
    }
    let before = &value[..start];
    let after = &value[end..];
    let lead = if !before.is_empty() && !before.ends_with(char::is_whitespace) {
        " "
    } else {
        ""
    };
    let trail = if !after.is_empty() && !after.starts_with(char::is_whitespace) {
        " "
    } else {
        ""
    };
    Dictated {
        value: format!("{before}{lead}{spoken}{trail}{after}"),
        cursor: before.len() + lead.len() + spoken.len(),
    }
}

fn floor_char_boundary(value: &str, mut offset: usize) -> usize {
    while !value.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

pub fn format_model_size(bytes: u64) -> String {
    format!("{} MB", (bytes as f64 / 1_000_000.).round())
}

/// What the install prompt says, and what its button does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub text: String,
    pub action: &'static str,
    pub busy: &'static str,
}

/// The app sets the model up on its own; this only shows when that could not
/// finish.
pub fn model_offer(bytes: u64) -> Offer {
    Offer {
        text: format!(
            "Dictation runs on this computer. The speech model ({}, Spanish and English) could not be set up yet. Try again.",
            format_model_size(bytes)
        ),
        action: "Download",
        busy: "Setting up the speech model…",
    }
}

/// Download progress, 0 to 1.
pub fn progress(done: u64, total: u64) -> f32 {
    if total == 0 {
        0.
    } else {
        (done as f64 / total as f64).clamp(0., 1.) as f32
    }
}

/// How long the microphone has been open, as the bar prints it: "0:07",
/// "12:30".
pub fn clock(elapsed: std::time::Duration) -> String {
    let seconds = elapsed.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// A fraction as the bar prints it.
pub fn percent(progress: f32) -> String {
    format!("{}%", (progress * 100.).round())
}

/// The newest loudness enters on the right; the oldest leaves on the left.
pub fn push_level(levels: &mut [f32; BARS], level: f32) {
    levels.rotate_left(1);
    levels[BARS - 1] = if level.is_finite() {
        level.clamp(0., 1.)
    } else {
        0.
    };
}

/// A bar's height in pixels inside the 20px waveform.
pub fn bar_height(level: f32) -> f32 {
    (level * 20.).round().max(2.)
}

/// The waveform eases each bar toward its level like the web bar's 75ms
/// height transition, `dt` seconds after the last frame.
pub fn ease_levels(shown: &mut [f32; BARS], levels: &[f32; BARS], dt: f32) {
    let step = (dt / 0.075).clamp(0., 1.);
    for (shown, level) in shown.iter_mut().zip(levels) {
        *shown += (level - *shown) * step;
    }
}

/// Where the speech model lives: Tauri's app data folder
/// (`models/dictation`), so a model either app downloaded serves both.
pub fn models_dir() -> PathBuf {
    crate::store::monocode_data_dir()
        .join("models")
        .join("dictation")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_clock_counts_minutes_and_seconds() {
        use std::time::Duration;
        assert_eq!(clock(Duration::from_millis(7_900)), "0:07");
        assert_eq!(clock(Duration::from_secs(750)), "12:30");
    }

    #[test]
    fn adds_the_spoken_text_at_the_cursor_with_one_space_around_it() {
        // "é" is two bytes, so the caret sits one byte later than the web's
        // UTF-16 offset of 12.
        assert_eq!(
            insert_dictated("Hola", 4, 4, "qué tal"),
            Dictated {
                value: "Hola qué tal".into(),
                cursor: 13,
            }
        );
        assert_eq!(
            insert_dictated("fix the bug", 4, 4, " now "),
            Dictated {
                value: "fix now the bug".into(),
                cursor: 7,
            }
        );
    }

    #[test]
    fn replaces_a_selection_and_keeps_an_empty_draft_clean() {
        assert_eq!(
            insert_dictated("say WORD here", 4, 8, "this").value,
            "say this here"
        );
        assert_eq!(
            insert_dictated("", 0, 0, "Let's ship it.").value,
            "Let's ship it."
        );
    }

    #[test]
    fn leaves_the_draft_alone_when_nothing_was_heard() {
        assert_eq!(
            insert_dictated("draft", 5, 5, "   "),
            Dictated {
                value: "draft".into(),
                cursor: 5,
            }
        );
    }

    #[test]
    fn keeps_offsets_inside_the_text() {
        // A stale caret past the end, inside a character or behind its
        // start still lands on a character boundary.
        assert_eq!(insert_dictated("ñ", 2, 9, "sí").value, "ñ sí");
        assert_eq!(insert_dictated("ñx", 1, 1, "a").value, "a ñx");
        assert_eq!(insert_dictated("hi", 5, 1, "yo").value, "h yo i");
    }

    #[test]
    fn names_the_model_size_in_megabytes() {
        assert_eq!(format_model_size(671_122_626), "671 MB");
    }

    #[test]
    fn offers_a_retry_that_names_the_size_never_another_app() {
        let offer = model_offer(671_122_626);
        assert!(offer.text.contains("671 MB"));
        assert!(!offer.text.contains("Echo"));
        assert_eq!(offer.action, "Download");
        assert_eq!(offer.busy, "Setting up the speech model…");
    }

    #[test]
    fn the_mic_and_bar_say_what_each_phase_is_doing() {
        assert_eq!(Phase::Idle.button_label(), "Dictate");
        assert_eq!(Phase::Listening.button_label(), "Stop dictation");
        assert_eq!(
            Phase::Installing.button_label(),
            "Setting up the speech model"
        );
        assert!(Phase::Starting.busy() && Phase::Finishing.busy() && Phase::Installing.busy());
        assert!(!Phase::Idle.busy() && !Phase::Listening.busy() && !Phase::NeedsModel.busy());
        assert_eq!(
            Phase::Starting.caption("x"),
            Some("Starting the microphone…")
        );
        assert_eq!(Phase::Listening.caption(""), Some("Listening…"));
        assert_eq!(Phase::Listening.caption("Hola"), Some("Hola"));
        assert_eq!(Phase::Finishing.caption(""), Some("Finishing…"));
        assert_eq!(Phase::Idle.caption("Hola"), None);
        assert_eq!(Phase::NeedsModel.caption(""), None);
    }

    #[test]
    fn the_waveform_slides_left_and_bars_keep_a_floor() {
        let mut levels = [0.; BARS];
        push_level(&mut levels, 0.5);
        push_level(&mut levels, 2.);
        push_level(&mut levels, f32::NAN);
        assert_eq!(levels[BARS - 3..], [0.5, 1., 0.]);
        assert_eq!(bar_height(0.), 2.);
        assert_eq!(bar_height(0.5), 10.);
        assert_eq!(bar_height(1.), 20.);
    }

    #[test]
    fn bars_ease_toward_their_level_within_the_transition() {
        let mut shown = [0.; BARS];
        let levels = [1.; BARS];
        ease_levels(&mut shown, &levels, 0.033);
        assert!(shown[0] > 0.4 && shown[0] < 0.5, "{}", shown[0]);
        ease_levels(&mut shown, &levels, 0.075);
        assert_eq!(shown, levels);
    }

    #[test]
    fn progress_is_a_fraction_of_the_download() {
        assert_eq!(progress(0, 0), 0.);
        assert_eq!(progress(50, 200), 0.25);
        assert_eq!(progress(300, 200), 1.);
        assert_eq!(percent(0.256), "26%");
        assert_eq!(percent(1.), "100%");
    }

    #[test]
    fn the_model_folder_is_the_desktop_apps() {
        let dir = models_dir();
        assert!(dir.ends_with("models/dictation"), "{}", dir.display());
        assert!(
            dir.starts_with(crate::store::monocode_data_dir()),
            "{}",
            dir.display()
        );
    }
}
