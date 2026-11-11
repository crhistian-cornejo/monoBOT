//! What sits beside monoBOT's icon in the menu bar (the tray on Windows),
//! the way a Live Activity sits there: one thing at a time, compact, as part
//! of the icon. A provider's usage is its logo and up to two rows, one per
//! window ("5h 72%", "wk 40%"); the agents, what plays and the system's load
//! read the same way. The user picks it from the icon's menu or Settings;
//! while it has nothing to say, the icon stands alone.
use crate::provider::Kind;

/// Every choice, in the order the menu lists them; "" shows nothing.
pub const CHOICES: [&str; 9] = [
    "",
    "usage:claude",
    "usage:codex",
    "usage:devin",
    "usage:opencode",
    "usage:kimi",
    "agents",
    "music",
    "system",
];
/// What a new install shows.
pub const DEFAULT: &str = "usage:claude";
/// The longest a row gets before it is cut with "…".
const ROW_CHARS: usize = 14;

/// The provider a choice reports the usage of.
pub fn usage_kind(id: &str) -> Option<Kind> {
    Kind::parse(id.strip_prefix("usage:")?)
}

/// The provider's short name, for the menu.
fn short(kind: Kind) -> &'static str {
    match kind {
        Kind::Claude => "Claude",
        Kind::OpenCode => "OpenCode",
        Kind::Kimi => "Kimi",
        other => other.label(),
    }
}

/// What the menu and Settings call a choice.
pub fn name(id: &str) -> String {
    match id {
        "" => "Nothing".into(),
        "agents" => "Agents at work".into(),
        "music" => "Now playing".into(),
        "system" => "Processor and memory".into(),
        _ => match usage_kind(id) {
            Some(kind) => format!("{} usage", short(kind)),
            None => id.into(),
        },
    }
}

/// The saved choice if it is one there is, else nothing.
pub fn normalized(saved: &str) -> &'static str {
    CHOICES
        .into_iter()
        .find(|choice| *choice == saved)
        .unwrap_or("")
}

/// The agents' sessions, as the notch's wings see them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Agents {
    pub working: usize,
    pub asking: usize,
    /// What the lead does, in one word ("Editing").
    pub word: Option<String>,
    /// The lead's steps done and in all.
    pub steps: Option<(u32, u32)>,
}

/// A provider's plan: its two shortest windows that still run.
#[derive(Clone, Debug, PartialEq)]
pub struct Usage {
    pub kind: Kind,
    /// Each window's label ("5h", "wk") and how much is used, 0 to 100.
    pub windows: Vec<(String, f64)>,
    /// When the fullest window resets ("Resets in 2h 10m").
    pub resets: Option<String>,
}

impl Usage {
    pub fn of(kind: Kind, snapshot: &crate::usage::Snapshot, now: i64) -> Option<Self> {
        let mut running: Vec<_> = snapshot
            .windows
            .iter()
            .filter(|w| w.resets_at.is_none_or(|at| at > now))
            .collect();
        if running.is_empty() {
            return None;
        }
        let fullest = running
            .iter()
            .max_by(|a, b| a.used_percent.total_cmp(&b.used_percent))
            .copied()?;
        let resets = fullest
            .resets_at
            .is_some()
            .then(|| fullest.reset_label(now));
        running.sort_by_key(|w| w.minutes);
        let mut windows: Vec<(String, f64)> = running
            .iter()
            .take(2)
            .map(|w| (w.label(), w.used_percent.clamp(0., 100.)))
            .collect();
        // A model's own weekly limit (Fable) shows by its name. With three to
        // say and two rows, the shortest window stays and the fuller of the
        // other two takes the second row.
        windows.extend(
            snapshot
                .models
                .iter()
                .filter(|m| m.window.resets_at.is_none_or(|at| at > now))
                .map(|m| (m.model.clone(), m.window.used_percent.clamp(0., 100.))),
        );
        if windows.len() > 2 {
            let first = windows.remove(0);
            let fuller = windows
                .into_iter()
                .reduce(|a, b| if b.1 > a.1 { b } else { a })?;
            windows = vec![first, fuller];
        }
        Some(Self {
            kind,
            windows,
            resets,
        })
    }

    fn fullest(&self) -> f64 {
        self.windows
            .iter()
            .map(|(_, used)| *used)
            .fold(0., f64::max)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Music {
    pub title: String,
    pub artist: String,
    pub playing: bool,
}

/// The processor's load and the memory in use, 0 to 100.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct System {
    pub cpu: Option<f32>,
    pub memory: Option<f32>,
}

/// Everything an item is made from, at one moment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Now {
    pub agents: Agents,
    pub music: Option<Music>,
    pub usage: Vec<Usage>,
    pub system: System,
    /// Usage says what is left instead of what is used (the footer's
    /// choice).
    pub remaining: bool,
}

/// The small picture before the rows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mark {
    /// The provider's logo.
    Logo(Kind),
    /// A ring filled clockwise from the top, 0 to 1.
    Ring(f32),
    /// The work mark: three dots.
    Dots,
    /// A note, for what plays.
    Note,
}

