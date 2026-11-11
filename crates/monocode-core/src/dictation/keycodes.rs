//! The global shortcut as the keyboard hooks see it: the shortcut written as
//! GPUI's keystrokes ("ctrl-space"), which is how Settings › Keybindings
//! stores it, each key's code on Windows (virtual keys) and macOS (`kVK_*`),
//! and the filter both hooks run on every key: which key events are the
//! shortcut's, which to keep from the app in front, and what to report.
use super::hotkey::{KeyEvent, Modifiers, Shortcut};

/// Reads a GPUI keystroke ("ctrl-space", "cmd-shift-m", "win-h", "ctrl--")
/// as a shortcut. `None` for one with `fn`, no key, or a key the hooks do
/// not know.
pub fn from_keystroke(text: &str) -> Option<Shortcut> {
    let text = text.trim();
    // "ctrl--" names the minus key.
    let (head, minus) = match text.strip_suffix("--") {
        Some(head) => (head, true),
        None => (text, false),
    };
    let mut parts: Vec<String> = Vec::new();
    let mut key = minus.then(|| "Minus".to_owned());
    for part in head.split('-').filter(|part| !part.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => parts.push("Ctrl".into()),
            "alt" | "option" => parts.push("Alt".into()),
            "shift" => parts.push("Shift".into()),
            "cmd" | "super" | "win" | "platform" | "meta" => parts.push("Meta".into()),
            "fn" | "function" => return None,
            "+" => key = Some("Plus".into()),
            "-" => key = Some("Minus".into()),
            other => {
                if key.replace(other.to_owned()).is_some() {
                    return None;
                }
            }
        }
    }
    parts.push(key?);
    Shortcut::parse(&parts.join("+")).ok()
}

/// The shortcut as a GPUI keystroke, for binding it inside the app's own
/// windows ("ctrl-space"). `Meta` is `cmd`, which GPUI reads as the
/// platform key (⌘, or the Windows key).
pub fn to_keystroke(shortcut: &Shortcut) -> String {
    let m = shortcut.modifiers;
    let mut out = String::new();
    for (on, name) in [
        (m.ctrl, "ctrl-"),
        (m.alt, "alt-"),
        (m.meta, "cmd-"),
        (m.shift, "shift-"),
    ] {
        if on {
            out.push_str(name);
        }
    }
    out.push_str(&match shortcut.key.as_str() {
        "Plus" => "+".to_owned(),
        "Minus" => "-".to_owned(),
        key => key.to_ascii_lowercase(),
    });
    out
}

/// The Windows virtual-key code of a key named as `Shortcut::key` names it.
pub fn windows_vk(key: &str) -> Option<u16> {
    let code = match key {
        "Space" => 0x20,
        "Enter" => 0x0D,
        "Tab" => 0x09,
        "Escape" => 0x1B,
        "Backspace" => 0x08,
        "Delete" => 0x2E,
        "Insert" => 0x2D,
        "Home" => 0x24,
        "End" => 0x23,
        "PageUp" => 0x21,
        "PageDown" => 0x22,
        "Left" => 0x25,
        "Up" => 0x26,
        "Right" => 0x27,
        "Down" => 0x28,
        "Plus" | "=" => 0xBB,
        "Minus" | "-" => 0xBD,
        "," => 0xBC,
        "." => 0xBE,
        "/" => 0xBF,
        ";" => 0xBA,
        "'" => 0xDE,
        "[" => 0xDB,
        "]" => 0xDD,
        "\\" => 0xDC,
        "`" => 0xC0,
        _ => {
            if let Some(n) = function_key(key) {
                return Some(0x70 + u16::from(n) - 1);
            }
            let mut chars = key.chars();
            return match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_alphanumeric() => {
                    Some(u16::from(c.to_ascii_uppercase() as u8))
                }
                _ => None,
            };
        }
    };
    Some(code)
}

/// Escape's virtual-key code.
pub const WINDOWS_ESCAPE: u16 = 0x1B;

