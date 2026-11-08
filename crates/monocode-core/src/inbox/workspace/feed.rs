//! A calendar the user follows by its private iCal address, such as Google Calendar's
//! «Secret address in iCal format». The address works like a password, so it is kept in the
//! system's store of secrets (Keychain, Credential Manager), never in a file. Its events are
//! read only: fetched over HTTPS, recurring ones laid out over a window around today, and kept
//! in `mail/calendar-feed.json` until the next refresh. Nothing is ever written back.
use super::calendar::{checked_feed, ics_date_of};
use super::{read_json, secrets, write_json, CalendarEvent, Service, WRITE};
use chrono::{
    DateTime, Datelike, Duration, FixedOffset, NaiveDate, NaiveDateTime, TimeZone, Utc, Weekday,
};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{BufReader, Read};

const KIND: &str = "calendar";
/// The largest calendar fetched, and how long a fetch may take.
const MAX_BYTES: u64 = 8 * 1024 * 1024;
/// How far back and ahead recurring events are laid out.
const PAST_DAYS: i64 = 62;
const AHEAD_DAYS: i64 = 400;
const MAX_EVENTS: usize = 5_000;
/// A feed older than this is fetched again when the agenda opens.
pub const STALE_MS: i64 = 15 * 60_000;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Cached {
    events: Vec<CalendarEvent>,
    refreshed_at: i64,
    error: String,
    /// Recurring events whose rule could not be read, shown once only.
    simplified: usize,
}

/// Where a followed calendar stands, for the agenda to say.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeedState {
    pub connected: bool,
    pub refreshed_at: i64,
    pub events: usize,
    pub error: String,
    pub simplified: usize,
}

/// The address as it will be fetched: `webcal://` becomes `https://`; only a public https
/// address is accepted.
pub fn feed_url(text: &str) -> Result<String, String> {
    let text = text.trim();
    let url = match text.strip_prefix("webcal://") {
        Some(rest) => format!("https://{rest}"),
        None => text.to_owned(),
    };
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) || url.len() > 2_000 {
        return Err("Paste the calendar's iCal address on one line.".into());
    }
    monobot_email::unsubscribe::public_host(&url)
        .map_err(|_| "Use the calendar's https:// (or webcal://) iCal address.".to_owned())?;
    Ok(url)
}

impl Service {
    fn feed_path(&self) -> std::path::PathBuf {
        self.path("calendar-feed.json")
    }

    /// Follows the calendar at `address` (empty: stops following it and forgets its events).
    pub fn set_calendar_feed(&self, address: &str) -> Result<(), String> {
        if address.trim().is_empty() {
            secrets::set(&self.data_dir, KIND, "")?;
            let _guard = WRITE.lock().unwrap_or_else(|p| p.into_inner());
            let _ = std::fs::remove_file(self.feed_path());
            return Ok(());
        }
        let url = feed_url(address)?;
        secrets::set(&self.data_dir, KIND, &url)
    }

    pub fn calendar_feed_state(&self) -> FeedState {
        let connected = secrets::get(&self.data_dir, KIND).is_ok();
        let cached: Cached = read_json(&self.feed_path()).unwrap_or_default();
        FeedState {
            connected,
            refreshed_at: cached.refreshed_at,
            events: cached.events.len(),
            error: cached.error,
            simplified: cached.simplified,
        }
    }

    /// Fetches the followed calendar and keeps its events. Its address never leaves the
    /// system's store except for this request.
    pub fn refresh_calendar_feed(&self) -> Result<usize, String> {
        let url = secrets::get(&self.data_dir, KIND)
            .map_err(|_| "No calendar is followed yet.".to_owned())?;
        let now = Utc::now();
        let result = fetch(&url).and_then(|text| {
            parse_feed(
                &text,
                now - Duration::days(PAST_DAYS),
                now + Duration::days(AHEAD_DAYS),
            )
        });
        let _guard = WRITE.lock().unwrap_or_else(|p| p.into_inner());
        let mut cached: Cached = read_json(&self.feed_path()).unwrap_or_default();
        cached.refreshed_at = now.timestamp_millis();
        let outcome = match result {
            Ok((events, simplified)) => {
                let count = events.len();
                cached.events = events;
                cached.simplified = simplified;
                cached.error.clear();
                Ok(count)
            }
            Err(error) => {
                // The events already kept stay: a failed fetch is not an empty calendar.
                cached.error = error.clone();
                Err(error)
            }
        };
        write_json(&self.feed_path(), &cached)?;
        outcome
    }

