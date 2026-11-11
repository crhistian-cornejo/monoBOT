//! Automatic retry of a turn the provider failed with a transient error.
//!
//! Port of `src/features/sessions/model/providerRetry.ts`. An overloaded
//! provider (529, 429), a 5xx or a dropped connection ends the turn with an
//! error; this module decides whether to try again, when, and what the notice
//! above the composer says. State lives in memory only: the UI keeps one
//! `Option<ProviderRetry>` per session.
//!
//! Typical flow for the UI:
//! 1. `provider::run` returns `Err(message)` (or an `Event::Status` carries the
//!    error text): call [`next`] with the session facts in [`Session`].
//! 2. Show [`title`] and [`status`] (refresh every second) in the notice.
//! 3. On a timer, when [`due`] is true, resend the turn and store
//!    [`ProviderRetry::sending`] as the state.
//! 4. When that turn ends without an error and [`succeeded`] is true, clear it.
//!
//! Usage, credit and spending limits ([`crate::usage::Limit::from_error`]) are
//! never retried; they keep their own notice.
//!
//! A turn the loop guard stopped ([`crate::loop_guard::LOOPING`]) gets the
//! same notice without a countdown: sending it again by itself would most
//! likely go round the same circle, but one click carries on, and tells the
//! agent to read the last result and try another way.

use regex::Regex;
use std::sync::OnceLock;

/// Automatic retries after the failed turn, before giving up.
pub const MAX_RETRIES: u32 = 4;

/// Up to this fraction is added to each wait, so sessions of one account that
/// failed together don't all retry in the same second.
const JITTER: f64 = 0.25;

/// Why the provider failed a turn in a way that's worth retrying.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reason {
    Overloaded,
    Server,
    Connection,
    /// The loop guard stopped the turn: carried on only when the user asks.
    Looping,
}

impl Reason {
    /// The first wait; each later retry waits twice as long as the one before.
    fn first_wait_ms(self) -> u64 {
        match self {
            Reason::Overloaded | Reason::Server => 60_000,
            Reason::Connection => 15_000,
            Reason::Looping => 0,
        }
    }
}

/// What a retry sends, after a transient failure or a usage limit.
pub const CONTINUE_PROMPT: &str = "Continue from where you left off.";
/// What carrying on after the loop guard sends.
const UNSTUCK_PROMPT: &str = "Continue from where you left off. Before, you made the same call and got the same result again and again: read that result, then take a different approach, or finish and report if the work is done.";

/// The provider failed the last turn with a transient error.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProviderRetry {
    pub reason: Reason,
    /// Failed attempts at this turn, the original one included.
    pub attempt: u32,
    /// Epoch ms of the next automatic retry. `None` when it won't retry by itself.
    pub retry_at: Option<u64>,
    /// A retry was sent; clear this once that turn ends without an error.
    pub sending: bool,
}

/// What the retry decision needs to know about the session.
#[derive(Clone, Debug, Default)]
pub struct Session<'a> {
    /// The session already holds a usage-limit notice.
    pub usage_limited: bool,
    /// The session is a scheduled automation run (they come back on their own).
    pub automation: bool,
    /// The session is a Mono habit run that disappears when it ends.
    pub ephemeral: bool,
    /// A turn is running.
    pub busy: bool,
    /// The current retry state, if any.
    pub retry: Option<&'a ProviderRetry>,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("provider retry pattern"))
}

