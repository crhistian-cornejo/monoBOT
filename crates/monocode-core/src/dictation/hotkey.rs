//! The global dictation shortcut: what it is (modifiers and a key, Ctrl+Space
//! by default), how it is written and read back, which choices clash with
//! the system or common apps, and the tap / hold rule that turns its presses
//! and releases into start, stop and cancel.
use serde::{Deserialize, Serialize};
use std::fmt;

/// A press shorter than this toggles dictation; held longer, it is push to
/// talk and letting go stops. The composer's mic uses the same rule.
pub const HOLD_MS: u64 = 300;

/// The system a shortcut is shown for, or a paste is sent on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
    Linux,
}

impl Os {
    /// This computer.
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

/// The modifiers held with the key. `meta` is ⌘ on macOS and the Windows
/// key elsewhere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

impl Modifiers {
    pub fn any(self) -> bool {
        self.ctrl || self.alt || self.shift || self.meta
    }
}

/// A key and the modifiers held with it. Stored in settings as its
/// platform-neutral text ("Ctrl+Space", "Ctrl+Alt+D", "Meta+H").
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Shortcut {
    pub modifiers: Modifiers,
    /// The key's canonical name: "Space", "A", "7", "F5", "Enter", "Up"…
    pub key: String,
}

impl Default for Shortcut {
    /// Ctrl+Space, on every system.
    fn default() -> Self {
        Shortcut::new(
            Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
            "Space",
        )
    }
}

/// Why a typed shortcut was not accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    Empty,
    /// Only modifiers, no key.
    NoKey,
    /// Two keys, as in "Ctrl+A+B".
    TwoKeys,
    UnknownKey(String),
    /// A plain key (other than F1–F24) would fire on every keystroke.
    NeedsModifier,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Empty => write!(f, "Press a shortcut."),
            ParseError::NoKey => write!(f, "Add a key to the modifiers."),
            ParseError::TwoKeys => write!(f, "A shortcut has one key."),
            ParseError::UnknownKey(key) => write!(f, "\"{key}\" is not a key monoBOT knows."),
            ParseError::NeedsModifier => write!(
                f,
                "Hold Ctrl, Alt, Shift or {} with the key, or use F1–F24.",
                if Os::current() == Os::Mac {
                    "⌘"
                } else {
                    "Win"
                }
            ),
        }
    }
}

impl std::error::Error for ParseError {}

/// A modifier glyph as macOS draws it (⌃⌥⇧⌘).
fn mac_glyph(glyph: char, modifiers: &mut Modifiers) -> bool {
    match glyph {
        '⌃' => modifiers.ctrl = true,
        '⌥' => modifiers.alt = true,
        '⇧' => modifiers.shift = true,
        '⌘' => modifiers.meta = true,
        _ => return false,
    }
    true
}

fn modifier(token: &str, modifiers: &mut Modifiers) -> bool {
    match token.to_ascii_lowercase().as_str() {
        "ctrl" | "control" | "ctl" => modifiers.ctrl = true,
        "alt" | "option" | "opt" => modifiers.alt = true,
        "shift" => modifiers.shift = true,
        "meta" | "cmd" | "command" | "win" | "windows" | "super" => modifiers.meta = true,
        _ => return false,
    }
    true
}

/// The canonical name of a key, or `None` for one it does not know.
fn key_name(token: &str) -> Option<String> {
    let lower = token.to_ascii_lowercase();
    let named = match lower.as_str() {
        "space" | "spacebar" | "␣" => "Space",
        "enter" | "return" | "↩" | "⏎" => "Enter",
        "tab" | "⇥" => "Tab",
        "esc" | "escape" | "⎋" => "Escape",
        "backspace" | "⌫" => "Backspace",
        "delete" | "del" | "⌦" => "Delete",
        "insert" | "ins" => "Insert",
        "home" => "Home",
        "end" => "End",
        "pageup" | "pgup" => "PageUp",
        "pagedown" | "pgdn" => "PageDown",
        "up" | "↑" => "Up",
        "down" | "↓" => "Down",
        "left" | "←" => "Left",
        "right" | "→" => "Right",
        "plus" => "Plus",
        "minus" => "Minus",
        _ => "",
    };
    if !named.is_empty() {
        return Some(named.to_owned());
    }
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        return (1..=24).contains(&n).then(|| format!("F{n}"));
    }
    let mut chars = token.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphanumeric() => Some(c.to_ascii_uppercase().to_string()),
        (Some(c), None) if c.is_ascii_punctuation() && c != '+' => Some(c.to_string()),
        _ => None,
    }
}