    /// The followed calendar's events between two instants (milliseconds).
    pub(super) fn feed_events(&self, from: i64, to: i64) -> Vec<CalendarEvent> {
        let cached: Cached = read_json(&self.feed_path()).unwrap_or_default();
        cached
            .events
            .into_iter()
            .filter(|event| {
                DateTime::parse_from_rfc3339(&event.start).is_ok_and(|a| a.timestamp_millis() < to)
                    && DateTime::parse_from_rfc3339(&event.end)
                        .is_ok_and(|b| b.timestamp_millis() > from)
            })
            .collect()
    }
}

fn fetch(url: &str) -> Result<String, String> {
    let response = monobot_http::agent()
        .redirects(2)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .get(url)
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(404, _) | ureq::Error::Status(403, _) => {
                "The calendar address was refused. Copy the secret iCal address again.".to_owned()
            }
            ureq::Error::Status(status, _) => format!("The calendar answered HTTP {status}."),
            _ => "The calendar could not be reached.".to_owned(),
        })?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "The calendar could not be read.")?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("The calendar is too large (8 MiB at most).".into());
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if !text.to_ascii_uppercase().contains("BEGIN:VCALENDAR") {
        return Err("That address does not return a calendar.".into());
    }
    Ok(text)
}

/// A recurrence rule, the parts MonoBOT lays out.
#[derive(Debug, Default, PartialEq, Eq)]
struct Rule {
    freq: String,
    interval: i64,
    count: Option<usize>,
    until: Option<i64>,
    by_day: Vec<(i64, Weekday)>,
    by_month_day: Vec<i64>,
    /// It asks for parts not laid out (BYSETPOS, BYWEEKNO…): only its first time is shown.
    unsupported: bool,
}

fn weekday(code: &str) -> Option<Weekday> {
    Some(match code {
        "MO" => Weekday::Mon,
        "TU" => Weekday::Tue,
        "WE" => Weekday::Wed,
        "TH" => Weekday::Thu,
        "FR" => Weekday::Fri,
        "SA" => Weekday::Sat,
        "SU" => Weekday::Sun,
        _ => return None,
    })
}

fn parse_rule(text: &str) -> Rule {
    let mut rule = Rule {
        interval: 1,
        ..Rule::default()
    };
    for part in text.split(';') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key.trim().to_ascii_uppercase().as_str() {
            "FREQ" => rule.freq = value.trim().to_ascii_uppercase(),
            "INTERVAL" => rule.interval = value.trim().parse().unwrap_or(1).max(1),
            "COUNT" => rule.count = value.trim().parse().ok(),
            "UNTIL" => {
                let value = value.trim();
                rule.until = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%SZ")
                    .map(|t| t.and_utc().timestamp_millis())
                    .or_else(|_| {
                        NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S")
                            .map(|t| t.and_utc().timestamp_millis())
                    })
                    .or_else(|_| {
                        NaiveDate::parse_from_str(value, "%Y%m%d").map(|d| {
                            d.and_hms_opt(23, 59, 59)
                                .unwrap_or_default()
                                .and_utc()
                                .timestamp_millis()
                        })
                    })
                    .ok();
            }
            "BYDAY" => {
                for day in value.split(',') {
                    let day = day.trim().to_ascii_uppercase();
                    let split = day.len().saturating_sub(2);
                    let (n, code) = day.split_at(split);
                    match (
                        weekday(code),
                        n.parse::<i64>().ok().or((n.is_empty()).then_some(0)),
                    ) {
                        (Some(w), Some(n)) => rule.by_day.push((n, w)),
                        _ => rule.unsupported = true,
                    }
                }
            }
            "BYMONTHDAY" => {
                for day in value.split(',') {
                    match day.trim().parse::<i64>() {
                        Ok(d) if d != 0 && (-31..=31).contains(&d) => rule.by_month_day.push(d),
                        _ => rule.unsupported = true,
                    }
                }
            }
            "WKST" | "BYMONTH" => {}
            _ => rule.unsupported = true,
        }
    }
    if !matches!(
        rule.freq.as_str(),
        "DAILY" | "WEEKLY" | "MONTHLY" | "YEARLY"
    ) {
        rule.unsupported = true;
    }
    rule
}

fn add_months(date: NaiveDate, months: i64) -> Option<NaiveDate> {
    let total = date.year() as i64 * 12 + date.month0() as i64 + months;
    let (year, month0) = (total.div_euclid(12) as i32, total.rem_euclid(12) as u32);
    NaiveDate::from_ymd_opt(year, month0 + 1, 1)
}

fn days_in_month(first: NaiveDate) -> u32 {
    add_months(first, 1)
        .map(|next| (next - first).num_days() as u32)
        .unwrap_or(30)
}

