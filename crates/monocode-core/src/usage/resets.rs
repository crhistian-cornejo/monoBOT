//! When to use a limit reset, and which. An account near a limit (98% by
//! default) that holds a reset for that limit is offered it; at the limit
//! the reset is used, after the user said yes or by itself when they chose
//! Automatic, and the agent goes on. It is never spent below the limit: the
//! last few percent are still the user's, and Claude's grants refuse a use
//! before the limit anyway.
//!
//! Of the resets that refill the limit in question, the one made for it
//! goes first (the 5-hour reset for the 5-hour limit, the weekly one for the
//! weekly limit, the Fable reset for Fable's weekly limit), then the
//! narrowest (5-hour, weekly, Fable, full), then the one that expires first.
use super::{ResetCredit, ResetCredits, Snapshot};
use serde::{Deserialize, Serialize};

/// What to do with a reset when a limit comes near.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Never offer or use one.
    Off,
    /// Ask near the limit; use it at the limit only after a yes.
    #[default]
    Ask,
    /// Use it at the limit without asking.
    Automatic,
}

impl Mode {
    pub fn from_id(id: &str) -> Self {
        match id {
            "off" => Self::Off,
            "automatic" => Self::Automatic,
            _ => Self::Ask,
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Ask => "ask",
            Self::Automatic => "automatic",
        }
    }
}

/// The percent of a limit where a reset is first offered.
pub const THRESHOLD: f64 = 98.;

/// A reset worth using for a limit that is near or reached.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub credit: ResetCredit,
    /// The limit it is for, by the provider's key.
    pub limit: String,
    /// How much of that limit is used.
    pub percent: f64,
}

impl Plan {
    /// Whether the limit is reached, so the reset can be used now.
    pub fn reached(&self) -> bool {
        self.percent >= 100.
    }
}

/// The limits at or past `threshold`, the fullest first: what the provider
/// says of each limit by name (Claude, Fable's weekly limit among them),
/// then the 5-hour and weekly windows. A window whose reset time has passed
/// is empty again.
pub fn pressing(snapshot: &Snapshot, threshold: f64, now: i64) -> Vec<(String, f64)> {
    let mut found: Vec<(String, f64)> = Vec::new();
    let mut note = |key: String, percent: f64| match found.iter_mut().find(|(k, _)| *k == key) {
        Some((_, kept)) => *kept = kept.max(percent),
        None => found.push((key, percent)),
    };
    if let Some(resets) = &snapshot.reset_credits {
        for (key, percent) in &resets.used {
            note(key.clone(), *percent);
        }
        for key in &resets.exhausted {
            note(key.clone(), 100.);
        }
    }
    for window in &snapshot.windows {
        if window.resets_at.is_some_and(|at| at <= now) {
            continue;
        }
        let key = match window.minutes {
            300 => "five_hour".to_owned(),
            10_080 => "seven_day".to_owned(),
            minutes => format!("window_{minutes}"),
        };
        note(key, window.used_percent.clamp(0., 100.));
    }
    found.retain(|(_, percent)| *percent >= threshold);
    found.sort_by(|a, b| b.1.total_cmp(&a.1));
    found
}

/// The kind of a reset: `session`, `weekly`, `fable`, `full`, or `any` for
/// one that refills every limit (Codex).
pub fn kind(credit: &ResetCredit) -> &str {
    match credit.reset_type.strip_prefix("claude:") {
        Some(kind) => kind,
        None => "any",
    }
}

/// The reset made for the limit `key`, if any kind is.
fn made_for(key: &str) -> Option<&'static str> {
    match key {
        "five_hour" => Some("session"),
        "seven_day" => Some("weekly"),
        "seven_day_overage_included" => Some("fable"),
        _ => None,
    }
}

fn usable(credit: &ResetCredit) -> bool {
    matches!(credit.status.as_str(), "available" | "unknown") && credit.left != Some(0)
}

/// Whether `credit` refills the limit `key`. The Fable reset gives back what
/// Fable used of the weekly limit, so it counts there too, as a last choice.
fn refills(credit: &ResetCredit, key: &str) -> bool {
    credit.clears.is_empty()
        || credit.clears.iter().any(|cleared| cleared == key)
        || (key == "seven_day" && kind(credit) == "fable")
}