/// A transient failure worth retrying. Usage, credit and spending limits are
/// not: waiting a few minutes won't lift them, and they have their own notice.
pub fn reason(message: &str) -> Option<Reason> {
    static OVERLOADED: OnceLock<Regex> = OnceLock::new();
    static SERVER: OnceLock<Regex> = OnceLock::new();
    static CONNECTION: OnceLock<Regex> = OnceLock::new();
    if message.starts_with(crate::loop_guard::LOOPING) {
        return Some(Reason::Looping);
    }
    if crate::usage::Limit::from_error(message).is_some() {
        return None;
    }
    if re(
        &OVERLOADED,
        r"(?i)overloaded|\b529\b|high demand|at capacity|capacity constraints|too many requests|\b429\b|rate[_ ]limit(?:ed|_error)?\b",
    )
    .is_match(message)
    {
        return Some(Reason::Overloaded);
    }
    if re(
        &SERVER,
        r"(?i)\b(?:api error|http|status(?: code)?|error)\W{0,3}5\d\d\b|internal server error|bad gateway|service unavailable|gateway time-?out|\bserver_error\b|\bapi_error\b",
    )
    .is_match(message)
    {
        return Some(Reason::Server);
    }
    if re(
        &CONNECTION,
        r"(?i)ECONNRESET|ECONNREFUSED|ETIMEDOUT|ENOTFOUND|EAI_AGAIN|EPIPE|socket hang up|network error|fetch failed|connection (?:error|reset|refused|closed|lost)|stream disconnected|error sending request|request timed out",
    )
    .is_match(message)
    {
        return Some(Reason::Connection);
    }
    None
}

/// How long to wait (ms) before retry number `retry` (1-based); `random` in [0, 1).
pub fn wait_ms(reason: Reason, retry: u32, random: f64) -> u64 {
    let exponent = retry.saturating_sub(1).min(32);
    let wait = reason.first_wait_ms() as f64 * 2f64.powi(exponent as i32);
    (wait * (1.0 + JITTER * random)).round() as u64
}

/// The retry state after the provider failed the turn with `message`, or
/// `None` when the failure isn't retryable (clear any previous state).
/// `random` in [0, 1) spreads the wait; pass `rand::random::<f64>()`.
pub fn next(session: &Session, message: &str, now: u64, random: f64) -> Option<ProviderRetry> {
    let reason = reason(message)?;
    if session.usage_limited {
        return None;
    }
    if let Some(previous) = session.retry {
        // The same failed turn reporting a second error: it isn't another attempt.
        if !previous.sending {
            return Some(previous.clone());
        }
    }
    let attempt = session.retry.map_or(0, |r| r.attempt) + 1;
    // Scheduled runs come back on their own schedule; don't add turns to them.
    let automatic = attempt <= MAX_RETRIES
        && !session.automation
        && !session.ephemeral
        && reason != Reason::Looping;
    Some(ProviderRetry {
        reason,
        attempt,
        retry_at: automatic.then(|| now + wait_ms(reason, attempt, random)),
        sending: false,
    })
}

/// Idle and past the wait: time to send the retry.
pub fn due(session: &Session, now: u64) -> bool {
    session.retry.is_some_and(|r| {
        r.retry_at
            .is_some_and(|at| !r.sending && !session.busy && now >= at)
    })
}

impl ProviderRetry {
    /// What the retry turn carries, so a further failure counts as the next attempt.
    pub fn sending(&self) -> ProviderRetry {
        ProviderRetry {
            reason: self.reason,
            attempt: self.attempt,
            retry_at: None,
            sending: true,
        }
    }
}

/// The retried turn ended without an error.
pub fn succeeded(session: &Session) -> bool {
    session.retry.is_some_and(|r| r.sending) && !session.busy
}

/// Heading of the notice.
pub fn title(retry: &ProviderRetry) -> &'static str {
    match retry.reason {
        Reason::Overloaded => "Provider is overloaded",
        Reason::Server => "Provider error",
        Reason::Connection => "Connection lost",
        Reason::Looping => "Stopped going in circles",
    }
}

/// The notice's button.
pub fn action(retry: &ProviderRetry) -> &'static str {
    match retry.reason {
        Reason::Looping => "Continue",
        _ => "Retry now",
    }
}

/// What the button, or the countdown, sends.
pub fn prompt(retry: &ProviderRetry) -> &'static str {
    match retry.reason {
        Reason::Looping => UNSTUCK_PROMPT,
        _ => CONTINUE_PROMPT,
    }
}

/// "Retry 2 of 4 in 1m 30s", or why it won't retry by itself.
pub fn status(retry: &ProviderRetry, now: u64) -> String {
    if retry.reason == Reason::Looping {
        return "The agent repeated a call that kept giving the same result".into();
    }
    let Some(at) = retry.retry_at else {
        return if retry.attempt > MAX_RETRIES {
            format!("Gave up after {MAX_RETRIES} retries")
        } else {
            "Won't retry automatically".into()
        };
    };
    let counter = format!("Retry {} of {MAX_RETRIES}", retry.attempt);
    if at > now {
        format!("{counter} in {}", format_wait(at - now))
    } else {
        format!("{counter} now")
    }
}

