//! How dictated text reaches the app in front: the steps and timings of a
//! paste through the clipboard (the app runs them off the UI thread), what
//! happens when nothing can take the text, and the last dictations kept for
//! Paste last.
use super::hotkey::{Modifiers, Os, Shortcut};
use std::collections::VecDeque;
use std::time::Duration;

/// After the text goes on the clipboard, before the paste chord: lets the
/// clipboard owner settle so the app in front reads the new text.
pub const SETTLE: Duration = Duration::from_millis(30);
/// After the paste chord, before the user's clipboard comes back: the app
/// in front has read it by then.
pub const RESTORE_AFTER: Duration = Duration::from_millis(500);
/// How many dictations Paste last keeps, in memory only.
pub const KEPT: usize = 20;

/// The chord that pastes on `os`: Ctrl+V, or ⌘V on macOS.
pub fn paste_chord(os: Os) -> Shortcut {
    let modifiers = if os == Os::Mac {
        Modifiers {
            meta: true,
            ..Modifiers::default()
        }
    } else {
        Modifiers {
            ctrl: true,
            ..Modifiers::default()
        }
    };
    Shortcut::new(modifiers, "V")
}

/// One step of a paste, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Keep what the clipboard holds now (text, pictures, files), to put
    /// back afterwards.
    SaveClipboard,
    /// Put the dictated text on the clipboard.
    WriteText(String),
    Wait(Duration),
    /// Send the paste chord to the app in front.
    SendPaste(Shortcut),
    /// Put the saved clipboard back, but only if it still holds our text
    /// (see [`still_ours`]): the user may have copied something meanwhile.
    RestoreIfStillOurs,
}

/// How a dictation ended, for the notch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Pasted into the app in front.
    Pasted,
    /// Nothing could take it: the text stays on the clipboard.
    Copied,
}

/// A paste to run, and what the notch says once it has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub steps: Vec<Step>,
    pub outcome: Outcome,
}

/// The steps that put `text` where the caret is on `os`. `has_target` is
/// whether an app in front can take a paste (a focused window that is not
/// the desktop, the notch or a locked screen). Without one the text is only
/// copied and left on the clipboard. Blank text plans nothing.
pub fn plan(text: &str, os: Os, has_target: bool) -> Option<Plan> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if !has_target {
        return Some(Plan {
            steps: vec![Step::WriteText(text.to_owned())],
            outcome: Outcome::Copied,
        });
    }
    Some(Plan {
        steps: vec![
            Step::SaveClipboard,
            Step::WriteText(text.to_owned()),
            Step::Wait(SETTLE),
            Step::SendPaste(paste_chord(os)),
            Step::Wait(RESTORE_AFTER),
            Step::RestoreIfStillOurs,
        ],
        outcome: Outcome::Pasted,
    })
}

/// Whether the clipboard, read just before restoring, still holds the text
/// we put there. Anything else (other text, a picture, empty) is the user's
/// own copy and stays.
pub fn still_ours(clipboard_now: Option<&str>, ours: &str) -> bool {
    clipboard_now.is_some_and(|now| now == ours.trim())
}

/// The last dictations, newest first, for Paste last. Memory only: what was
/// said never reaches disk.
#[derive(Clone, Debug, Default)]
pub struct History {
    items: VecDeque<String>,
}

impl History {
    /// Keeps a dictation. Blank text is skipped, and saying the same thing
    /// twice in a row keeps it once.
    pub fn push(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() || self.items.front().is_some_and(|last| last == text) {
            return;
        }
        self.items.push_front(text.to_owned());
        self.items.truncate(KEPT);
    }

    /// The newest dictation.
    pub fn last(&self) -> Option<&str> {
        self.items.front().map(String::as_str)
    }

    /// Newest first.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.items.iter().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pastes_with_ctrl_v_or_cmd_v() {
        assert_eq!(paste_chord(Os::Windows).display(Os::Windows), "Ctrl+V");
        assert_eq!(paste_chord(Os::Linux).display(Os::Linux), "Ctrl+V");
        assert_eq!(paste_chord(Os::Mac).display(Os::Mac), "⌘V");
    }

    #[test]
    fn a_paste_saves_writes_pastes_and_restores_after_500_ms() {
        let plan = plan("  hola mundo ", Os::Windows, true).unwrap();
        assert_eq!(plan.outcome, Outcome::Pasted);
        assert_eq!(
            plan.steps,
            vec![
                Step::SaveClipboard,
                Step::WriteText("hola mundo".into()),
                Step::Wait(SETTLE),
                Step::SendPaste(paste_chord(Os::Windows)),
                Step::Wait(Duration::from_millis(500)),
                Step::RestoreIfStillOurs,
            ]
        );
        let mac = super::plan("hi", Os::Mac, true).unwrap();
        assert!(mac.steps.contains(&Step::SendPaste(paste_chord(Os::Mac))));
        // The clipboard is saved before it is written and restored last.
        let total: Duration = plan
            .steps
            .iter()
            .filter_map(|step| match step {
                Step::Wait(wait) => Some(*wait),
                _ => None,
            })
            .sum();
        assert!(total >= RESTORE_AFTER);
    }

    #[test]
    fn with_nothing_in_front_the_text_is_only_copied() {
        let plan = plan("hola", Os::Mac, false).unwrap();
        assert_eq!(plan.outcome, Outcome::Copied);
        assert_eq!(plan.steps, vec![Step::WriteText("hola".into())]);
    }

    #[test]
    fn blank_text_plans_nothing() {
        assert_eq!(plan("", Os::Windows, true), None);
        assert_eq!(plan(" \n\t", Os::Mac, false), None);
    }

    #[test]
    fn restores_only_while_the_clipboard_holds_our_text() {
        assert!(still_ours(Some("hola"), " hola "));
        assert!(!still_ours(Some("something the user copied"), "hola"));
        assert!(!still_ours(None, "hola"));
    }

    #[test]
    fn paste_last_keeps_the_newest_twenty() {
        let mut history = History::default();
        assert_eq!(history.last(), None);
        assert!(history.is_empty());
        for n in 0..25 {
            history.push(&format!("dictation {n}"));
        }
        assert_eq!(history.len(), KEPT);
        assert_eq!(history.last(), Some("dictation 24"));
        assert_eq!(history.iter().last(), Some("dictation 5"));
        history.clear();
        assert!(history.is_empty());
    }

    #[test]
    fn paste_last_skips_blanks_and_repeats() {
        let mut history = History::default();
        history.push("  ");
        history.push(" uno ");
        history.push("uno");
        history.push("dos");
        history.push("uno");
        assert_eq!(history.iter().collect::<Vec<_>>(), ["uno", "dos", "uno"]);
    }
}