impl Shortcut {
    pub fn new(modifiers: Modifiers, key: &str) -> Self {
        Shortcut {
            modifiers,
            key: key.to_owned(),
        }
    }

    /// Reads a shortcut as people write it: "Ctrl+Space", "ctrl + alt + d",
    /// "Cmd+Shift+M", "Win+H", or macOS glyphs ("⌃Space", "⌘⇧M").
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(ParseError::Empty);
        }
        let mut modifiers = Modifiers::default();
        // Leading macOS glyphs run straight into the key.
        let mut rest = text;
        while let Some(c) = rest.chars().next() {
            if !mac_glyph(c, &mut modifiers) {
                break;
            }
            rest = &rest[c.len_utf8()..];
        }
        // "Ctrl++" names the plus key.
        let (rest, plus) = match rest.strip_suffix("++") {
            Some(head) => (head, true),
            None => (rest, rest == "+"),
        };
        let mut key = plus.then(|| "Plus".to_owned());
        for token in rest.split('+').map(str::trim).filter(|t| !t.is_empty()) {
            if modifier(token, &mut modifiers) {
                continue;
            }
            let name = key_name(token).ok_or_else(|| ParseError::UnknownKey(token.to_owned()))?;
            if key.replace(name).is_some() {
                return Err(ParseError::TwoKeys);
            }
        }
        let key = key.ok_or(ParseError::NoKey)?;
        let function_key = key.len() > 1 && key.starts_with('F') && key[1..].parse::<u8>().is_ok();
        if !modifiers.any() && !function_key {
            return Err(ParseError::NeedsModifier);
        }
        Ok(Shortcut { modifiers, key })
    }

    /// The shortcut as Settings and tooltips show it on `os`: "Ctrl+Space"
    /// on Windows and Linux, "⌃Space" on macOS.
    pub fn display(&self, os: Os) -> String {
        let m = self.modifiers;
        if os == Os::Mac {
            let mut out = String::new();
            for (on, glyph) in [(m.ctrl, '⌃'), (m.alt, '⌥'), (m.shift, '⇧'), (m.meta, '⌘')]
            {
                if on {
                    out.push(glyph);
                }
            }
            out.push_str(&self.key);
            return out;
        }
        let meta = if os == Os::Windows { "Win" } else { "Super" };
        let mut parts: Vec<&str> = Vec::new();
        for (on, name) in [
            (m.ctrl, "Ctrl"),
            (m.alt, "Alt"),
            (m.shift, "Shift"),
            (m.meta, meta),
        ] {
            if on {
                parts.push(name);
            }
        }
        parts.push(&self.key);
        parts.join("+")
    }

    /// The platform-neutral text settings store; `parse` reads it back.
    pub fn stored(&self) -> String {
        let m = self.modifiers;
        let mut parts: Vec<&str> = Vec::new();
        for (on, name) in [
            (m.ctrl, "Ctrl"),
            (m.alt, "Alt"),
            (m.shift, "Shift"),
            (m.meta, "Meta"),
        ] {
            if on {
                parts.push(name);
            }
        }
        parts.push(&self.key);
        parts.join("+")
    }

    /// A known clash on `os`, as the warning Settings shows next to the
    /// shortcut. The shortcut still works; the other use may win or be lost.
    pub fn conflict(&self, os: Os) -> Option<&'static str> {
        let m = self.modifiers;
        let only = |ctrl, alt, shift, meta| {
            m == Modifiers {
                ctrl,
                alt,
                shift,
                meta,
            }
        };
        match (self.key.as_str(), os) {
            ("Space", Os::Mac) if only(true, false, false, false) => Some(
                "macOS uses Ctrl+Space to switch input sources. Turn that off in System Settings › Keyboard › Keyboard Shortcuts › Input Sources, or pick another shortcut. Editors also use it for completion.",
            ),
            ("Space", Os::Windows) if only(true, false, false, false) => Some(
                "Windows uses Ctrl+Space to turn the input method (IME) on and off for Chinese, Japanese and Korean, and editors such as VS Code use it for completion.",
            ),
            ("Space", Os::Linux) if only(true, false, false, false) => Some(
                "Input methods (IBus, Fcitx) and editors such as VS Code use Ctrl+Space too.",
            ),
            ("Space", Os::Mac) if only(false, false, false, true) => {
                Some("⌘Space opens Spotlight.")
            }
            ("H", Os::Windows) if only(false, false, false, true) => {
                Some("Win+H opens Windows voice typing.")
            }
            _ => None,
        }
    }
}

impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display(Os::current()))
    }
}

impl From<Shortcut> for String {
    fn from(shortcut: Shortcut) -> String {
        shortcut.stored()
    }
}

impl TryFrom<String> for Shortcut {
    type Error = ParseError;
    fn try_from(text: String) -> Result<Self, ParseError> {
        Shortcut::parse(&text)
    }
}

/// What the keyboard hook reports about the shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyEvent {
    /// The shortcut went down. A held key repeats this; only the first
    /// counts.
    Press,
    /// Its key came up.
    Release,
    /// Escape went down.
    Escape,
}

/// What the dictation should do after an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Start,
    /// Stop listening and use what was heard.
    Stop,
    /// Drop the dictation.
    Cancel,
}

/// Where the tap / hold rule stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Idle,
    /// Listening while the key is held; `since` is when it went down.
    Recording { since: u64 },
    /// A tap started it; it listens until the next press.
    Toggled,
}

/// Turns the shortcut's presses and releases (with the time in
/// milliseconds) into start, stop and cancel. A tap toggles; a hold longer
/// than [`HOLD_MS`] is push to talk.
#[derive(Clone, Copy, Debug, Default)]
pub struct PressMachine {
    state: State,
    /// The key is physically down, so further presses are auto-repeat.
    down: bool,
}

impl PressMachine {
    pub fn state(&self) -> State {
        self.state
    }

    pub fn recording(&self) -> bool {
        self.state != State::Idle
    }

    /// Feeds one event. `None` means nothing changes (and an Escape that
    /// returns `None` belongs to the app in front).
    pub fn event(&mut self, event: KeyEvent, now_ms: u64) -> Option<Action> {
        match event {
            KeyEvent::Press => {
                if std::mem::replace(&mut self.down, true) {
                    return None;
                }
                match self.state {
                    State::Idle => {
                        self.state = State::Recording { since: now_ms };
                        Some(Action::Start)
                    }
                    // A press while listening ends it; its release is
                    // ignored.
                    State::Recording { .. } | State::Toggled => {
                        self.state = State::Idle;
                        Some(Action::Stop)
                    }
                }
            }
            KeyEvent::Release => {
                self.down = false;
                match self.state {
                    State::Recording { since } if now_ms.saturating_sub(since) < HOLD_MS => {
                        self.state = State::Toggled;
                        None
                    }
                    State::Recording { .. } => {
                        self.state = State::Idle;
                        Some(Action::Stop)
                    }
                    State::Idle | State::Toggled => None,
                }
            }
            KeyEvent::Escape => {
                if self.state == State::Idle {
                    return None;
                }
                self.state = State::Idle;
                Some(Action::Cancel)
            }
        }
    }

