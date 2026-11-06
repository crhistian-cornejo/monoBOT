//! "Send later": a message waits in the session's outbox until its time.
//!
//! Port of `src/features/sessions/model/scheduledSend.ts`, the `sendAt` field of
//! `QueuedMessage` (`session.ts`), the queue head rule of `messageQueue.ts`, the
//! time presets of `sessionReminders.ts` (`reminderTime`, shared with "Remind
//! me") and the validation of `shared/ui/DateTimeForm.tsx`.
//!
//! The UI owns the outbox (`Vec<Message>`) of each session. This module says
//! which presets to offer, validates a custom date, picks the next message to
//! send (scheduled ones are passed over), releases the ones whose time came,
//! and saves the scheduled ones so they survive a restart.
//!
//! Persistence: [`save`] / [`load`] keep every session's scheduled messages in
//! one JSON object under the store setting [`SETTING_KEY`]
//! (`monocode.scheduledSends`, `{ "<session id>": [Message, ...] }`). Tauri
//! keeps them inside the session record, not in `localStorage`, so there is no
//! Tauri key to stay compatible with; the camelCase field names match
//! `QueuedMessage` in case a migration copies them over.

use crate::store::Store;
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, TimeZone, Timelike};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Store setting that holds every session's scheduled messages.
pub const SETTING_KEY: &str = "monocode.scheduledSends";

/// Id of the "choose any date and time" step that follows the presets.
pub const CUSTOM: &str = "send-later:custom";

/// A message in a session's outbox.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Message {
    pub id: String,
    pub text: String,
    /// Paths of the attached files.
    pub attachments: Vec<String>,
    /// Epoch ms to send at. `None` means "as soon as the session allows".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub send_at: Option<u64>,
    /// Delivery failed; the message stays available to retry or edit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Message {
    /// Waiting for its time.
    pub fn is_scheduled(&self) -> bool {
        self.send_at.is_some()
    }
}

/// Whether the outbox sends by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueStatus {
    Active,
    Paused,
}

/// The times "Send later" and "Remind me" offer, in menu order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    InOneHour,
    InThreeHours,
    ThisEvening,
    Tomorrow,
    NextWeek,
}

impl Preset {
    pub const ALL: [Preset; 5] = [
        Preset::InOneHour,
        Preset::InThreeHours,
        Preset::ThisEvening,
        Preset::Tomorrow,
        Preset::NextWeek,
    ];

    /// The id Tauri's menus use (`reminder:1h`, ...).
    pub fn id(self) -> &'static str {
        match self {
            Preset::InOneHour => "reminder:1h",
            Preset::InThreeHours => "reminder:3h",
            Preset::ThisEvening => "reminder:evening",
            Preset::Tomorrow => "reminder:tomorrow",
            Preset::NextWeek => "reminder:next-week",
        }
    }

    pub fn from_id(id: &str) -> Option<Preset> {
        Preset::ALL.into_iter().find(|preset| preset.id() == id)
    }
}

/// Epoch ms of `preset` counted from `now`, in `now`'s time zone. `None` when it
/// would not be in the future (this evening, after 18:00).
pub fn preset_time<Tz: TimeZone>(preset: Preset, now: &DateTime<Tz>) -> Option<u64> {
    let at_local = |date: NaiveDate, hour: u32| {
        let naive = date.and_hms_opt(hour, 0, 0)?;
        // A time skipped by a DST jump moves to the next valid one.
        now.timezone()
            .from_local_datetime(&naive)
            .earliest()
            .or_else(|| {
                now.timezone()
                    .from_local_datetime(&(naive + Duration::hours(1)))
                    .earliest()
            })
    };
    let today = now.date_naive();
    let at = match preset {
        Preset::InOneHour => now.clone() + Duration::hours(1),
        Preset::InThreeHours => now.clone() + Duration::hours(3),
        Preset::ThisEvening => at_local(today, 18)?,
        Preset::Tomorrow => at_local(today.succ_opt()?, 9)?,
        Preset::NextWeek => {
            // Next Monday, even when it is Monday: Sun=0 .. Sat=6 as in JS.
            let day = now.weekday().num_days_from_sunday();
            let ahead = match (8 - day) % 7 {
                0 => 7,
                n => n,
            };
            at_local(today + Duration::days(i64::from(ahead)), 9)?
        }
    };
    u64::try_from(at.timestamp_millis())
        .ok()
        .filter(|&ms| ms > now.timestamp_millis() as u64)
}

