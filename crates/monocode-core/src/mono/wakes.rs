//! Wakes: a habit the agent sets for itself that runs once, at a time it
//! chose, and is then removed (`app wakes.add`). Ports `newWake` and the
//! limits of `monoHabits.ts`, and `wakeTime` and the `wakes.add` case of
//! `agentApp.ts`. A wake is stored in `habits.json` like any habit, with
//! `"once": true`.
use super::habits::{self, Habit, Schedule};
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc};
use serde_json::Value;

/// Wakes an agent may have waiting.
pub const WAKES_MAX: usize = 3;
/// How soon a wake may be, and how far ahead.
pub const MIN_AHEAD_MS: u64 = 5 * 60 * 1000;
pub const MAX_AHEAD_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// Longest note, in characters (`requiredString(input.note, "note", 2_000)`).
pub const NOTE_MAX: usize = 2_000;
const NAME_NOTE_CHARS: usize = 60;

/// A wake: the agent's note to itself, run once at `at`.
pub fn new_wake(note: &str, at_ms: u64, now_ms: u64) -> Habit {
    let name = if note.chars().count() > NAME_NOTE_CHARS {
        format!(
            "{}…",
            note.chars().take(NAME_NOTE_CHARS - 1).collect::<String>()
        )
    } else {
        note.to_string()
    };
    let local = DateTime::from_timestamp_millis(at_ms as i64)
        .unwrap_or_default()
        .with_timezone(&Local);
    Habit {
        id: super::new_id(),
        name: format!("Wake: {name}"),
        instructions: note.into(),
        // Unused for a wake, but a valid schedule keeps habits.json uniform.
        schedule: Schedule {
            schedule_kind: "daily".into(),
            minute: local.minute(),
            time: local.format("%H:%M").to_string(),
            day_of_week: chrono::Datelike::weekday(&local).num_days_from_sunday(),
        },
        enabled: true,
        created_at: now_ms,
        next_run_at: at_ms,
        last_run_at: None,
        last_outcome: None,
        last_error: None,
        runs: Vec::new(),
        run_requested: false,
        running_since: None,
        from_card: None,
        once: true,
    }
}

/// The note of a `wakes.add` call: a non-empty string, trimmed.
pub fn wake_note(value: Option<&Value>) -> Result<String, String> {
    match value {
        Some(Value::String(s)) if !s.trim().is_empty() && s.chars().count() <= NOTE_MAX => {
            Ok(s.trim().to_string())
        }
        _ => Err(format!(
            "note must be a non-empty string under {NOTE_MAX} characters"
        )),
    }
}