    /// The dictation ended on its own (an error, the window took it over):
    /// the next press starts a new one. A key still held stays held, so its
    /// repeats do not start one.
    pub fn reset(&mut self) {
        self.state = State::Idle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl(key: &str) -> Shortcut {
        Shortcut::new(
            Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
            key,
        )
    }

    #[test]
    fn the_default_is_ctrl_space_shown_for_each_system() {
        let shortcut = Shortcut::default();
        assert_eq!(shortcut, ctrl("Space"));
        assert_eq!(shortcut.display(Os::Windows), "Ctrl+Space");
        assert_eq!(shortcut.display(Os::Linux), "Ctrl+Space");
        assert_eq!(shortcut.display(Os::Mac), "⌃Space");
    }

    #[test]
    fn reads_shortcuts_as_people_write_them() {
        assert_eq!(Shortcut::parse("Ctrl+Space"), Ok(ctrl("Space")));
        assert_eq!(Shortcut::parse(" control + space "), Ok(ctrl("Space")));
        assert_eq!(Shortcut::parse("⌃Space"), Ok(ctrl("Space")));
        assert_eq!(Shortcut::parse("ctrl+d"), Ok(ctrl("D")));
        let all = Shortcut::parse("⌘⇧⌥⌃m").unwrap();
        assert_eq!(
            all.modifiers,
            Modifiers {
                ctrl: true,
                alt: true,
                shift: true,
                meta: true
            }
        );
        assert_eq!(all.display(Os::Mac), "⌃⌥⇧⌘M");
        assert_eq!(all.display(Os::Windows), "Ctrl+Alt+Shift+Win+M");
        assert_eq!(all.display(Os::Linux), "Ctrl+Alt+Shift+Super+M");
        let win_h = Shortcut::parse("Win+H").unwrap();
        assert_eq!(win_h, Shortcut::parse("cmd+h").unwrap());
        assert_eq!(Shortcut::parse("Ctrl++").unwrap().key, "Plus");
        assert_eq!(Shortcut::parse("Alt+F13").unwrap().key, "F13");
        assert_eq!(Shortcut::parse("F9").unwrap().key, "F9");
        assert_eq!(Shortcut::parse("Ctrl+esc").unwrap().key, "Escape");
        assert_eq!(Shortcut::parse("Shift+/").unwrap().key, "/");
    }

    #[test]
    fn refuses_what_is_not_a_shortcut() {
        assert_eq!(Shortcut::parse("  "), Err(ParseError::Empty));
        assert_eq!(Shortcut::parse("Ctrl+Shift"), Err(ParseError::NoKey));
        assert_eq!(Shortcut::parse("Ctrl+A+B"), Err(ParseError::TwoKeys));
        assert_eq!(Shortcut::parse("Space"), Err(ParseError::NeedsModifier));
        assert_eq!(Shortcut::parse("a"), Err(ParseError::NeedsModifier));
        assert_eq!(
            Shortcut::parse("Ctrl+Banana"),
            Err(ParseError::UnknownKey("Banana".into()))
        );
        assert_eq!(
            Shortcut::parse("Ctrl+F25"),
            Err(ParseError::UnknownKey("F25".into()))
        );
        assert!(!ParseError::NeedsModifier.to_string().is_empty());
    }

    #[test]
    fn settings_store_the_neutral_text_and_read_it_back() {
        let shortcut = Shortcut::parse("⌘⇧D").unwrap();
        assert_eq!(shortcut.stored(), "Shift+Meta+D");
        let json = serde_json::to_string(&shortcut).unwrap();
        assert_eq!(json, "\"Shift+Meta+D\"");
        assert_eq!(serde_json::from_str::<Shortcut>(&json).unwrap(), shortcut);
        assert_eq!(
            serde_json::from_str::<Shortcut>("\"Ctrl+Space\"").unwrap(),
            Shortcut::default()
        );
        assert!(serde_json::from_str::<Shortcut>("\"Banana\"").is_err());
        for os in [Os::Mac, Os::Windows, Os::Linux] {
            let shown = shortcut.display(os);
            assert_eq!(Shortcut::parse(&shown).unwrap(), shortcut, "{shown}");
        }
    }

    #[test]
    fn warns_about_ctrl_space_and_win_h() {
        let ctrl_space = Shortcut::default();
        let mac = ctrl_space.conflict(Os::Mac).unwrap();
        assert!(mac.contains("input sources"), "{mac}");
        let windows = ctrl_space.conflict(Os::Windows).unwrap();
        assert!(
            windows.contains("IME") && windows.contains("completion"),
            "{windows}"
        );
        assert!(ctrl_space.conflict(Os::Linux).is_some());
        let win_h = Shortcut::parse("Win+H").unwrap();
        assert!(win_h
            .conflict(Os::Windows)
            .unwrap()
            .contains("voice typing"));
        assert_eq!(win_h.conflict(Os::Mac), None);
        assert!(Shortcut::parse("Cmd+Space")
            .unwrap()
            .conflict(Os::Mac)
            .is_some());
        // Other modifiers on the same key are free.
        assert_eq!(
            Shortcut::parse("Ctrl+Shift+Space")
                .unwrap()
                .conflict(Os::Mac),
            None
        );
        assert_eq!(
            Shortcut::parse("Ctrl+Alt+D").unwrap().conflict(Os::Windows),
            None
        );
    }

    use KeyEvent::{Escape, Press, Release};

    #[test]
    fn a_tap_toggles_and_the_next_press_stops() {
        let mut machine = PressMachine::default();
        assert_eq!(machine.event(Press, 1_000), Some(Action::Start));
        assert_eq!(machine.state(), State::Recording { since: 1_000 });
        assert_eq!(machine.event(Release, 1_120), None);
        assert_eq!(machine.state(), State::Toggled);
        assert!(machine.recording());
        assert_eq!(machine.event(Press, 5_000), Some(Action::Stop));
        // That press's release, short or long, does nothing.
        assert_eq!(machine.event(Release, 6_000), None);
        assert_eq!(machine.state(), State::Idle);
        assert_eq!(machine.event(Press, 7_000), Some(Action::Start));
    }

    #[test]
    fn a_hold_is_push_to_talk() {
        let mut machine = PressMachine::default();
        assert_eq!(machine.event(Press, 0), Some(Action::Start));
        assert_eq!(machine.event(Release, 2_500), Some(Action::Stop));
        assert_eq!(machine.state(), State::Idle);
    }

    #[test]
    fn the_hold_threshold_is_300_ms() {
        let mut short = PressMachine::default();
        short.event(Press, 10);
        assert_eq!(short.event(Release, 10 + HOLD_MS - 1), None);
        assert_eq!(short.state(), State::Toggled);

        let mut long = PressMachine::default();
        long.event(Press, 10);
        assert_eq!(long.event(Release, 10 + HOLD_MS), Some(Action::Stop));

        // A clock that went backwards counts as a tap.
        let mut skewed = PressMachine::default();
        skewed.event(Press, 500);
        assert_eq!(skewed.event(Release, 100), None);
        assert_eq!(skewed.state(), State::Toggled);
    }

    #[test]
    fn auto_repeat_is_ignored() {
        let mut machine = PressMachine::default();
        assert_eq!(machine.event(Press, 0), Some(Action::Start));
        for t in (30..2_000).step_by(33) {
            assert_eq!(machine.event(Press, t), None);
        }
        assert_eq!(machine.state(), State::Recording { since: 0 });
        assert_eq!(machine.event(Release, 2_000), Some(Action::Stop));

        // Holding the press that stopped a toggled dictation does not start
        // another one.
        let mut toggled = PressMachine::default();
        toggled.event(Press, 0);
        toggled.event(Release, 100);
        assert_eq!(toggled.event(Press, 3_000), Some(Action::Stop));
        assert_eq!(toggled.event(Press, 3_500), None);
        assert_eq!(toggled.event(Press, 4_000), None);
        assert_eq!(toggled.event(Release, 4_100), None);
        assert_eq!(toggled.event(Press, 5_000), Some(Action::Start));
    }

    #[test]
    fn escape_cancels_while_listening_and_passes_through_otherwise() {
        let mut held = PressMachine::default();
        assert_eq!(held.event(Escape, 0), None);
        held.event(Press, 0);
        assert_eq!(held.event(Escape, 1_000), Some(Action::Cancel));
        // The held key's release after the cancel stops nothing.
        assert_eq!(held.event(Release, 1_500), None);
        assert_eq!(held.event(Escape, 1_600), None);

        let mut toggled = PressMachine::default();
        toggled.event(Press, 0);
        toggled.event(Release, 50);
        assert_eq!(toggled.event(Escape, 900), Some(Action::Cancel));
        assert_eq!(toggled.state(), State::Idle);
        assert_eq!(toggled.event(Press, 1_000), Some(Action::Start));
    }

    #[test]
    fn a_dictation_that_ended_on_its_own_resets() {
        let mut machine = PressMachine::default();
        machine.event(Press, 0);
        machine.event(Release, 100);
        machine.reset();
        assert!(!machine.recording());
        assert_eq!(machine.event(Press, 500), Some(Action::Start));

        // Reset while the key is held: its repeats still do not restart.
        machine.reset();
        assert_eq!(machine.event(Press, 530), None);
        assert_eq!(machine.event(Release, 600), None);
        assert_eq!(machine.event(Press, 700), Some(Action::Start));
    }

    #[test]
    fn a_release_without_a_press_does_nothing() {
        let mut machine = PressMachine::default();
        assert_eq!(machine.event(Release, 0), None);
        assert_eq!(machine.event(Press, 10), Some(Action::Start));
    }
}