/// A menu entry for the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresetChoice {
    pub preset: Preset,
    pub label: String,
    /// Epoch ms it would send at.
    pub send_at: Option<u64>,
    /// False when the time has already passed (this evening, late at night).
    pub enabled: bool,
}

/// The presets to list under Send later, with their labels
/// ("In 1 hour (15:30)", "This evening (18:00)", "Tomorrow (9:00)",
/// "Next week (Mon 9:00)"). Follow them with a "Choose date and time" entry
/// that opens the custom form ([`CUSTOM`]).
pub fn preset_choices<Tz: TimeZone>(now: &DateTime<Tz>) -> Vec<PresetChoice> {
    Preset::ALL
        .into_iter()
        .map(|preset| {
            let send_at = preset_time(preset, now);
            let clock = |hours: i64| {
                let at = now.clone() + Duration::hours(hours);
                format!("{}:{:02}", at.hour(), at.minute())
            };
            let label = match preset {
                Preset::InOneHour => format!("In 1 hour ({})", clock(1)),
                Preset::InThreeHours => format!("In 3 hours ({})", clock(3)),
                Preset::ThisEvening => "This evening (18:00)".into(),
                Preset::Tomorrow => "Tomorrow (9:00)".into(),
                Preset::NextWeek => "Next week (Mon 9:00)".into(),
            };
            PresetChoice {
                preset,
                label,
                send_at,
                enabled: send_at.is_some(),
            }
        })
        .collect()
}

/// `YYYY-MM-DDTHH:MM`, the value of the date and time form.
pub fn parse_local_datetime(value: &str) -> Option<NaiveDateTime> {
    let bytes = value.as_bytes();
    let shape = bytes.len() == 16
        && bytes.iter().enumerate().all(|(i, b)| match i {
            4 | 7 => *b == b'-',
            10 => *b == b'T',
            13 => *b == b':',
            _ => b.is_ascii_digit(),
        });
    if !shape {
        return None;
    }
    NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M").ok()
}

/// The same string for a moment, to preselect it in the form.
pub fn format_local_datetime<Tz: TimeZone>(at: &DateTime<Tz>) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}",
        at.year(),
        at.month(),
        at.day(),
        at.hour(),
        at.minute()
    )
}

/// Validate what the date and time form holds. Returns epoch ms, or the message
/// to show under the form.
pub fn validate_custom<Tz: TimeZone>(value: &str, now: &DateTime<Tz>) -> Result<u64, &'static str> {
    let naive = parse_local_datetime(value).ok_or("Choose a valid date and time.")?;
    let at = now
        .timezone()
        .from_local_datetime(&naive)
        .earliest()
        .ok_or("Choose a valid date and time.")?;
    let ms = at.timestamp_millis();
    if ms <= now.timestamp_millis() {
        return Err("Choose a date and time in the future.");
    }
    Ok(ms as u64)
}

/// "15:30" today, "Tomorrow 9:00", "Fri, Nov 7, 9:00" later.
pub fn format_scheduled_send<Tz: TimeZone>(send_at: u64, now: &DateTime<Tz>) -> String {
    let Some(date) = now.timezone().timestamp_millis_opt(send_at as i64).single() else {
        return String::new();
    };
    let time = format!("{}:{:02}", date.hour(), date.minute());
    let today = now.date_naive();
    if date.date_naive() == today {
        time
    } else if Some(date.date_naive()) == today.succ_opt() {
        format!("Tomorrow {time}")
    } else {
        format!("{}, {time}", date.naive_local().format("%a, %b %-d"))
    }
}

