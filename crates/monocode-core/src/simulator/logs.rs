//! The device's log as it happens, for the drawer under the device:
//! `simctl spawn <udid> log stream --style ndjson`, parsed into lines the
//! pane filters by level and by scope (the whole device, one process, one
//! subsystem). A busy simulator writes several hundred lines a second at the
//! info level, so lines arrive in batches.
use serde_json::Value;
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

/// The levels `log` knows, quietest first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Debug,
    #[default]
    Info,
    /// What `os_log` writes without a level ("Notice" in Console).
    Default,
    Error,
    Fault,
}

impl Level {
    pub const ALL: [Self; 5] = [
        Self::Debug,
        Self::Info,
        Self::Default,
        Self::Error,
        Self::Fault,
    ];

    fn from_message_type(kind: &str) -> Self {
        match kind {
            "Debug" => Self::Debug,
            "Info" => Self::Info,
            "Error" => Self::Error,
            "Fault" => Self::Fault,
            _ => Self::Default,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Debug => "Debug",
            Self::Info => "Info",
            Self::Default => "Notice",
            Self::Error => "Error",
            Self::Fault => "Fault",
        }
    }

    /// `log stream --level` takes debug, info or default.
    fn stream_flag(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            _ => "default",
        }
    }
}

/// One log message.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// "19:16:57.007", the device's local time.
    pub time: String,
    pub level: Level,
    /// The process's executable name.
    pub process: String,
    pub pid: u32,
    pub subsystem: String,
    pub category: String,
    pub message: String,
}

/// One NDJSON line from `log stream`; `None` for anything that is not a log
/// event (the header, activity and signpost events).
pub fn parse(line: &str) -> Option<Line> {
    let event: Value = serde_json::from_str(line.trim()).ok()?;
    if event["eventType"].as_str()? != "logEvent" {
        return None;
    }
    let string = |key: &str| event[key].as_str().unwrap_or_default().to_owned();
    // "2026-11-18 19:16:57.007038-0500" → "19:16:57.007".
    let time = event["timestamp"]
        .as_str()
        .and_then(|stamp| stamp.split(' ').nth(1))
        .map(|clock| clock.get(..12).unwrap_or(clock).to_owned())
        .unwrap_or_default();
    let process = event["processImagePath"]
        .as_str()
        .and_then(|path| path.rsplit('/').next())
        .unwrap_or_default()
        .to_owned();
    Some(Line {
        time,
        level: Level::from_message_type(event["messageType"].as_str().unwrap_or_default()),
        process,
        pid: event["processID"].as_u64().unwrap_or_default() as u32,
        subsystem: string("subsystem"),
        category: string("category"),
        message: string("eventMessage"),
    })
}

/// Which messages the drawer shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    /// Everything on the device.
    #[default]
    Device,
    /// One process, by executable name: the app being worked on.
    Process(String),
    /// One subsystem, such as the app's bundle id.
    Subsystem(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    /// The quietest level shown.
    pub level: Level,
    pub scope: Scope,
    /// Shown only when the message, process or subsystem contains it,
    /// ignoring case.
    pub text: String,
}

impl Filter {
    pub fn admits(&self, line: &Line) -> bool {
        if line.level < self.level {
            return false;
        }
        let scoped = match &self.scope {
            Scope::Device => true,
            Scope::Process(name) => line.process == *name,
            Scope::Subsystem(name) => line.subsystem == *name,
        };
        if !scoped {
            return false;
        }
        let text = self.text.trim();
        if text.is_empty() {
            return true;
        }
        let text = text.to_lowercase();
        [
            &line.message,
            &line.process,
            &line.subsystem,
            &line.category,
        ]
        .iter()
        .any(|field| field.to_lowercase().contains(&text))
    }
}

/// How often lines are handed over, at most.
const BATCH_EVERY: Duration = Duration::from_millis(150);

/// A running `log stream`; dropping it ends the stream.
pub struct LogStream {
    child: Child,
}

impl LogStream {
    /// Streams the device's log from `level` up. `on_lines` gets each batch
    /// on a reader thread and returns false to stop.
    pub fn start(
        udid: &str,
        level: Level,
        mut on_lines: impl FnMut(Vec<Line>) -> bool + Send + 'static,
    ) -> Result<Self, String> {
        let mut child = Command::new(super::XCRUN)
            .args([
                "simctl", "spawn", udid, "log", "stream", "--style", "ndjson",
            ])
            .args(["--level", level.stream_flag()])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("log stream: {e}"))?;
        let stdout = child.stdout.take().ok_or("log stream has no output")?;
        std::thread::spawn(move || {
            let mut batch = Vec::new();
            let mut sent = Instant::now();
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Some(line) = parse(&line) {
                    batch.push(line);
                }
                if !batch.is_empty() && (sent.elapsed() >= BATCH_EVERY || batch.len() >= 500) {
                    if !on_lines(std::mem::take(&mut batch)) {
                        return;
                    }
                    sent = Instant::now();
                }
            }
            if !batch.is_empty() {
                on_lines(batch);
            }
        });
        Ok(Self { child })
    }
}

impl Drop for LogStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EVENT: &str = r#"{"messageType":"Error","eventType":"logEvent","subsystem":"com.example.app","category":"network","processImagePath":"\/Users\/x\/Library\/Developer\/CoreSimulator\/Devices\/A\/data\/Containers\/Bundle\/Application\/B\/Example.app\/Example","timestamp":"2026-11-18 19:16:57.007038-0500","eventMessage":"Request failed: 500","processID":2263}"#;

    #[test]
    fn a_log_event_becomes_a_line() {
        let line = parse(EVENT).unwrap();
        assert_eq!(line.time, "19:16:57.007");
        assert_eq!(line.level, Level::Error);
        assert_eq!(line.process, "Example");
        assert_eq!(line.pid, 2263);
        assert_eq!(line.subsystem, "com.example.app");
        assert_eq!(line.category, "network");
        assert_eq!(line.message, "Request failed: 500");
        assert!(parse("getpwuid_r did not find a match for uid 501").is_none());
        assert!(parse(r#"{"eventType":"activityCreateEvent"}"#).is_none());
        assert_eq!(
            parse(&EVENT.replace("\"Error\"", "\"Default\""))
                .unwrap()
                .level,
            Level::Default
        );
    }

    #[test]
    fn filters_go_by_level_scope_and_text() {
        let line = parse(EVENT).unwrap();
        let mut filter = Filter::default();
        assert!(filter.admits(&line));
        filter.level = Level::Fault;
        assert!(!filter.admits(&line));
        filter.level = Level::Error;
        filter.scope = Scope::Process("Example".into());
        assert!(filter.admits(&line));
        filter.scope = Scope::Process("SpringBoard".into());
        assert!(!filter.admits(&line));
        filter.scope = Scope::Subsystem("com.example.app".into());
        assert!(filter.admits(&line));
        filter.text = "  REQUEST ".into();
        assert!(filter.admits(&line));
        filter.text = "timeout".into();
        assert!(!filter.admits(&line));
        assert!(Level::Debug < Level::Info && Level::Default < Level::Error);
    }
}