fn format_wait(ms: u64) -> String {
    let seconds = ms.div_ceil(1000);
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let (minutes, rest) = (seconds / 60, seconds % 60);
    if rest > 0 {
        format!("{minutes}m {rest}s")
    } else {
        format!("{minutes}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn retry(reason: Reason, attempt: u32, retry_at: Option<u64>, sending: bool) -> ProviderRetry {
        ProviderRetry {
            reason,
            attempt,
            retry_at,
            sending,
        }
    }

    #[test]
    fn retries_overloads_server_errors_and_dropped_connections() {
        assert_eq!(
            reason(
                r#"API Error: 529 {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#
            ),
            Some(Reason::Overloaded)
        );
        assert_eq!(
            reason("We're currently experiencing high demand"),
            Some(Reason::Overloaded)
        );
        assert_eq!(
            reason("API Error: 500 Internal server error"),
            Some(Reason::Server)
        );
        assert_eq!(
            reason("unexpected status 503 Service Unavailable"),
            Some(Reason::Server)
        );
        assert_eq!(reason("HTTP 502 Bad Gateway"), Some(Reason::Server));
        assert_eq!(reason("read ECONNRESET"), Some(Reason::Connection));
        assert_eq!(
            reason("stream disconnected before completion"),
            Some(Reason::Connection)
        );
        assert_eq!(reason("Request timed out."), Some(Reason::Connection));
    }

    #[test]
    fn never_retries_limits_auth_or_the_users_own_errors() {
        assert_eq!(reason("Claude AI usage limit reached|1760000000"), None);
        assert_eq!(reason("Your credit balance is too low"), None);
        assert_eq!(reason("insufficient_quota"), None);
        assert_eq!(reason("API Error: 401 invalid x-api-key"), None);
        assert_eq!(reason("Prompt is too long"), None);
        assert_eq!(reason("Listening on port 5000"), None);
    }

    #[test]
    fn doubles_each_time_starting_at_a_minute_for_server_errors() {
        let waits: Vec<u64> = (1..=4).map(|n| wait_ms(Reason::Server, n, 0.0)).collect();
        assert_eq!(waits, [60_000, 120_000, 240_000, 480_000]);
        assert_eq!(wait_ms(Reason::Connection, 1, 0.0), 15_000);
    }

    #[test]
    fn adds_up_to_a_quarter_at_random() {
        assert!(wait_ms(Reason::Server, 1, 0.999) < 75_000);
        assert_eq!(wait_ms(Reason::Server, 1, 0.5), 67_500);
    }

    #[test]
    fn schedules_the_first_retry() {
        assert_eq!(
            next(&Session::default(), "API Error: 529 Overloaded", 1_000, 0.0),
            Some(retry(Reason::Overloaded, 1, Some(61_000), false))
        );
    }

    #[test]
    fn counts_a_failed_retry_as_the_next_attempt() {
        let sending = retry(Reason::Server, 2, None, true);
        let session = Session {
            retry: Some(&sending),
            ..Default::default()
        };
        assert_eq!(
            next(&session, "HTTP 502 Bad Gateway", 0, 0.0),
            Some(retry(Reason::Server, 3, Some(240_000), false))
        );
    }

    #[test]
    fn doesnt_count_a_second_error_from_the_same_failed_turn() {
        let failed = retry(Reason::Server, 1, Some(60_000), false);
        let session = Session {
            retry: Some(&failed),
            ..Default::default()
        };
        assert_eq!(
            next(&session, "read ECONNRESET", 5_000, 0.0),
            Some(failed.clone())
        );
    }

    #[test]
    fn stops_retrying_by_itself_after_the_last_attempt() {
        let last = retry(Reason::Server, MAX_RETRIES, None, true);
        let session = Session {
            retry: Some(&last),
            ..Default::default()
        };
        assert_eq!(
            next(&session, "HTTP 500", 0, 0.0),
            Some(retry(Reason::Server, MAX_RETRIES + 1, None, false))
        );
    }

    #[test]
    fn leaves_scheduled_runs_and_usage_limits_alone() {
        let automation = Session {
            automation: true,
            ..Default::default()
        };
        assert_eq!(
            next(&automation, "HTTP 500", 0, 0.0),
            Some(retry(Reason::Server, 1, None, false))
        );
        let ephemeral = Session {
            ephemeral: true,
            ..Default::default()
        };
        assert_eq!(next(&ephemeral, "HTTP 500", 0, 0.0).unwrap().retry_at, None);
        let limited = Session {
            usage_limited: true,
            ..Default::default()
        };
        assert_eq!(next(&limited, "HTTP 500", 0, 0.0), None);
    }

    #[test]
    fn clears_the_retry_when_the_turn_fails_for_another_reason() {
        let sending = retry(Reason::Server, 1, None, true);
        let session = Session {
            retry: Some(&sending),
            ..Default::default()
        };
        assert_eq!(next(&session, "Prompt is too long", 0, 0.0), None);
    }

    #[test]
    fn is_due_once_idle_and_past_the_wait() {
        let waiting = retry(Reason::Server, 1, Some(60_000), false);
        let idle = Session {
            retry: Some(&waiting),
            ..Default::default()
        };
        assert!(!due(&idle, 59_999));
        assert!(due(&idle, 60_000));
        let busy = Session {
            retry: Some(&waiting),
            busy: true,
            ..Default::default()
        };
        assert!(!due(&busy, 60_000));
        let gave_up = retry(Reason::Server, 5, None, false);
        let session = Session {
            retry: Some(&gave_up),
            ..Default::default()
        };
        assert!(!due(&session, u64::MAX));
    }

    #[test]
    fn succeeds_when_the_retried_turn_ends_without_an_error() {
        let sent = retry(Reason::Server, 2, Some(1), false).sending();
        assert_eq!(sent, retry(Reason::Server, 2, None, true));
        let busy = Session {
            retry: Some(&sent),
            busy: true,
            ..Default::default()
        };
        assert!(!succeeded(&busy));
        let idle = Session {
            retry: Some(&sent),
            ..Default::default()
        };
        assert!(succeeded(&idle));
        assert!(!due(&idle, u64::MAX));
    }

    #[test]
    fn status_says_when_and_which_retry_comes_next() {
        let r = retry(Reason::Server, 2, Some(95_000), false);
        assert_eq!(status(&r, 0), "Retry 2 of 4 in 1m 35s");
        assert_eq!(status(&r, 60_000), "Retry 2 of 4 in 35s");
        assert_eq!(status(&r, 95_000), "Retry 2 of 4 now");
    }

    #[test]
    fn status_says_why_it_wont_retry_by_itself() {
        assert_eq!(
            status(&retry(Reason::Server, MAX_RETRIES + 1, None, false), 0),
            "Gave up after 4 retries"
        );
        assert_eq!(
            status(&retry(Reason::Server, 1, None, false), 0),
            "Won't retry automatically"
        );
        assert_eq!(
            title(&retry(Reason::Connection, 1, None, false)),
            "Connection lost"
        );
    }

    #[test]
    fn a_turn_stopped_going_in_circles_waits_for_the_user_to_continue() {
        let message = crate::loop_guard::LOOPING;
        assert_eq!(reason(message), Some(Reason::Looping));
        assert_eq!(
            reason(&format!("{message} · Saving failed: disk full")),
            Some(Reason::Looping)
        );
        let stopped = next(&Session::default(), message, 0, 0.0).unwrap();
        assert_eq!(stopped, retry(Reason::Looping, 1, None, false));
        assert!(!due(
            &Session {
                retry: Some(&stopped),
                ..Default::default()
            },
            u64::MAX
        ));
        assert_eq!(action(&stopped), "Continue");
        assert!(prompt(&stopped).contains("take a different approach"));
        let server = retry(Reason::Server, 1, None, false);
        assert_eq!(
            (action(&server), prompt(&server)),
            ("Retry now", CONTINUE_PROMPT)
        );
    }
}