/// The next message to send. Scheduled ones wait until their time comes.
pub fn head(queue: &[Message]) -> Option<&Message> {
    queue.iter().find(|message| !message.is_scheduled())
}

/// A message the queue would send as soon as the session allows.
pub fn has_immediate(queue: &[Message]) -> bool {
    queue.iter().any(|message| !message.is_scheduled())
}

/// The earliest scheduled send, to know when to look again.
pub fn next_send_at(queue: &[Message]) -> Option<u64> {
    queue.iter().filter_map(|message| message.send_at).min()
}

/// Turn scheduled messages into ordinary queued ones: those `release` accepts.
/// They keep their place. Returns whether anything changed; a queue that had no
/// status starts [`QueueStatus::Active`], a paused one stays paused.
pub fn release(
    queue: &mut [Message],
    status: &mut Option<QueueStatus>,
    mut release: impl FnMut(&Message) -> bool,
) -> bool {
    let mut changed = false;
    for message in queue.iter_mut() {
        if message.is_scheduled() && release(message) {
            message.send_at = None;
            changed = true;
        }
    }
    if changed && status.is_none() {
        *status = Some(QueueStatus::Active);
    }
    changed
}

/// Release the messages whose time has come (`send_at <= now`).
pub fn release_due(queue: &mut [Message], status: &mut Option<QueueStatus>, now: u64) -> bool {
    release(queue, status, |message| {
        message.send_at.is_some_and(|at| at <= now)
    })
}

/// "Send now" on one scheduled message.
pub fn send_now(queue: &mut [Message], status: &mut Option<QueueStatus>, id: &str) -> bool {
    release(queue, status, |message| message.id == id)
}

/// After a restart the queue pauses only when something was due right away.
pub fn status_after_restart(queue: &[Message]) -> Option<QueueStatus> {
    if queue.is_empty() {
        None
    } else if queue.iter().any(|message| !message.is_scheduled()) {
        Some(QueueStatus::Paused)
    } else {
        Some(QueueStatus::Active)
    }
}

type Book = BTreeMap<String, Vec<Message>>;

fn read_book(store: &Store) -> Result<Book, String> {
    Ok(store
        .setting(SETTING_KEY)?
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default())
}

/// Save the scheduled messages of `session` (others in `queue` are ignored: an
/// immediate message is the turn machinery's business, as in Tauri). An empty
/// result removes the session's entry.
pub fn save(store: &Store, session: i64, queue: &[Message]) -> Result<(), String> {
    let mut book = read_book(store)?;
    let scheduled: Vec<Message> = queue
        .iter()
        .filter(|message| message.is_scheduled())
        .cloned()
        .collect();
    if scheduled.is_empty() {
        book.remove(&session.to_string());
    } else {
        book.insert(session.to_string(), scheduled);
    }
    store.save_setting(
        SETTING_KEY,
        &serde_json::to_string(&book).map_err(|e| e.to_string())?,
    )
}

/// The scheduled messages saved for `session`. Entries without a finite time or
/// an id are dropped, like `sanitizeQueuedMessages` does.
pub fn load(store: &Store, session: i64) -> Result<Vec<Message>, String> {
    Ok(read_book(store)?
        .remove(&session.to_string())
        .unwrap_or_default()
        .into_iter()
        .filter(|message| !message.id.is_empty() && message.is_scheduled())
        .collect())
}