/// The reset to use for the limit `key`, as ranked above.
pub fn pick<'a>(resets: &'a ResetCredits, key: &str) -> Option<&'a ResetCredit> {
    const ORDER: [&str; 5] = ["session", "weekly", "fable", "full", "any"];
    resets
        .credits
        .iter()
        .filter(|credit| usable(credit) && refills(credit, key))
        .min_by_key(|credit| {
            let kind = kind(credit);
            let rank = if made_for(key) == Some(kind) {
                0
            } else if key == "seven_day" && kind == "fable" {
                ORDER.len() + 1
            } else {
                1 + ORDER.iter().position(|k| *k == kind).unwrap_or(ORDER.len())
            };
            (rank, credit.expires_at.unwrap_or(i64::MAX))
        })
}

/// The reset worth offering or using now, if a limit is at or past
/// `threshold` and a reset refills it. The fullest such limit wins.
pub fn plan(snapshot: &Snapshot, threshold: f64, now: i64) -> Option<Plan> {
    let resets = snapshot.reset_credits.as_ref()?;
    if resets.available_count == 0 {
        return None;
    }
    pressing(snapshot, threshold, now)
        .into_iter()
        .find_map(|(limit, percent)| {
            pick(resets, &limit).map(|credit| Plan {
                credit: credit.clone(),
                limit,
                percent,
            })
        })
}

/// Whether to use the planned reset now: only at the limit, and only when
/// the user chose Automatic or said yes to it.
pub fn use_now(mode: Mode, plan: &Plan, approved: bool) -> bool {
    plan.reached()
        && match mode {
            Mode::Off => false,
            Mode::Ask => approved,
            Mode::Automatic => true,
        }
}

/// A limit's name for people.
pub fn limit_label(key: &str) -> String {
    match key {
        "five_hour" => "5-hour limit".into(),
        "seven_day" => "weekly limit".into(),
        "seven_day_overage_included" => "Fable weekly limit".into(),
        "seven_day_opus" => "Opus weekly limit".into(),
        "seven_day_sonnet" => "Sonnet weekly limit".into(),
        other => match other.strip_prefix("window_") {
            Some(minutes) => format!("{minutes}-minute limit"),
            None => other.replace('_', " "),
        },
    }
}

/// A reset's name for people, to sit inside a sentence: "5-hour reset",
/// "weekly reset", "Fable reset"…
pub fn reset_label(credit: &ResetCredit) -> String {
    if let Some(title) = credit.title.as_deref().filter(|t| !t.trim().is_empty()) {
        return title.trim().to_owned();
    }
    match kind(credit) {
        "session" => "5-hour reset",
        "weekly" => "weekly reset",
        "fable" => "Fable reset",
        "full" => "full reset",
        _ => "banked reset",
    }
    .into()
}