/// What sits beside the icon.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    pub mark: Mark,
    /// One or two short rows beside the mark.
    pub rows: Vec<String>,
    /// How full it is, 0 to 1, and the number for it: the Windows tray has
    /// no room for rows, so it draws a ring with the number inside.
    pub fill: Option<f32>,
    pub figure: Option<u32>,
    /// What the pointer shows over it.
    pub tooltip: String,
    /// It needs the user (an approval, a plan near its limit).
    pub urgent: bool,
}

fn clip(text: &str, chars: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= chars {
        return text.into();
    }
    let cut: String = text.chars().take(chars.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

fn agents(now: &Agents) -> Option<Item> {
    if now.asking > 0 {
        return Some(Item {
            id: "agents".into(),
            mark: Mark::Dots,
            rows: vec!["Needs".into(), "you".into()],
            fill: None,
            figure: Some(now.asking as u32),
            tooltip: if now.asking == 1 {
                "An agent waits for your answer".into()
            } else {
                format!("{} agents wait for your answer", now.asking)
            },
            urgent: true,
        });
    }
    if now.working == 0 {
        return None;
    }
    let word = now.word.clone().unwrap_or_else(|| "Working".into());
    let (second, fill) = match now.steps {
        Some((done, all)) if all > 0 && now.working == 1 => (
            format!("{done}/{all}"),
            Some((done as f32 / all as f32).clamp(0., 1.)),
        ),
        _ => (format!("{} at work", now.working), None),
    };
    Some(Item {
        id: "agents".into(),
        mark: fill.map_or(Mark::Dots, Mark::Ring),
        tooltip: format!("{}: {}", second, word.to_lowercase()),
        rows: vec![word, second],
        fill,
        figure: Some(now.working as u32),
        urgent: false,
    })
}

fn usage(id: &str, kind: Kind, now: &Now) -> Option<Item> {
    let usage = now.usage.iter().find(|usage| usage.kind == kind)?;
    if usage.windows.is_empty() {
        return None;
    }
    let shown = |used: f64| if now.remaining { 100. - used } else { used };
    let rows = usage
        .windows
        .iter()
        .map(|(label, used)| format!("{label} {}%", shown(*used).round()))
        .collect();
    let fullest = usage.fullest();
    let resets = usage
        .resets
        .as_ref()
        .map(|resets| format!(" · {resets}"))
        .unwrap_or_default();
    let windows = usage
        .windows
        .iter()
        .map(|(label, used)| format!("{label} {}% used", used.round()))
        .collect::<Vec<_>>()
        .join(", ");
    Some(Item {
        id: id.into(),
        mark: Mark::Logo(kind),
        rows,
        fill: Some((shown(fullest) / 100.) as f32),
        figure: Some(shown(fullest).round() as u32),
        tooltip: format!("{}: {windows}{resets}", kind.label()),
        urgent: fullest >= 90.,
    })
}

fn music(now: &Now) -> Option<Item> {
    let music = now.music.as_ref().filter(|music| music.playing)?;
    if music.title.trim().is_empty() {
        return None;
    }
    let mut rows = vec![clip(&music.title, ROW_CHARS)];
    if !music.artist.trim().is_empty() {
        rows.push(clip(&music.artist, ROW_CHARS));
    }
    Some(Item {
        id: "music".into(),
        mark: Mark::Note,
        rows,
        fill: None,
        figure: None,
        tooltip: match music.artist.trim() {
            "" => music.title.trim().into(),
            artist => format!("{} — {artist}", music.title.trim()),
        },
        urgent: false,
    })
}

fn system(now: &Now) -> Option<Item> {
    let cpu = now.system.cpu?;
    let percent = |value: f32| value.round().clamp(0., 100.) as u32;
    let mut rows = vec![format!("CPU {}%", percent(cpu))];
    if let Some(memory) = now.system.memory {
        rows.push(format!("MEM {}%", percent(memory)));
    }
    Some(Item {
        id: "system".into(),
        mark: Mark::Ring(cpu / 100.),
        tooltip: rows.join(", "),
        rows,
        fill: Some(cpu / 100.),
        figure: Some(percent(cpu)),
        urgent: false,
    })
}

/// What sits beside the icon now, if the choice has something to say.
pub fn item(choice: &str, now: &Now) -> Option<Item> {
    match normalized(choice) {
        "" => None,
        "agents" => agents(&now.agents),
        "music" => music(now),
        "system" => system(now),
        id => usage(id, usage_kind(id)?, now),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{Snapshot, Window};

    fn claude(windows: &[(&str, f64)]) -> Usage {
        Usage {
            kind: Kind::Claude,
            windows: windows.iter().map(|(l, u)| (l.to_string(), *u)).collect(),
            resets: Some("Resets in 2h".into()),
        }
    }

    #[test]
    fn a_choice_is_one_there_is_or_nothing() {
        assert_eq!(normalized("music"), "music");
        assert_eq!(normalized("usage:cursor"), "");
        assert_eq!(normalized("nope"), "");
        assert_eq!(name(""), "Nothing");
        assert_eq!(name("usage:claude"), "Claude usage");
        assert!(CHOICES.contains(&DEFAULT));
    }

    #[test]
    fn usage_is_the_logo_and_a_row_per_window() {
        let mut now = Now {
            usage: vec![claude(&[("5h", 72.4), ("wk", 40.)])],
            ..Default::default()
        };
        let item = item("usage:claude", &now).unwrap();
        assert_eq!(item.mark, Mark::Logo(Kind::Claude));
        assert_eq!(item.rows, ["5h 72%", "wk 40%"]);
        assert_eq!(item.figure, Some(72));
        assert_eq!(
            item.tooltip,
            "Claude Code: 5h 72% used, wk 40% used · Resets in 2h"
        );
        assert!(!item.urgent);
        now.remaining = true;
        now.usage = vec![claude(&[("5h", 93.)])];
        let item = super::item("usage:claude", &now).unwrap();
        assert_eq!(item.rows, ["5h 7%"]);
        assert!(item.urgent, "a plan near its limit needs the user");
        assert!(super::item("usage:codex", &now).is_none());
    }

    #[test]
    fn the_rest_read_as_two_short_rows() {
        let now = Now {
            agents: Agents {
                working: 1,
                asking: 0,
                word: Some("Editing".into()),
                steps: Some((3, 4)),
            },
            music: Some(Music {
                title: "A song whose title goes on".into(),
                artist: "Someone".into(),
                playing: true,
            }),
            system: System {
                cpu: Some(31.4),
                memory: Some(61.),
            },
            ..Default::default()
        };
        let agents = item("agents", &now).unwrap();
        assert_eq!(agents.rows, ["Editing", "3/4"]);
        assert_eq!(agents.mark, Mark::Ring(0.75));
        assert_eq!(
            item("music", &now).unwrap().rows,
            ["A song whose…", "Someone"]
        );
        assert_eq!(item("system", &now).unwrap().rows, ["CPU 31%", "MEM 61%"]);
        assert!(item("", &now).is_none());

        let asking = Now {
            agents: Agents {
                working: 2,
                asking: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let item = item("agents", &asking).unwrap();
        assert_eq!(item.rows, ["Needs", "you"]);
        assert!(item.urgent);
    }

    #[test]
    fn nothing_to_say_leaves_the_icon_alone() {
        let now = Now {
            music: Some(Music {
                title: "Song".into(),
                artist: String::new(),
                playing: false,
            }),
            ..Default::default()
        };
        for choice in CHOICES {
            assert!(item(choice, &now).is_none(), "{choice}");
        }
    }

    #[test]
    fn a_models_own_limit_shows_by_its_name() {
        let window = |used_percent, minutes| Window {
            used_percent,
            minutes,
            resets_at: Some(1_000_000),
        };
        let fable = |used| crate::usage::ModelWindow {
            model: "Fable".into(),
            window: window(used, 10_080),
        };
        // The weekly limit and Fable's: "wk 52%" over "Fable 36%".
        let snapshot = Snapshot {
            windows: vec![window(52., 10_080)],
            models: vec![fable(36.)],
            ..Default::default()
        };
        let usage = Usage::of(Kind::Claude, &snapshot, 1_000).unwrap();
        assert_eq!(
            usage.windows,
            [("wk".to_string(), 52.), ("Fable".to_string(), 36.)]
        );
        let now = Now {
            usage: vec![usage],
            ..Default::default()
        };
        let item = item("usage:claude", &now).unwrap();
        assert_eq!(item.rows, ["wk 52%", "Fable 36%"]);
        assert!(item
            .tooltip
            .starts_with("Claude Code: wk 52% used, Fable 36% used"));
        // Three to say: the 5-hour window stays, the fuller of the rest follows.
        let three = Snapshot {
            windows: vec![window(20., 300), window(52., 10_080)],
            models: vec![fable(90.)],
            ..Default::default()
        };
        let usage = Usage::of(Kind::Claude, &three, 1_000).unwrap();
        assert_eq!(
            usage.windows,
            [("5h".to_string(), 20.), ("Fable".to_string(), 90.)]
        );
    }

    #[test]
    fn usage_keeps_the_two_shortest_windows_that_still_run() {
        let window = |used_percent, minutes, resets_at| Window {
            used_percent,
            minutes,
            resets_at,
        };
        let snapshot = Snapshot {
            windows: vec![
                window(80., 10_080, Some(1_000_000)),
                window(40., 300, Some(10_000)),
                window(100., 1_440, Some(500)),
            ],
            ..Default::default()
        };
        let usage = Usage::of(Kind::Codex, &snapshot, 1_000).unwrap();
        assert_eq!(
            usage.windows,
            [("5h".to_string(), 40.), ("wk".to_string(), 80.)]
        );
        assert!(Usage::of(Kind::Codex, &Snapshot::default(), 0).is_none());
    }
}
