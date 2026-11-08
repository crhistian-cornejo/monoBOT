use super::{read_json, write_json, Service, WRITE};
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::BufReader;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    pub id: String,
    pub title: String,
    pub start: String,
    pub end: String,
    pub timezone: String,
    pub description: String,
    pub source: String,
    #[serde(default)]
    pub uid: String,
    #[serde(default)]
    pub all_day: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NewEvent {
    pub title: String,
    pub start: String,
    pub end: String,
    pub timezone: String,
    pub description: String,
}

fn instant(value: &str) -> Result<DateTime<chrono::FixedOffset>, String> {
    DateTime::parse_from_rfc3339(value).map_err(|_| {
        "Use ISO 8601 dates with an explicit offset (for example 2026-11-07T10:00:00+01:00).".into()
    })
}
fn range(start: &str, end: &str) -> Result<(i64, i64), String> {
    let (a, b) = (
        instant(start)?.timestamp_millis(),
        instant(end)?.timestamp_millis(),
    );
    if b <= a || b - a > 90 * 86_400_000 {
        return Err("Choose a positive agenda range of at most 90 days.".into());
    }
    Ok((a, b))
}
fn checked(event: NewEvent, id: String, source: &str) -> Result<CalendarEvent, String> {
    let title = event.title.trim();
    if title.is_empty()
        || title.len() > 500
        || title.chars().any(char::is_control)
        || event.description.len() > 10_000
    {
        return Err(
            "Enter a title of at most 500 bytes and a description of at most 10,000 bytes.".into(),
        );
    }
    let timezone: Tz = if event.timezone.trim().is_empty() {
        chrono_tz::UTC
    } else {
        event
            .timezone
            .parse()
            .map_err(|_| "Use an IANA time zone, for example Europe/Madrid.")?
    };
    let (start, end) = (instant(&event.start)?, instant(&event.end)?);
    if end <= start {
        return Err("The event must end after it starts.".into());
    }
    Ok(CalendarEvent {
        uid: format!("{id}@monobot.local"),
        id,
        title: title.into(),
        start: start.with_timezone(&timezone).to_rfc3339(),
        end: end.with_timezone(&timezone).to_rfc3339(),
        timezone: timezone.name().into(),
        description: event.description,
        source: source.into(),
        all_day: false,
    })
}