/// The days of a month a monthly or yearly rule picks.
fn month_days(first: NaiveDate, rule: &Rule, start_day: u32) -> Vec<NaiveDate> {
    let length = days_in_month(first);
    let mut days = Vec::new();
    if !rule.by_month_day.is_empty() {
        for d in &rule.by_month_day {
            let day = if *d > 0 { *d } else { length as i64 + 1 + d };
            if (1..=length as i64).contains(&day) {
                days.push(first.with_day(day as u32).unwrap_or(first));
            }
        }
    } else if !rule.by_day.is_empty() {
        for (n, weekday) in &rule.by_day {
            let matching: Vec<NaiveDate> = (1..=length)
                .filter_map(|d| first.with_day(d))
                .filter(|d| d.weekday() == *weekday)
                .collect();
            match *n {
                0 => days.extend(matching),
                n if n > 0 => days.extend(matching.get(n as usize - 1)),
                n => days.extend(
                    matching
                        .len()
                        .checked_sub((-n) as usize)
                        .and_then(|i| matching.get(i)),
                ),
            }
        }
    } else if start_day <= length {
        days.push(first.with_day(start_day).unwrap_or(first));
    }
    days.sort();
    days.dedup();
    days
}

/// The dates a rule repeats on, from `start` (its first), until `to` or its end.
fn dates(start: NaiveDate, rule: &Rule, to: NaiveDate) -> Vec<NaiveDate> {
    let mut out = Vec::new();
    let mut period = 0i64;
    while out.len() < MAX_EVENTS && period < 20_000 {
        let candidates: Vec<NaiveDate> = match rule.freq.as_str() {
            "DAILY" => vec![start + Duration::days(period * rule.interval)],
            "WEEKLY" => {
                let week = start - Duration::days(start.weekday().num_days_from_monday() as i64)
                    + Duration::weeks(period * rule.interval);
                if rule.by_day.is_empty() {
                    vec![week + Duration::days(start.weekday().num_days_from_monday() as i64)]
                } else {
                    let mut days: Vec<NaiveDate> = rule
                        .by_day
                        .iter()
                        .map(|(_, w)| week + Duration::days(w.num_days_from_monday() as i64))
                        .collect();
                    days.sort();
                    days.dedup();
                    days
                }
            }
            "MONTHLY" => add_months(start.with_day(1).unwrap_or(start), period * rule.interval)
                .map(|first| month_days(first, rule, start.day()))
                .unwrap_or_default(),
            _ => add_months(
                start.with_day(1).unwrap_or(start),
                12 * period * rule.interval,
            )
            .and_then(|first| first.with_month(start.month()))
            .map(|first| {
                if rule.by_day.is_empty() && rule.by_month_day.is_empty() {
                    first.with_day(start.day()).into_iter().collect()
                } else {
                    month_days(first, rule, start.day())
                }
            })
            .unwrap_or_default(),
        };
        if candidates.iter().all(|d| *d > to) && !candidates.is_empty() {
            break;
        }
        for date in candidates {
            if date < start || date > to {
                continue;
            }
            out.push(date);
            if rule.count.is_some_and(|count| out.len() >= count) {
                return out;
            }
        }
        period += 1;
    }
    out
}

struct Base {
    uid: String,
    title: String,
    description: String,
    start: DateTime<FixedOffset>,
    end: DateTime<FixedOffset>,
    timezone: String,
    all_day: bool,
    /// It replaces one time of a recurring event (it has a RECURRENCE-ID).
    replaces: bool,
}