/// Every session that has scheduled messages, with the earliest time of each, so
/// the app can arm one timer at startup.
pub fn all_next_send_at(store: &Store) -> Result<Vec<(i64, u64)>, String> {
    Ok(read_book(store)?
        .into_iter()
        .filter_map(|(session, queue)| Some((session.parse().ok()?, next_send_at(&queue)?)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, Utc};

    fn message(id: &str, send_at: Option<u64>) -> Message {
        Message {
            id: id.into(),
            text: id.into(),
            send_at,
            ..Default::default()
        }
    }

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<FixedOffset> {
        FixedOffset::west_opt(5 * 3600)
            .unwrap()
            .with_ymd_and_hms(y, m, d, h, min, 0)
            .unwrap()
    }

    fn ms(date: DateTime<FixedOffset>) -> Option<u64> {
        Some(date.timestamp_millis() as u64)
    }

    #[test]
    fn the_queue_sends_the_first_message_that_isnt_waiting() {
        let queue = [message("later", Some(5_000)), message("now", None)];
        assert_eq!(head(&queue).unwrap().id, "now");
        assert!(has_immediate(&queue));
    }

    #[test]
    fn nothing_to_send_while_every_message_waits() {
        let queue = [message("later", Some(5_000))];
        assert!(head(&queue).is_none());
        assert!(!has_immediate(&queue));
    }

    #[test]
    fn releases_messages_whose_time_came_in_place() {
        let mut queue = vec![
            message("first", Some(1_000)),
            message("normal", None),
            message("later", Some(9_000)),
        ];
        let mut status = Some(QueueStatus::Active);
        assert!(release_due(&mut queue, &mut status, 1_000));
        assert_eq!(
            queue,
            [
                message("first", None),
                message("normal", None),
                message("later", Some(9_000))
            ]
        );
        assert_eq!(head(&queue).unwrap().id, "first");
    }

    #[test]
    fn leaves_the_queue_alone_when_nothing_is_due() {
        let mut queue = vec![message("later", Some(9_000))];
        let mut status = None;
        assert!(!release_due(&mut queue, &mut status, 1_000));
        assert_eq!(queue, [message("later", Some(9_000))]);
        assert_eq!(status, None);
    }

    #[test]
    fn starts_a_queue_that_had_no_status_and_keeps_a_paused_one_paused() {
        let mut queue = vec![message("a", Some(0))];
        let mut status = None;
        release_due(&mut queue, &mut status, 1);
        assert_eq!(status, Some(QueueStatus::Active));
        let mut queue = vec![message("a", Some(0))];
        let mut status = Some(QueueStatus::Paused);
        release_due(&mut queue, &mut status, 1);
        assert_eq!(status, Some(QueueStatus::Paused));
    }

    #[test]
    fn sends_one_message_now_on_request() {
        let mut queue = vec![message("a", Some(9_000)), message("b", Some(9_000))];
        let mut status = None;
        assert!(send_now(&mut queue, &mut status, "b"));
        let times: Vec<_> = queue.iter().map(|m| m.send_at).collect();
        assert_eq!(times, [Some(9_000), None]);
    }

    #[test]
    fn finds_the_earliest_scheduled_send() {
        let queue = [
            message("a", Some(9_000)),
            message("b", None),
            message("c", Some(4_000)),
        ];
        assert_eq!(next_send_at(&queue), Some(4_000));
        assert_eq!(next_send_at(&[message("b", None)]), None);
    }

    #[test]
    fn names_today_tomorrow_and_later_days() {
        let now = at(2026, 11, 5, 14, 0);
        let today = format_scheduled_send(ms(at(2026, 11, 5, 15, 30)).unwrap(), &now);
        assert_eq!(today, "15:30");
        assert_eq!(
            format_scheduled_send(ms(at(2026, 11, 6, 9, 0)).unwrap(), &now),
            "Tomorrow 9:00"
        );
        assert_eq!(
            format_scheduled_send(ms(at(2026, 11, 9, 9, 0)).unwrap(), &now),
            "Mon, Nov 9, 9:00"
        );
    }

    #[test]
    fn presets_use_elapsed_hours_across_midnight() {
        let now = at(2026, 9, 11, 23, 45);
        assert_eq!(
            preset_time(Preset::InOneHour, &now),
            ms(at(2026, 9, 12, 0, 45))
        );
        assert_eq!(
            preset_time(Preset::InThreeHours, &now),
            Some(now.timestamp_millis() as u64 + 10_800_000)
        );
    }

    #[test]
    fn this_evening_is_never_in_the_past() {
        assert_eq!(
            preset_time(Preset::ThisEvening, &at(2026, 9, 11, 17, 59)),
            ms(at(2026, 9, 11, 18, 0))
        );
        assert_eq!(
            preset_time(Preset::ThisEvening, &at(2026, 9, 11, 18, 0)),
            None
        );
        assert_eq!(
            preset_time(Preset::ThisEvening, &at(2026, 9, 11, 23, 0)),
            None
        );
        let choices = preset_choices(&at(2026, 9, 11, 23, 0));
        assert!(!choices[2].enabled);
        assert!(choices[3].enabled);
    }

    #[test]
    fn tomorrow_is_nine_local_across_a_year_boundary() {
        assert_eq!(
            preset_time(Preset::Tomorrow, &at(2026, 12, 31, 21, 0)),
            ms(at(2027, 1, 1, 9, 0))
        );
    }

    #[test]
    fn next_week_is_the_next_monday_even_before_nine() {
        for (day, monday) in [(11, 14), (13, 14), (14, 21)] {
            assert_eq!(
                preset_time(Preset::NextWeek, &at(2026, 9, day, 8, 0)),
                ms(at(2026, 9, monday, 9, 0)),
                "from September {day}"
            );
        }
    }

    #[test]
    fn preset_labels_and_ids() {
        let choices = preset_choices(&at(2026, 9, 11, 9, 5));
        let labels: Vec<_> = choices.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "In 1 hour (10:05)",
                "In 3 hours (12:05)",
                "This evening (18:00)",
                "Tomorrow (9:00)",
                "Next week (Mon 9:00)"
            ]
        );
        assert_eq!(Preset::from_id("reminder:tomorrow"), Some(Preset::Tomorrow));
        assert_eq!(Preset::from_id("reminder:cancel"), None);
        assert_eq!(Preset::from_id("unknown"), None);
    }

    #[test]
    fn validates_the_custom_date() {
        let now = at(2026, 11, 5, 14, 0);
        assert_eq!(
            validate_custom("2026-11-05T15:30", &now),
            Ok(ms(at(2026, 11, 5, 15, 30)).unwrap())
        );
        assert_eq!(
            validate_custom("2026-11-05T14:00", &now),
            Err("Choose a date and time in the future.")
        );
        for bad in [
            "",
            "2026-11-05",
            "2026-02-30T10:00",
            "2026-11-05T25:00",
            "2026-11-05 10:00",
        ] {
            assert_eq!(
                validate_custom(bad, &now),
                Err("Choose a valid date and time."),
                "{bad}"
            );
        }
        assert_eq!(format_local_datetime(&now), "2026-11-05T14:00");
    }

    #[test]
    fn restart_pauses_only_when_something_was_due_right_away() {
        assert_eq!(status_after_restart(&[]), None);
        assert_eq!(
            status_after_restart(&[message("a", Some(1))]),
            Some(QueueStatus::Active)
        );
        assert_eq!(
            status_after_restart(&[message("a", Some(1)), message("b", None)]),
            Some(QueueStatus::Paused)
        );
    }

    #[test]
    fn scheduled_messages_survive_in_the_store() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let queue = [
            message("a", Some(9_000)),
            message("b", None),
            message("c", Some(4_000)),
        ];
        save(&store, 7, &queue).unwrap();
        save(&store, 8, &[message("z", Some(1))]).unwrap();
        let loaded = load(&store, 7).unwrap();
        assert_eq!(
            loaded,
            [message("a", Some(9_000)), message("c", Some(4_000))]
        );
        let mut next = all_next_send_at(&store).unwrap();
        next.sort();
        assert_eq!(next, [(7, 4_000), (8, 1)]);
        save(&store, 7, &[message("b", None)]).unwrap();
        assert!(load(&store, 7).unwrap().is_empty());
        assert_eq!(load(&store, 99).unwrap(), []);
        let _ = Utc::now();
    }
}