impl Service {
    /// Local reminders/time blocks never send invitations or change a remote calendar.
    pub fn create_event(&self, event: NewEvent) -> Result<CalendarEvent, String> {
        let event = checked(event, uuid::Uuid::new_v4().to_string(), "local")?;
        let _guard = WRITE.lock().unwrap_or_else(|p| p.into_inner());
        let mut events: Vec<CalendarEvent> = read_json(&self.path("agenda.json"))?;
        if events.len() >= 2000 {
            return Err("Export and remove older events before adding more (limit: 2,000).".into());
        }
        events.push(event.clone());
        write_json(&self.path("agenda.json"), &events)?;
        Ok(event)
    }
    pub fn remove_event(&self, id: &str) -> Result<(), String> {
        let _guard = WRITE.lock().unwrap_or_else(|p| p.into_inner());
        let mut events: Vec<CalendarEvent> = read_json(&self.path("agenda.json"))?;
        events.retain(|event| event.id != id);
        write_json(&self.path("agenda.json"), &events)
    }
    pub fn agenda(&self, start: &str, end: &str) -> Result<Vec<CalendarEvent>, String> {
        self.agenda_with_remote(start, end, true)
    }
    /// Agent callers must pass false unless the user granted the remote mail server to
    /// their identity. The user's native Agenda view may include it directly.
    pub fn agenda_with_remote(
        &self,
        start: &str,
        end: &str,
        include_remote: bool,
    ) -> Result<Vec<CalendarEvent>, String> {
        let (from, to) = range(start, end)?;
        let mut events: Vec<CalendarEvent> = read_json(&self.path("agenda.json"))?;
        events.retain(|event| {
            instant(&event.start).is_ok_and(|a| a.timestamp_millis() < to)
                && instant(&event.end).is_ok_and(|b| b.timestamp_millis() > from)
        });
        if include_remote && !self.config()?.mailflare_url.is_empty() {
            let schedule = self.mailflare_call("get_schedule", json!({"start":start,"end":end}))?;
            if schedule["truncated"] == true {
                return Err(
                    "The mail server's schedule was truncated. Choose a shorter date range.".into(),
                );
            }
            let remote = schedule["events"]
                .as_array()
                .ok_or("The mail server returned an invalid schedule.")?;
            for item in remote {
                let text = |key: &str| item[key].as_str().unwrap_or("").to_owned();
                let event = checked(
                    NewEvent {
                        title: text("title"),
                        start: text("startsAt"),
                        end: text("endsAt"),
                        timezone: text("timeZone"),
                        description: text("description"),
                    },
                    format!("mailflare:{}", text("id")),
                    "mailflare",
                )?;
                events.push(event);
            }
        }
        events.sort_by_key(|event| {
            instant(&event.start)
                .map(|at| at.timestamp_millis())
                .unwrap_or(0)
        });
        Ok(events)
    }
    /// The user's own agenda view: their events, the calendar they follow, and the remote
    /// mail server's schedule when one is set up. Agents never get the followed calendar.
    pub fn agenda_for_user(&self, start: &str, end: &str) -> Result<Vec<CalendarEvent>, String> {
        let (from, to) = range(start, end)?;
        let mut events = self.agenda_with_remote(start, end, true)?;
        events.extend(self.feed_events(from, to));
        events.sort_by_key(|event| {
            instant(&event.start)
                .map(|at| at.timestamp_millis())
                .unwrap_or(0)
        });
        Ok(events)
    }
    /// Atomic import: unsupported recurrence or ambiguous floating/DST times
    /// reject the entire file instead of silently moving or dropping events.
    pub fn import_ics(&self, text: &str) -> Result<usize, String> {
        if text.len() > 2 * 1024 * 1024 {
            return Err("ICS files must be at most 2 MiB.".into());
        }
        let mut imported = Vec::new();
        for calendar in ical::IcalParser::new(BufReader::new(text.as_bytes())) {
            let calendar = calendar.map_err(|_| "Invalid ICS calendar.")?;
            for event in calendar.events {
                if imported.len() >= 2000 {
                    return Err("An import can contain at most 2,000 events.".into());
                }
                if event.properties.iter().any(|p| {
                    matches!(
                        p.name.as_str(),
                        "RRULE" | "RDATE" | "EXDATE" | "RECURRENCE-ID"
                    )
                }) {
                    return Err("Recurring events are not imported from a file. Follow the calendar by its iCal address instead (Agenda › Calendar).".into());
                }
                let property = |name: &str| event.properties.iter().find(|p| p.name == name);
                let value = |name: &str| {
                    property(name)
                        .and_then(|p| p.value.as_deref())
                        .unwrap_or("")
                };
                let start_prop = property("DTSTART").ok_or("ICS event is missing DTSTART.")?;
                let end_prop = property("DTEND").ok_or("ICS time blocks need DTEND.")?;
                let date_only = |property: &ical::property::Property| {
                    property
                        .value
                        .as_deref()
                        .is_some_and(|value| value.len() == 8)
                };
                let all_day = date_only(start_prop);
                if all_day != date_only(end_prop) {
                    return Err("ICS DTSTART and DTEND must both be dates or both be times.".into());
                }
                let (start, timezone) = ics_date(start_prop)?;
                let (end, _) = ics_date(end_prop)?;
                let uid = unescape(value("UID"));
                if uid.is_empty() || uid.len() > 1000 || uid.chars().any(char::is_control) {
                    return Err("ICS events need a single-line UID of at most 1,000 bytes.".into());
                }
                use sha2::{Digest, Sha256};
                let mut event = checked(
                    NewEvent {
                        title: unescape(value("SUMMARY")),
                        start,
                        end,
                        timezone,
                        description: unescape(value("DESCRIPTION")),
                    },
                    format!("ics:{:x}", Sha256::digest(uid.as_bytes())),
                    "ics",
                )?;
                event.uid = uid;
                event.all_day = all_day;
                imported.push(event);
            }
        }
        if imported.is_empty() {
            return Err("No events were found in the ICS file.".into());
        }
        let _guard = WRITE.lock().unwrap_or_else(|p| p.into_inner());
        let mut events: Vec<CalendarEvent> = read_json(&self.path("agenda.json"))?;
        for event in &mut imported {
            if let Some(old) = events.iter().find(|old| old.uid == event.uid) {
                event.id = old.id.clone();
            }
            events.retain(|old| old.id != event.id && old.uid != event.uid);
            events.push(event.clone());
        }
        if events.len() > 2000 {
            return Err("The calendar can keep at most 2,000 events.".into());
        }
        write_json(&self.path("agenda.json"), &events)?;
        Ok(imported.len())
    }
    pub fn export_ics(&self) -> Result<String, String> {
        let events: Vec<CalendarEvent> = read_json(&self.path("agenda.json"))?;
        let mut lines = vec![
            "BEGIN:VCALENDAR".into(),
            "VERSION:2.0".into(),
            "PRODID:-//MonoBOT//Agenda//EN".into(),
        ];
        let stamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        for event in events {
            let date_line = |name: &str, value: &str| -> Result<String, String> {
                let at = instant(value)?;
                Ok(if event.all_day {
                    format!("{name};VALUE=DATE:{}", at.format("%Y%m%d"))
                } else {
                    format!("{name}:{}", at.with_timezone(&Utc).format("%Y%m%dT%H%M%SZ"))
                })
            };
            let start = date_line("DTSTART", &event.start)?;
            let end = date_line("DTEND", &event.end)?;
            lines.extend([
                "BEGIN:VEVENT".into(),
                format!(
                    "UID:{}",
                    escape(&if event.uid.is_empty() {
                        format!("{}@monobot.local", event.id)
                    } else {
                        event.uid
                    })
                ),
                format!("DTSTAMP:{stamp}"),
                start,
                end,
                format!("SUMMARY:{}", escape(&event.title)),
                format!("DESCRIPTION:{}", escape(&event.description)),
                "END:VEVENT".into(),
            ]);
        }
        lines.push("END:VCALENDAR".into());
        Ok(lines
            .iter()
            .map(|l| fold(l))
            .collect::<Vec<_>>()
            .join("\r\n")
            + "\r\n")
    }
}