/// The events of a calendar file between `from` and `to`, recurring ones laid out; and how
/// many recurring events had a rule that is only shown at its first time.
pub(crate) fn parse_feed(
    text: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<(Vec<CalendarEvent>, usize), String> {
    let mut events = Vec::new();
    let mut simplified = 0;
    // The times of recurring events that were moved, changed or cancelled: (uid, start).
    let mut moved: HashSet<(String, i64)> = HashSet::new();
    let mut later: Vec<(Base, Vec<i64>, Option<String>)> = Vec::new();
    for calendar in ical::IcalParser::new(BufReader::new(text.as_bytes())) {
        let calendar = calendar.map_err(|_| "The calendar file could not be read.")?;
        for event in calendar.events {
            let property = |name: &str| event.properties.iter().find(|p| p.name == name);
            let value = |name: &str| {
                property(name)
                    .and_then(|p| p.value.clone())
                    .unwrap_or_default()
            };
            let (Some(start), Some(uid)) = (
                property("DTSTART"),
                property("UID").and_then(|p| p.value.clone()),
            ) else {
                continue;
            };
            let Ok((start, timezone, all_day)) = ics_date_of(start) else {
                continue;
            };
            let end = match property("DTEND").map(ics_date_of) {
                Some(Ok((end, _, _))) if end > start => end,
                _ => {
                    if all_day {
                        start + Duration::days(1)
                    } else {
                        let minutes = value("DURATION")
                            .trim_start_matches("PT")
                            .trim_end_matches('M')
                            .parse::<i64>()
                            .unwrap_or(60);
                        start + Duration::minutes(minutes.clamp(1, 24 * 60))
                    }
                }
            };
            let cancelled = value("STATUS").eq_ignore_ascii_case("CANCELLED");
            let replaces = property("RECURRENCE-ID").is_some();
            if let Some(recurrence) = property("RECURRENCE-ID").and_then(|p| ics_date_of(p).ok()) {
                moved.insert((uid.clone(), recurrence.0.timestamp_millis()));
            }
            let exdates: Vec<i64> = event
                .properties
                .iter()
                .filter(|p| p.name == "EXDATE")
                .flat_map(|p| {
                    let params = p.params.clone();
                    p.value
                        .clone()
                        .unwrap_or_default()
                        .split(',')
                        .filter_map(|v| {
                            let one = ical::property::Property {
                                name: "EXDATE".into(),
                                params: params.clone(),
                                value: Some(v.trim().to_owned()),
                            };
                            ics_date_of(&one).ok().map(|d| d.0.timestamp_millis())
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            let base = Base {
                uid: uid.clone(),
                title: unescape(&value("SUMMARY")),
                description: unescape(&value("DESCRIPTION")),
                start,
                end,
                timezone,
                all_day,
                replaces,
            };
            let rule = (!replaces)
                .then(|| property("RRULE").and_then(|p| p.value.clone()))
                .flatten();
            if cancelled {
                continue;
            }
            later.push((base, exdates, rule));
        }
    }
    for (base, exdates, rule) in later {
        let duration = base.end - base.start;
        let mut starts: Vec<DateTime<FixedOffset>> = Vec::new();
        match rule.map(|r| parse_rule(&r)) {
            Some(rule) if !rule.unsupported => {
                let zone: Option<Tz> = base.timezone.parse().ok();
                let local = base.start.naive_local();
                let last = (to + Duration::days(1)).date_naive();
                for date in dates(local.date(), &rule, last) {
                    let naive = date.and_time(local.time());
                    let at = match zone {
                        Some(zone) => zone
                            .from_local_datetime(&naive)
                            .earliest()
                            .map(|t| t.fixed_offset()),
                        None => base.start.offset().from_local_datetime(&naive).single(),
                    };
                    let Some(at) = at else { continue };
                    if rule
                        .until
                        .is_some_and(|until| at.timestamp_millis() > until)
                    {
                        break;
                    }
                    starts.push(at);
                }
            }
            Some(_) => {
                simplified += 1;
                starts.push(base.start);
            }
            None => starts.push(base.start),
        }
        for start in starts {
            let at = start.timestamp_millis();
            if exdates.contains(&at) {
                continue;
            }
            // A moved, changed or cancelled time of a series is its own entry (or none).
            if !base.replaces && moved.contains(&(base.uid.clone(), at)) {
                continue;
            }
            let end = start + duration;
            if end.with_timezone(&Utc) <= from || start.with_timezone(&Utc) >= to {
                continue;
            }
            let id = format!("feed:{}:{}", base.uid, at);
            let id = if base.replaces {
                format!("{id}:moved")
            } else {
                id
            };
            if let Ok(event) = checked_feed(
                id,
                &base.title,
                &base.description,
                start,
                end,
                &base.timezone,
                base.all_day,
                &base.uid,
            ) {
                events.push(event);
            }
            if events.len() >= MAX_EVENTS {
                return Err(
                    "The calendar has too many events to follow (5,000 in its window).".into(),
                );
            }
        }
    }
    events.sort_by_key(|e| e.start.clone());
    Ok((events, simplified))
}

fn unescape(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(c) => out.push(c),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calendar(events: &str) -> String {
        format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{events}END:VCALENDAR\r\n")
    }

    fn window() -> (DateTime<Utc>, DateTime<Utc>) {
        (
            Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 12, 31, 0, 0, 0).unwrap(),
        )
    }

    fn starts(events: &[CalendarEvent]) -> Vec<String> {
        events.iter().map(|e| e.start[..16].to_owned()).collect()
    }

    #[test]
    fn a_weekly_meeting_repeats_on_its_days_skips_exceptions_and_follows_a_moved_time() {
        let text = calendar(concat!(
            "BEGIN:VEVENT\r\nUID:standup\r\nSUMMARY:Standup\r\n",
            "DTSTART;TZID=America/Lima:20261102T090000\r\nDTEND;TZID=America/Lima:20261102T091500\r\n",
            "RRULE:FREQ=WEEKLY;BYDAY=MO,WE;COUNT=5\r\n",
            "EXDATE;TZID=America/Lima:20261104T090000\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:standup\r\nSUMMARY:Standup (moved)\r\n",
            "RECURRENCE-ID;TZID=America/Lima:20261109T090000\r\n",
            "DTSTART;TZID=America/Lima:20261109T110000\r\nDTEND;TZID=America/Lima:20261109T111500\r\nEND:VEVENT\r\n",
        ));
        let (from, to) = window();
        let (events, simplified) = parse_feed(&text, from, to).unwrap();
        assert_eq!(simplified, 0);
        assert_eq!(
            starts(&events),
            [
                "2026-11-02T09:00",
                "2026-11-09T11:00",
                "2026-11-11T09:00",
                "2026-11-16T09:00"
            ]
        );
        assert_eq!(events[1].title, "Standup (moved)");
        assert!(events
            .iter()
            .all(|e| e.source == "feed" && e.timezone == "America/Lima"));
        assert!(events[0].start.ends_with("-05:00"));
    }

    #[test]
    fn monthly_daily_and_whole_day_rules_and_what_cannot_be_laid_out() {
        let text = calendar(concat!(
            "BEGIN:VEVENT\r\nUID:pay\r\nSUMMARY:Payday\r\nDTSTART;VALUE=DATE:20261030\r\n",
            "DTEND;VALUE=DATE:20261031\r\nRRULE:FREQ=MONTHLY;BYDAY=-1FR\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:gym\r\nSUMMARY:Gym\r\nDTSTART:20261201T120000Z\r\nDTEND:20261201T130000Z\r\n",
            "RRULE:FREQ=DAILY;INTERVAL=2;UNTIL=20261205T235959Z\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:odd\r\nSUMMARY:Odd\r\nDTSTART:20261115T100000Z\r\nDTEND:20261115T110000Z\r\n",
            "RRULE:FREQ=MONTHLY;BYSETPOS=2;BYDAY=TU\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:gone\r\nSUMMARY:Gone\r\nSTATUS:CANCELLED\r\nDTSTART:20261120T100000Z\r\nDTEND:20261120T110000Z\r\nEND:VEVENT\r\n",
        ));
        let (from, to) = window();
        let (events, simplified) = parse_feed(&text, from, to).unwrap();
        let titles = |title: &str| {
            events
                .iter()
                .filter(|e| e.title == title)
                .map(|e| e.start[..10].to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(titles("Payday"), ["2026-11-27", "2026-12-25"]);
        assert!(events
            .iter()
            .filter(|e| e.title == "Payday")
            .all(|e| e.all_day));
        assert_eq!(titles("Gym"), ["2026-12-01", "2026-12-03", "2026-12-05"]);
        assert_eq!(titles("Odd"), ["2026-11-15"]);
        assert_eq!(simplified, 1);
        assert!(titles("Gone").is_empty());
    }

    #[test]
    fn the_address_is_a_secret_and_only_a_public_https_one_is_followed() {
        assert_eq!(
            feed_url(" webcal://calendar.google.com/calendar/ical/x/private-abc/basic.ics ")
                .unwrap(),
            "https://calendar.google.com/calendar/ical/x/private-abc/basic.ics"
        );
        assert!(feed_url("http://calendar.example.com/a.ics").is_err());
        assert!(feed_url("https://192.168.0.2/a.ics").is_err());
        assert!(feed_url("https://a.test/x y.ics").is_err());
        let dir = tempfile::tempdir().unwrap();
        let service = Service::new(dir.path());
        assert!(!service.calendar_feed_state().connected);
        assert!(service.refresh_calendar_feed().is_err());
        service
            .set_calendar_feed("https://calendar.google.com/calendar/ical/x/private-abc/basic.ics")
            .unwrap();
        assert!(service.calendar_feed_state().connected);
        assert!(!std::fs::read_dir(dir.path())
            .map(|d| d.flatten().any(|e| std::fs::read_to_string(e.path())
                .unwrap_or_default()
                .contains("private-abc")))
            .unwrap_or(false));
        service.set_calendar_feed("").unwrap();
        assert!(!service.calendar_feed_state().connected);
    }
}