/// The macOS virtual key code (`kVK_*`, the ANSI layout) of a key named as
/// `Shortcut::key` names it.
pub fn mac_keycode(key: &str) -> Option<u16> {
    let code = match key {
        "A" => 0,
        "S" => 1,
        "D" => 2,
        "F" => 3,
        "H" => 4,
        "G" => 5,
        "Z" => 6,
        "X" => 7,
        "C" => 8,
        "V" => 9,
        "B" => 11,
        "Q" => 12,
        "W" => 13,
        "E" => 14,
        "R" => 15,
        "Y" => 16,
        "T" => 17,
        "1" => 18,
        "2" => 19,
        "3" => 20,
        "4" => 21,
        "6" => 22,
        "5" => 23,
        "Plus" | "=" => 24,
        "9" => 25,
        "7" => 26,
        "Minus" | "-" => 27,
        "8" => 28,
        "0" => 29,
        "]" => 30,
        "O" => 31,
        "U" => 32,
        "[" => 33,
        "I" => 34,
        "P" => 35,
        "Enter" => 36,
        "L" => 37,
        "J" => 38,
        "'" => 39,
        "K" => 40,
        ";" => 41,
        "\\" => 42,
        "," => 43,
        "/" => 44,
        "N" => 45,
        "M" => 46,
        "." => 47,
        "Tab" => 48,
        "Space" => 49,
        "`" => 50,
        "Backspace" => 51,
        "Escape" => 53,
        "Insert" => 114,
        "Home" => 115,
        "PageUp" => 116,
        "Delete" => 117,
        "End" => 119,
        "PageDown" => 121,
        "Left" => 123,
        "Right" => 124,
        "Down" => 125,
        "Up" => 126,
        "F1" => 122,
        "F2" => 120,
        "F3" => 99,
        "F4" => 118,
        "F5" => 96,
        "F6" => 97,
        "F7" => 98,
        "F8" => 100,
        "F9" => 101,
        "F10" => 109,
        "F11" => 103,
        "F12" => 111,
        "F13" => 105,
        "F14" => 107,
        "F15" => 113,
        "F16" => 106,
        "F17" => 64,
        "F18" => 79,
        "F19" => 80,
        "F20" => 90,
        _ => return None,
    };
    Some(code)
}

/// `kVK_Escape`.
pub const MAC_ESCAPE: u16 = 53;
/// `kVK_ANSI_V`, for the paste chord.
pub const MAC_V: u16 = 9;

fn function_key(key: &str) -> Option<u8> {
    let n = key.strip_prefix('F')?.parse::<u8>().ok()?;
    (1..=24).contains(&n).then_some(n)
}

/// What a hook does with one key event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Verdict {
    /// Keep the event from the app in front.
    pub swallow: bool,
    /// What to tell the dictation, if anything.
    pub report: Option<KeyEvent>,
    /// A chord with Alt or the Windows key was just taken: Windows opens
    /// the menu bar or Start when that modifier comes up alone, so the hook
    /// sends a harmless key of its own first.
    pub mask_modifier: bool,
}

/// The shortcut a hook watches for, in that system's key codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Watch {
    pub key: u16,
    pub modifiers: Modifiers,
    pub escape: u16,
}

/// The filter both hooks run on every key event. It takes the shortcut's key
/// only with exactly its modifiers held (so Ctrl+Shift+Space stays the
/// quick composer's), then every repeat and the release of that press, and
/// Escape only while a system dictation listens.
#[derive(Clone, Copy, Debug, Default)]
pub struct Filter {
    watch: Option<Watch>,
    /// The shortcut's key, while it is down as the shortcut.
    held: Option<u16>,
    /// An Escape taken from the app has not come up yet: its code.
    escape_held: Option<u16>,
}

impl Filter {
    /// Watching for nothing yet; `const`, for a hook's static.
    pub const fn new() -> Self {
        Filter {
            watch: None,
            held: None,
            escape_held: None,
        }
    }

    /// Watches for `watch`, or for nothing. A press already taken keeps its
    /// release.
    pub fn set(&mut self, watch: Option<Watch>) {
        self.watch = watch;
    }

    pub fn watching(&self) -> bool {
        self.watch.is_some()
    }