/// A calendar date as an instant with its offset, its zone's name, and whether it is a whole
/// day (`VALUE=DATE`).
pub(super) fn ics_date_of(
    property: &ical::property::Property,
) -> Result<(DateTime<chrono::FixedOffset>, String, bool), String> {
    let all_day = property
        .value
        .as_deref()
        .is_some_and(|v| v.trim().len() == 8);
    let (start, timezone) = ics_date(property)?;
    Ok((instant(&start)?, timezone, all_day))
}

/// An event of a followed calendar, checked like the user's own but read only.
#[allow(clippy::too_many_arguments)]
pub(super) fn checked_feed(
    id: String,
    title: &str,
    description: &str,
    start: DateTime<chrono::FixedOffset>,
    end: DateTime<chrono::FixedOffset>,
    timezone: &str,
    all_day: bool,
    uid: &str,
) -> Result<CalendarEvent, String> {
    let title: String = title
        .chars()
        .filter(|c| !c.is_control())
        .take(500)
        .collect();
    let title = if title.trim().is_empty() {
        "(No title)".to_owned()
    } else {
        title.trim().to_owned()
    };
    let zone: Tz = timezone.parse().unwrap_or(chrono_tz::UTC);
    if end <= start {
        return Err("The event must end after it starts.".into());
    }
    Ok(CalendarEvent {
        id,
        title,
        start: start.with_timezone(&zone).to_rfc3339(),
        end: end.with_timezone(&zone).to_rfc3339(),
        timezone: zone.name().into(),
        description: description.chars().take(10_000).collect(),
        source: "feed".into(),
        uid: uid.chars().take(1_000).collect(),
        all_day,
    })
}