/// How many uses of resets of `credit`'s kind the account holds.
pub fn left_of_kind(resets: &ResetCredits, credit: &ResetCredit) -> u64 {
    resets
        .credits
        .iter()
        .filter(|other| usable(other) && kind(other) == kind(credit))
        .map(|other| other.left.unwrap_or(1))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::Window;

    fn credit(id: &str, kind: &str, clears: &[&str], expires: Option<i64>) -> ResetCredit {
        ResetCredit {
            id: id.into(),
            status: "available".into(),
            reset_type: format!("claude:{kind}"),
            expires_at: expires,
            clears: clears.iter().map(|k| (*k).into()).collect(),
            left: Some(1),
            needs_limit: true,
            ..Default::default()
        }
    }

    fn snapshot(windows: &[(u64, f64)], resets: ResetCredits) -> Snapshot {
        Snapshot {
            windows: windows
                .iter()
                .map(|(minutes, used)| Window {
                    used_percent: *used,
                    minutes: *minutes,
                    resets_at: None,
                })
                .collect(),
            reset_credits: Some(resets),
            ..Default::default()
        }
    }

    fn holding(credits: Vec<ResetCredit>) -> ResetCredits {
        ResetCredits {
            available_count: credits.len() as u64,
            credits,
            ..Default::default()
        }
    }

    #[test]
    fn the_reset_made_for_the_limit_goes_first_then_the_narrowest_then_the_soonest() {
        let resets = holding(vec![
            credit("full", "full", &["five_hour", "seven_day"], Some(1)),
            credit("late", "session", &["five_hour"], Some(900)),
            credit("soon", "session", &["five_hour"], Some(500)),
            credit("week", "weekly", &["seven_day"], None),
            credit("fable", "fable", &["seven_day_overage_included"], None),
        ]);
        assert_eq!(pick(&resets, "five_hour").unwrap().id, "soon");
        assert_eq!(pick(&resets, "seven_day").unwrap().id, "week");
        assert_eq!(
            pick(&resets, "seven_day_overage_included").unwrap().id,
            "fable"
        );
        // Without the weekly reset, the full one refills the weekly limit
        // before the Fable reset, which only gives back what Fable used.
        let resets = holding(vec![
            credit("fable", "fable", &["seven_day_overage_included"], None),
            credit("full", "full", &["five_hour", "seven_day"], None),
        ]);
        assert_eq!(pick(&resets, "seven_day").unwrap().id, "full");
        let only_fable = holding(vec![credit(
            "fable",
            "fable",
            &["seven_day_overage_included"],
            None,
        )]);
        assert_eq!(pick(&only_fable, "seven_day").unwrap().id, "fable");
        // Nothing refills Opus's limit here; a used-up or paused one never counts.
        assert!(pick(&resets, "seven_day_opus").is_none());
        let mut spent = credit("soon", "session", &["five_hour"], None);
        spent.left = Some(0);
        let mut paused = credit("p", "session", &["five_hour"], None);
        paused.status = "paused".into();
        assert!(pick(&holding(vec![spent, paused]), "five_hour").is_none());
        // Codex's banked reset refills every limit.
        let codex = ResetCredits {
            available_count: 1,
            credits: vec![ResetCredit {
                id: "c1".into(),
                status: "available".into(),
                reset_type: "codexRateLimits".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(pick(&codex, "seven_day").unwrap().id, "c1");
    }

    #[test]
    fn a_reset_is_offered_near_the_limit_and_used_only_at_it() {
        let resets = holding(vec![credit("s", "session", &["five_hour"], None)]);
        // 60%: nothing to offer.
        assert!(plan(&snapshot(&[(300, 60.)], resets.clone()), THRESHOLD, 0).is_none());
        // 98%: offered, not used yet, even when automatic.
        let near = plan(&snapshot(&[(300, 98.)], resets.clone()), THRESHOLD, 0).unwrap();
        assert_eq!(
            (near.limit.as_str(), near.credit.id.as_str()),
            ("five_hour", "s")
        );
        assert!(!near.reached());
        assert!(!use_now(Mode::Automatic, &near, true));
        // At the limit: automatic uses it, ask only after a yes, off never.
        let full = plan(&snapshot(&[(300, 100.)], resets.clone()), THRESHOLD, 0).unwrap();
        assert!(use_now(Mode::Automatic, &full, false));
        assert!(!use_now(Mode::Ask, &full, false));
        assert!(use_now(Mode::Ask, &full, true));
        assert!(!use_now(Mode::Off, &full, true));
        // A window whose reset time passed is empty again.
        let mut past = snapshot(&[(300, 100.)], resets);
        past.windows[0].resets_at = Some(10);
        assert!(plan(&past, THRESHOLD, 20).is_none());
    }

    #[test]
    fn claudes_own_numbers_name_fables_limit_and_what_is_reached() {
        let mut resets = holding(vec![
            credit("s", "session", &["five_hour"], None),
            credit("f", "fable", &["seven_day_overage_included"], None),
        ]);
        resets.used.insert("seven_day_overage_included".into(), 99.);
        // The weekly window is fine; Fable's weekly limit is at 99%.
        let near = plan(
            &snapshot(&[(300, 40.), (10_080, 70.)], resets.clone()),
            THRESHOLD,
            0,
        );
        assert_eq!(near.unwrap().credit.id, "f");
        // Reached limits come first.
        resets.exhausted.push("five_hour".into());
        let reached = plan(&snapshot(&[(300, 40.)], resets), THRESHOLD, 0).unwrap();
        assert_eq!((reached.credit.id.as_str(), reached.percent), ("s", 100.));
        // An account with no uses left is never asked about.
        let mut none = holding(vec![credit("s", "session", &["five_hour"], None)]);
        none.available_count = 0;
        assert!(plan(&snapshot(&[(300, 100.)], none), THRESHOLD, 0).is_none());
    }

    #[test]
    fn names_for_people() {
        assert_eq!(limit_label("five_hour"), "5-hour limit");
        assert_eq!(
            limit_label("seven_day_overage_included"),
            "Fable weekly limit"
        );
        assert_eq!(limit_label("window_43200"), "43200-minute limit");
        let fable = credit("f", "fable", &[], None);
        assert_eq!(reset_label(&fable), "Fable reset");
        let titled = ResetCredit {
            title: Some(" Launch week reset ".into()),
            ..fable.clone()
        };
        assert_eq!(reset_label(&titled), "Launch week reset");
        let resets = holding(vec![
            fable.clone(),
            fable.clone(),
            credit("s", "session", &[], None),
        ]);
        assert_eq!(left_of_kind(&resets, &fable), 2);
        assert_eq!(Mode::from_id("automatic"), Mode::Automatic);
        assert_eq!(Mode::from_id("nonsense"), Mode::Ask);
        assert_eq!(Mode::Off.id(), "off");
    }
}