/// When a wake runs: `in` minutes from now, or at an `at` date and time
/// (`wakeTime`). Errors carry the texts the agent is shown.
pub fn wake_time(input: &Value, now_ms: u64) -> Result<u64, String> {
    let now = now_ms as f64;
    let at = if let Some(minutes) = input.get("in") {
        let minutes = minutes
            .as_f64()
            .filter(|m| m.is_finite())
            .ok_or("in must be a number of minutes")?;
        now + minutes * 60_000.0
    } else if let Some(text) = input
        .get("at")
        .and_then(Value::as_str)
        .filter(|t| !t.trim().is_empty())
    {
        parse_date(text.trim()).ok_or(r#"at must be a date and time like "2026-11-05T09:30""#)?
            as f64
    } else {
        return Err(r#"Give "in" (minutes) or "at" (date and time)"#.into());
    };
    if at - now < MIN_AHEAD_MS as f64 {
        return Err("A wake must be at least 5 minutes ahead".into());
    }
    if at - now > MAX_AHEAD_MS as f64 {
        return Err("A wake can be at most 7 days ahead; use a habit instead".into());
    }
    Ok(at.round() as u64)
}

/// A date and time as JavaScript's `new Date(text)` reads the usual forms:
/// with an offset or `Z` it is exact, without one it is local time, and a
/// bare date is midnight UTC.
fn parse_date(text: &str) -> Option<i64> {
    let zoned = text
        .strip_suffix('Z')
        .or_else(|| text.strip_suffix('z'))
        .map(|rest| format!("{rest}+00:00"))
        .unwrap_or_else(|| text.to_string());
    if let Ok(at) = DateTime::parse_from_rfc3339(&zoned) {
        return Some(at.timestamp_millis());
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f%:z",
        "%Y-%m-%dT%H:%M%:z",
        "%Y-%m-%d %H:%M:%S%.f%:z",
        "%Y-%m-%d %H:%M%:z",
    ] {
        if let Ok(at) = DateTime::parse_from_str(&zoned, format) {
            return Some(at.timestamp_millis());
        }
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(text, format) {
            return Local
                .from_local_datetime(&naive)
                .earliest()
                .map(|at| at.timestamp_millis());
        }
    }
    let day = NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()?;
    Some(
        Utc.from_utc_datetime(&day.and_hms_opt(0, 0, 0)?)
            .timestamp_millis(),
    )
}

/// Adds a wake to the Mono's habits (the `wakes.add` rules). Call it inside
/// `habits::update`.
pub fn add_wake(list: &mut Vec<Habit>, wake: Habit) -> Result<(), String> {
    if list.iter().filter(|h| h.once && h.enabled).count() >= WAKES_MAX {
        return Err(format!(
            "At most {WAKES_MAX} wakes can wait; run habits.list and remove one"
        ));
    }
    if list.len() >= habits::MAX {
        return Err(format!("A Mono can have at most {} habits", habits::MAX));
    }
    list.push(wake);
    Ok(())
}

/// Removes wakes that already ran (one interrupted by the app closing keeps
/// its entry, since only a recorded outcome removes it). Returns how many.
pub fn drop_spent_wakes(list: &mut Vec<Habit>) -> usize {
    let before = list.len();
    list.retain(|h| !(h.once && h.last_run_at.is_some() && h.running_since.is_none()));
    before - list.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mono::outcomes::{record_run, RunResult};
    use serde_json::json;

    const NOW: u64 = 1_793_793_600_000; // 2026-11-04T12:00:00Z

    #[test]
    fn takes_minutes_from_now_or_a_date_and_time() {
        assert_eq!(
            Utc.with_ymd_and_hms(2026, 11, 4, 12, 0, 0)
                .unwrap()
                .timestamp_millis() as u64,
            NOW
        );
        let tomorrow = Utc
            .with_ymd_and_hms(2026, 11, 5, 9, 30, 0)
            .unwrap()
            .timestamp_millis() as u64;
        assert_eq!(wake_time(&json!({"in": 30}), NOW), Ok(NOW + 30 * 60_000));
        assert_eq!(
            wake_time(&json!({"at": "2026-11-05T09:30:00Z"}), NOW),
            Ok(tomorrow)
        );
        // Without seconds, with an offset, and as plain local time.
        assert_eq!(
            wake_time(&json!({"at": "2026-11-05T09:30Z"}), NOW),
            Ok(tomorrow)
        );
        assert_eq!(
            wake_time(&json!({"at": "2026-11-05T04:30:00-05:00"}), NOW),
            Ok(tomorrow)
        );
        let local = Local
            .with_ymd_and_hms(2026, 11, 5, 9, 30, 0)
            .earliest()
            .unwrap()
            .timestamp_millis() as u64;
        assert_eq!(
            wake_time(&json!({"at": "2026-11-05T09:30"}), NOW),
            Ok(local)
        );
    }

    #[test]
    fn keeps_wakes_between_five_minutes_and_seven_days_ahead() {
        let err = |v: Value| wake_time(&v, NOW).unwrap_err();
        assert_eq!(
            err(json!({"in": 2})),
            "A wake must be at least 5 minutes ahead"
        );
        assert_eq!(
            err(json!({"in": 60 * 24 * 8})),
            "A wake can be at most 7 days ahead; use a habit instead"
        );
        assert!(wake_time(&json!({"in": 5}), NOW).is_ok());
        assert!(wake_time(&json!({"in": 60 * 24 * 7}), NOW).is_ok());
        assert_eq!(
            err(json!({})),
            r#"Give "in" (minutes) or "at" (date and time)"#
        );
        assert_eq!(
            err(json!({"at": "  "})),
            r#"Give "in" (minutes) or "at" (date and time)"#
        );
        assert_eq!(
            err(json!({"at": "soon"})),
            r#"at must be a date and time like "2026-11-05T09:30""#
        );
        assert_eq!(err(json!({"in": "30"})), "in must be a number of minutes");
        assert_eq!(err(json!({"in": null})), "in must be a number of minutes");
        // A past date is too soon, not a parse error.
        assert_eq!(
            err(json!({"at": "2020-01-01T00:00:00Z"})),
            "A wake must be at least 5 minutes ahead"
        );
    }

    #[test]
    fn the_note_is_a_trimmed_non_empty_string() {
        assert_eq!(wake_note(Some(&json!("  Check it "))).unwrap(), "Check it");
        let text = "note must be a non-empty string under 2000 characters";
        assert_eq!(wake_note(None).unwrap_err(), text);
        assert_eq!(wake_note(Some(&json!("   "))).unwrap_err(), text);
        assert_eq!(wake_note(Some(&json!(5))).unwrap_err(), text);
        assert!(wake_note(Some(&json!("x".repeat(2001)))).is_err());
    }

    #[test]
    fn a_wake_is_a_once_habit_due_at_its_time() {
        let wake = new_wake("Check the refund", NOW + 3_600_000, NOW);
        assert!(wake.once && wake.enabled);
        assert_eq!(wake.next_run_at, NOW + 3_600_000);
        assert_eq!(wake.created_at, NOW);
        assert_eq!(wake.name, "Wake: Check the refund");
        assert_eq!(wake.instructions, "Check the refund");
        assert_eq!(wake.schedule.schedule_kind, "daily");
        wake.schedule.validate().unwrap();
        assert!(wake.when_label().starts_with("Once, "));
        assert!(!wake.due(NOW));
        assert!(wake.due(NOW + 3_600_000));
        let long = new_wake(&"a".repeat(70), NOW + 3_600_000, NOW);
        assert_eq!(long.name, format!("Wake: {}…", "a".repeat(59)));
        assert_eq!(long.instructions.len(), 70);
    }

    #[test]
    fn habits_json_marks_a_wake_with_once_and_leaves_other_habits_alone() {
        let wake = new_wake("Check", NOW + 3_600_000, NOW);
        let plain = Habit::new("A".into(), "B".into(), Schedule::default(), NOW).unwrap();
        let text = serde_json::to_string(&vec![wake, plain]).unwrap();
        assert_eq!(text.matches(r#""once":true"#).count(), 1);
        let back: Vec<Habit> = serde_json::from_str(&text).unwrap();
        assert!(back[0].once && !back[1].once);
        // Only exactly `true` makes a wake, as in the desktop app.
        let odd = text.replace(r#""once":true"#, r#""once":"yes""#);
        assert!(!serde_json::from_str::<Vec<Habit>>(&odd).unwrap()[0].once);
    }

    #[test]
    fn at_most_three_wakes_wait_and_twenty_habits_fit() {
        let mut list = Vec::new();
        for n in 0..WAKES_MAX {
            add_wake(&mut list, new_wake(&format!("w{n}"), NOW + 600_000, NOW)).unwrap();
        }
        assert_eq!(
            add_wake(&mut list, new_wake("w", NOW + 600_000, NOW)).unwrap_err(),
            "At most 3 wakes can wait; run habits.list and remove one"
        );
        // A wake that is switched off no longer waits.
        list[0].enabled = false;
        add_wake(&mut list, new_wake("w", NOW + 600_000, NOW)).unwrap();
        let mut full: Vec<Habit> = (0..habits::MAX)
            .map(|n| Habit::new(format!("H{n}"), "x".into(), Schedule::default(), NOW).unwrap())
            .collect();
        assert_eq!(
            add_wake(&mut full, new_wake("w", NOW + 600_000, NOW)).unwrap_err(),
            "A Mono can have at most 20 habits"
        );
        assert_eq!(full.len(), habits::MAX);
    }

    #[test]
    fn a_wake_runs_once_and_is_removed_when_its_run_is_recorded() {
        let mut list = vec![
            Habit::new("A".into(), "B".into(), Schedule::default(), NOW).unwrap(),
            new_wake("Check", NOW + 600_000, NOW),
        ];
        let at = NOW + 600_000;
        let id = list[1].id.clone();
        assert!(list[1].due(at));
        list[1].claim(at).unwrap();
        // It keeps its time and stays listed while it runs, and is not due again.
        assert_eq!(list[1].next_run_at, at);
        assert!(list[1].enabled && !list[1].due(at + 1));
        assert!(list[1].view()["running"].as_bool().unwrap());
        let done = record_run(&mut list, &id, RunResult::completed(at, None), at + 5).unwrap();
        assert!(done.removed);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "A");
        assert!(record_run(&mut list, &id, RunResult::completed(at, None), at).is_none());
    }

    #[test]
    fn a_failed_wake_is_removed_too_and_says_so() {
        let mut list = vec![new_wake("Check", NOW + 600_000, NOW)];
        let id = list[0].id.clone();
        list[0].claim(NOW + 600_000).unwrap();
        let done = record_run(
            &mut list,
            &id,
            RunResult::failed(NOW + 600_000, Some("offline".into())),
            NOW + 700_000,
        )
        .unwrap();
        assert!(list.is_empty() && done.removed);
        assert!(done.notice.unwrap().contains("offline"));
    }

    #[test]
    fn a_manual_run_of_a_wake_leaves_its_time_and_it_is_still_removed() {
        let mut wake = new_wake("Check", NOW + 3_600_000, NOW);
        wake.run_requested = true;
        wake.claim(NOW).unwrap();
        assert_eq!(wake.next_run_at, NOW + 3_600_000);
        assert!(wake.enabled && !wake.run_requested);
    }

    #[test]
    fn a_missed_wake_is_switched_off_not_run_late() {
        let mut wake = new_wake("Check", NOW + 600_000, NOW);
        let late = NOW + 600_000 + habits::GRACE_MS + 1;
        wake.skip_missed(late).unwrap();
        assert!(!wake.enabled);
        assert!(!wake.due(late));
        // Within the grace period it still runs.
        let mut soon = new_wake("Check", NOW + 600_000, NOW);
        soon.skip_missed(NOW + 600_000 + habits::GRACE_MS).unwrap();
        assert!(soon.enabled);
        // An ordinary habit moves on to its next time.
        let mut daily = Habit::new("A".into(), "B".into(), Schedule::default(), NOW).unwrap();
        daily
            .skip_missed(daily.next_run_at + habits::GRACE_MS + 1)
            .unwrap();
        assert!(daily.enabled && daily.next_run_at > NOW + habits::GRACE_MS);
    }

    #[test]
    fn an_interrupted_wake_is_spent() {
        let mut list = vec![
            new_wake("Check", NOW + 600_000, NOW),
            new_wake("Later", NOW + 900_000, NOW),
        ];
        list[0].claim(NOW + 600_000).unwrap();
        assert_eq!(drop_spent_wakes(&mut list), 0);
        list[0]
            .finish(habits::Run {
                at: NOW + 600_000,
                duration_ms: 1,
                outcome: "interrupted".into(),
                report: None,
                status: None,
                error: Some("The app closed before this run finished".into()),
            })
            .unwrap();
        assert_eq!(drop_spent_wakes(&mut list), 1);
        assert_eq!(list[0].instructions, "Later");
    }

    #[test]
    fn the_view_shows_a_wake_like_the_desktop_app_does() {
        let mut wake = new_wake("Check", NOW + 600_000, NOW);
        let view = wake.view();
        assert_eq!(view["wake"], json!(true));
        assert_eq!(view["enabled"], json!(true));
        assert!(view["schedule"].as_str().unwrap().starts_with("Once, "));
        assert!(view["nextRun"].is_string());
        assert!(view.get("lastRun").is_none() && view.get("running").is_none());
        wake.enabled = false;
        assert!(wake.view()["nextRun"].is_null());
        let mut daily = Habit::new("A".into(), "B".into(), Schedule::default(), NOW).unwrap();
        assert!(daily.view().get("wake").is_none());
        assert_eq!(daily.view()["schedule"], json!("Daily at 09:00"));
        daily.last_run_at = Some(NOW);
        daily.last_outcome = Some("failed".into());
        daily.last_error = Some("down".into());
        let view = daily.view();
        assert_eq!(view["lastOutcome"], json!("failed"));
        assert_eq!(view["lastError"], json!("down"));
    }
}