fn ics_date(property: &ical::property::Property) -> Result<(String, String), String> {
    let value = property.value.as_deref().ok_or("ICS date is empty.")?;
    let timezone = property.params.as_ref().and_then(|params| {
        params
            .iter()
            .find(|(key, _)| key == "TZID")
            .and_then(|(_, values)| values.first())
    });
    if value.ends_with('Z') {
        let at = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%SZ")
            .map_err(|_| "Invalid UTC ICS date.")?;
        return Ok((at.and_utc().to_rfc3339(), "UTC".into()));
    }
    if value.len() == 8 {
        let date =
            NaiveDate::parse_from_str(value, "%Y%m%d").map_err(|_| "Invalid all-day ICS date.")?;
        return Ok((
            date.and_hms_opt(0, 0, 0).unwrap().and_utc().to_rfc3339(),
            "UTC".into(),
        ));
    }
    let timezone: Tz = timezone
        .ok_or("Floating ICS times need an explicit TZID.")?
        .parse()
        .map_err(|_| "Unknown ICS time zone.")?;
    let naive =
        NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S").map_err(|_| "Invalid ICS date.")?;
    let at = timezone
        .from_local_datetime(&naive)
        .single()
        .ok_or("ICS time is ambiguous or does not exist during a daylight-saving change.")?;
    Ok((at.to_rfc3339(), timezone.name().into()))
}
fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\\n")
        .replace(';', "\\;")
        .replace(',', "\\,")
}
fn unescape(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(c) => out.push(c),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}
fn fold(value: &str) -> String {
    let mut out = String::new();
    let mut bytes = 0;
    for c in value.chars() {
        if bytes + c.len_utf8() > 75 {
            out.push_str("\r\n ");
            bytes = 1;
        }
        out.push(c);
        bytes += c.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_day_ics_keeps_date_values_and_rejects_mixed_date_types() {
        let dir = tempfile::tempdir().unwrap();
        let service = Service::new(dir.path());
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:holiday@example.com\r\nSUMMARY:Holiday\r\nDTSTART;VALUE=DATE:20261107\r\nDTEND;VALUE=DATE:20261109\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        service.import_ics(ics).unwrap();
        let output = service.export_ics().unwrap();
        assert!(output.contains("DTSTART;VALUE=DATE:20261107\r\n"));
        assert!(output.contains("DTEND;VALUE=DATE:20261109\r\n"));
        let mixed = ics.replace("DTEND;VALUE=DATE:20261109", "DTEND:20261109T120000Z");
        assert!(service.import_ics(&mixed).is_err());
        assert_eq!(
            service
                .agenda_with_remote("2026-11-01T00:00:00Z", "2026-11-10T00:00:00Z", false)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn agenda_uses_instants_and_roundtrips_utf8_ics_without_header_injection() {
        let dir = tempfile::tempdir().unwrap();
        let service = Service::new(dir.path());
        service
            .create_event(NewEvent {
                title: "Revisión, código; 東京 ".repeat(3),
                start: "2026-11-07T10:00:00+01:00".into(),
                end: "2026-11-07T11:00:00+01:00".into(),
                timezone: "Europe/Madrid".into(),
                description: "Text\nBEGIN:VEVENT".into(),
            })
            .unwrap();
        assert_eq!(
            service
                .agenda("2026-11-07T08:00:00Z", "2026-11-07T10:30:00Z")
                .unwrap()
                .len(),
            1
        );
        let ics = service.export_ics().unwrap();
        assert!(ics.split("\r\n").all(|line| line.len() <= 75));
        assert_eq!(service.import_ics(&ics).unwrap(), 1);
        assert_eq!(
            service
                .agenda("2026-11-07T08:00:00Z", "2026-11-07T10:30:00Z")
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            service.import_ics(&service.export_ics().unwrap()).unwrap(),
            1
        );
        assert_eq!(
            service
                .agenda("2026-11-07T08:00:00Z", "2026-11-07T10:30:00Z")
                .unwrap()
                .len(),
            1
        );
        assert!(range("2026-01-01T00:00:00Z", "2026-04-02T00:00:00Z").is_err());
    }
    #[test]
    fn rejects_dst_ambiguity_and_bad_dates_before_mutating() {
        let p = ical::property::Property {
            name: "DTSTART".into(),
            params: Some(vec![("TZID".into(), vec!["Europe/Madrid".into()])]),
            value: Some("20261025T023000".into()),
        };
        assert!(ics_date(&p).is_err());
        assert!(checked(
            NewEvent {
                title: "Review".into(),
                start: "2026-01-01".into(),
                end: "2026-01-02".into(),
                ..NewEvent::default()
            },
            "1".into(),
            "local"
        )
        .is_err());
    }

    #[test]
    fn an_agent_without_a_mailflare_grant_reads_only_the_local_agenda() {
        let dir = tempfile::tempdir().unwrap();
        let service = Service::new(dir.path());
        service
            .save_config(&super::super::Config {
                mailflare_url: "https://private-mail.example.com".into(),
                ..Default::default()
            })
            .unwrap();
        assert!(service
            .agenda_with_remote("2026-11-07T00:00:00Z", "2026-11-08T00:00:00Z", false)
            .unwrap()
            .is_empty());
    }
}