    /// One key event: its code, whether it went down, the modifiers held
    /// right now, and whether a system dictation is listening (Escape
    /// cancels it then).
    pub fn key(&mut self, code: u16, down: bool, held: Modifiers, listening: bool) -> Verdict {
        let pass = Verdict::default();
        // The press taken keeps its repeats and its release, even when the
        // shortcut changed or was turned off meanwhile.
        if self.held == Some(code) {
            if down {
                return Verdict {
                    swallow: true,
                    ..pass
                };
            }
            self.held = None;
            return Verdict {
                swallow: true,
                report: Some(KeyEvent::Release),
                ..pass
            };
        }
        // An Escape taken keeps its repeats and its release too, even when
        // the shortcut was turned off meanwhile.
        if self.escape_held == Some(code) {
            if !down {
                self.escape_held = None;
            }
            return Verdict {
                swallow: true,
                ..pass
            };
        }
        let Some(watch) = self.watch else {
            return pass;
        };
        if code == watch.key {
            if !down || held != watch.modifiers {
                return pass;
            }
            self.held = Some(code);
            return Verdict {
                swallow: true,
                report: Some(KeyEvent::Press),
                mask_modifier: watch.modifiers.alt || watch.modifiers.meta,
            };
        }
        if code == watch.escape && down && listening {
            self.escape_held = Some(code);
            return Verdict {
                swallow: true,
                report: Some(KeyEvent::Escape),
                ..pass
            };
        }
        pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl() -> Modifiers {
        Modifiers {
            ctrl: true,
            ..Modifiers::default()
        }
    }

    #[test]
    fn keystrokes_and_shortcuts_round_trip() {
        let shortcut = from_keystroke("ctrl-space").unwrap();
        assert_eq!(shortcut, Shortcut::default());
        assert_eq!(to_keystroke(&shortcut), "ctrl-space");
        let mac = from_keystroke("cmd-shift-m").unwrap();
        assert!(mac.modifiers.meta && mac.modifiers.shift);
        assert_eq!(mac.key, "M");
        assert_eq!(to_keystroke(&mac), "cmd-shift-m");
        assert_eq!(from_keystroke("win-h").unwrap().stored(), "Meta+H");
        assert_eq!(from_keystroke("alt-f13").unwrap().key, "F13");
        assert_eq!(from_keystroke("ctrl--").unwrap().key, "Minus");
        assert_eq!(to_keystroke(&from_keystroke("ctrl--").unwrap()), "ctrl--");
        assert_eq!(from_keystroke("ctrl-pageup").unwrap().key, "PageUp");
        assert_eq!(
            to_keystroke(&from_keystroke("ctrl-pageup").unwrap()),
            "ctrl-pageup"
        );
        // What a hook cannot watch.
        assert_eq!(from_keystroke(""), None);
        assert_eq!(from_keystroke("ctrl"), None);
        assert_eq!(from_keystroke("fn-f5"), None);
        assert_eq!(
            from_keystroke("space"),
            None,
            "a plain key needs a modifier"
        );
        assert_eq!(from_keystroke("ctrl-a-b"), None);
    }

    #[test]
    fn keys_have_codes_on_both_systems() {
        assert_eq!(windows_vk("Space"), Some(0x20));
        assert_eq!(windows_vk("D"), Some(u16::from(b'D')));
        assert_eq!(windows_vk("7"), Some(u16::from(b'7')));
        assert_eq!(windows_vk("F1"), Some(0x70));
        assert_eq!(windows_vk("F24"), Some(0x87));
        assert_eq!(windows_vk("Minus"), Some(0xBD));
        assert_eq!(windows_vk("Nope"), None);
        assert_eq!(mac_keycode("Space"), Some(49));
        assert_eq!(mac_keycode("V"), Some(MAC_V));
        assert_eq!(mac_keycode("Escape"), Some(MAC_ESCAPE));
        assert_eq!(mac_keycode("F5"), Some(96));
        assert_eq!(mac_keycode("F24"), None);
        // Every key a shortcut can name on Windows has a code.
        for key in [
            "Space",
            "Enter",
            "Tab",
            "Escape",
            "Backspace",
            "Delete",
            "Insert",
            "Home",
            "End",
            "PageUp",
            "PageDown",
            "Up",
            "Down",
            "Left",
            "Right",
            "Plus",
            "Minus",
            "A",
            "Z",
            "0",
            "9",
            "F12",
        ] {
            assert!(windows_vk(key).is_some(), "{key}");
            assert!(mac_keycode(key).is_some(), "{key}");
        }
    }

    fn watch() -> Filter {
        let mut filter = Filter::default();
        filter.set(Some(Watch {
            key: 0x20,
            modifiers: ctrl(),
            escape: WINDOWS_ESCAPE,
        }));
        filter
    }

    #[test]
    fn takes_the_shortcut_with_exactly_its_modifiers() {
        let mut filter = watch();
        let none = Modifiers::default();
        // Space alone and Ctrl+Shift+Space go on to the app.
        assert_eq!(filter.key(0x20, true, none, false), Verdict::default());
        assert_eq!(filter.key(0x20, false, none, false), Verdict::default());
        let shift = Modifiers {
            shift: true,
            ..ctrl()
        };
        assert_eq!(filter.key(0x20, true, shift, false), Verdict::default());
        assert_eq!(filter.key(0x20, false, shift, false), Verdict::default());
        // Ctrl+Space is ours, down, repeats and up.
        let press = filter.key(0x20, true, ctrl(), false);
        assert!(press.swallow && !press.mask_modifier);
        assert_eq!(press.report, Some(KeyEvent::Press));
        let repeat = filter.key(0x20, true, ctrl(), false);
        assert!(repeat.swallow && repeat.report.is_none());
        // Ctrl let go first: the key's release is still ours.
        let release = filter.key(0x20, false, none, false);
        assert!(release.swallow);
        assert_eq!(release.report, Some(KeyEvent::Release));
        // Other keys pass.
        assert_eq!(filter.key(0x41, true, ctrl(), true), Verdict::default());
    }

    #[test]
    fn escape_is_taken_only_while_listening() {
        let mut filter = watch();
        let none = Modifiers::default();
        assert_eq!(
            filter.key(WINDOWS_ESCAPE, true, none, false),
            Verdict::default()
        );
        assert_eq!(
            filter.key(WINDOWS_ESCAPE, false, none, false),
            Verdict::default()
        );
        let cancel = filter.key(WINDOWS_ESCAPE, true, none, true);
        assert!(cancel.swallow);
        assert_eq!(cancel.report, Some(KeyEvent::Escape));
        // Its repeats and its release, even after listening stopped.
        assert_eq!(filter.key(WINDOWS_ESCAPE, true, none, false).report, None);
        assert!(filter.key(WINDOWS_ESCAPE, false, none, false).swallow);
        assert_eq!(
            filter.key(WINDOWS_ESCAPE, false, none, false),
            Verdict::default()
        );
        // Turned off while Escape is down: its release is still taken, and
        // the next Escape, with nothing listening, reaches the app.
        assert!(filter.key(WINDOWS_ESCAPE, true, none, true).swallow);
        filter.set(None);
        assert!(filter.key(WINDOWS_ESCAPE, false, none, false).swallow);
        assert_eq!(
            filter.key(WINDOWS_ESCAPE, true, none, false),
            Verdict::default()
        );
    }

    #[test]
    fn alt_and_windows_chords_mask_their_modifier() {
        let mut filter = Filter::default();
        let alt = Modifiers {
            alt: true,
            ..Modifiers::default()
        };
        filter.set(Some(Watch {
            key: 0x44,
            modifiers: alt,
            escape: WINDOWS_ESCAPE,
        }));
        assert!(filter.key(0x44, true, alt, false).mask_modifier);
        // Turned off mid-press: the release is still taken, then nothing.
        filter.set(None);
        assert!(!filter.watching());
        let release = filter.key(0x44, false, alt, false);
        assert!(release.swallow);
        assert_eq!(release.report, Some(KeyEvent::Release));
        assert_eq!(filter.key(0x44, true, alt, false), Verdict::default());
    }
}
