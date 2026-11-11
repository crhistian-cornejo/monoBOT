//! Subscription usage, ported from rateLimits.ts and src-tauri/rate_limits.rs.
//! Blocking probes belong on a worker. Credentials are read only, never rotated.
pub(crate) mod claude;
mod devin;
pub(crate) mod kimi;
mod opencode;
mod pi;
pub mod resets;

use crate::{catalog, provider::Kind};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Window {
    pub used_percent: f64,
    pub minutes: u64,
    pub resets_at: Option<i64>,
}
impl Window {
    pub fn label(&self) -> String {
        match self.minutes {
            300 => "5h".into(),
            1_440 => "1d".into(),
            10_080 => "wk".into(),
            43_200 => "mo".into(),
            n if n % 60 == 0 => format!("{}h", n / 60),
            n => format!("{n}m"),
        }
    }
    pub fn countdown(&self, now: i64) -> String {
        self.resets_at
            .map(|at| duration(at.saturating_sub(now)))
            .unwrap_or_else(|| self.label())
    }
    pub fn reset_label(&self, now: i64) -> String {
        match self.resets_at {
            Some(at) if at <= now => "Resets now".into(),
            Some(at) => format!("Resets in {}", duration(at - now)),
            None => format!("{} window", self.label()),
        }
    }
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    pub windows: Vec<Window>,
    pub updated_at: i64,
    #[serde(default)]
    pub reset_credits: Option<ResetCredits>,
    #[serde(default)]
    pub extra_usage_balance: Option<f64>,
    #[serde(default)]
    pub identity: Option<crate::accounts::Identity>,
    /// Limits of one model's own, such as Claude's weekly limit for Fable.
    /// Kept apart from `windows`: reaching one stops only that model, so it
    /// never counts toward the account's headroom.
    #[serde(default)]
    pub models: Vec<ModelWindow>,
}
/// A limit that applies to one model only.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelWindow {
    /// The model's name as the provider shows it ("Fable").
    pub model: String,
    pub window: Window,
}
/// The limit resets an account holds: Codex's banked resets, Claude's reset
/// grants (5-hour, weekly, Fable, full) and Claude's weekly 5-hour reset.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ResetCredits {
    pub available_count: u64,
    pub credits: Vec<ResetCredit>,
    /// How much of each limit is used, by the provider's key for it
    /// (`five_hour`, `seven_day`, `seven_day_overage_included`…), where the
    /// provider says (Claude); the windows say the rest.
    #[serde(default)]
    pub used: std::collections::BTreeMap<String, f64>,
    /// The limits reached, by key.
    #[serde(default)]
    pub exhausted: Vec<String>,
}
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ResetCredit {
    pub id: String,
    pub status: String,
    pub reset_type: String,
    pub granted_at: Option<i64>,
    pub expires_at: Option<i64>,
    pub title: Option<String>,
    pub description: Option<String>,
    /// The limits it refills, by key; empty for all of them (Codex).
    #[serde(default)]
    pub clears: Vec<String>,
    /// How many uses it has left, where one credit holds several (Claude).
    #[serde(default)]
    pub left: Option<u64>,
    /// Whether it can only be used once a limit is reached.
    #[serde(default)]
    pub needs_limit: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Limit {
    pub resets_at: Option<i64>,
    pub resume_at_reset: bool,
}
impl Limit {
    /// Manual continuation becomes available when the reported reset passes.
    pub fn waiting(&self, now: i64) -> bool {
        self.resets_at.is_some_and(|at| at > now)
    }
    pub fn from_error(message: &str) -> Option<Self> {
        // Claude's synthetic assistant errors use this code before the result
        // frame arrives. Ordinary transient HTTP 429s must remain retryable.
        if message.trim().eq_ignore_ascii_case("rate_limit") {
            return Some(Self::default());
        }
        static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let pattern=PATTERN.get_or_init(||regex::Regex::new(r"(?i)(?:usage|spending|monthly|weekly|daily)[ _]limit[ _](?:reached|exceeded)|(?:quota|credits?) (?:exceeded|exhausted|depleted)|insufficient[_ ](?:quota|credits)|hit your (?:(?:usage|session) )?limit|credit balance is too low|reached (?:its|your) usage limit").unwrap());
        pattern.is_match(message).then(Self::default)
    }
    /// Extra-usage refusals fall back to the subscription and do not stop a turn.
    pub fn from_claude_info(info: &Value) -> Option<Self> {
        (info["status"] == "rejected"
            && info["isUsingOverage"] != true
            && info["rateLimitType"] != "overage")
            .then(|| Self {
                resets_at: timestamp(&info["resetsAt"]),
                resume_at_reset: false,
            })
    }
    pub fn resume_due(&self, now: i64, busy: bool) -> bool {
        !busy
            && self.resume_at_reset
            && self
                .resets_at
                .is_some_and(|at| now >= at.saturating_add(30_000))
    }
}
#[derive(Clone, Debug)]
pub struct FetchError {
    pub message: String,
    pub unavailable: bool,
}
impl FetchError {
    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            unavailable: true,
        }
    }
    fn failed(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            unavailable: false,
        }
    }
}

pub fn fetch(kind: Kind) -> Result<Snapshot, FetchError> {
    fetch_for(kind, &Default::default(), None)
}
pub fn fetch_for(
    kind: Kind,
    profile: &crate::accounts::Profile,
    model: Option<&str>,
) -> Result<Snapshot, FetchError> {
    if kind == Kind::Devin {
        return fetch_devin();
    }
    if kind == Kind::OpenCode {
        return fetch_opencode();
    }
    if kind == Kind::Pi {
        return fetch_pi(model);
    }
    if kind == Kind::Kimi {
        return kimi::fetch_for(profile);
    }
    if !kind.subscription_usage() {
        return Err(FetchError::unavailable(format!(
            "No subscription usage is available for {}.",
            kind.label()
        )));
    }
    if kind == Kind::Claude {
        return claude::fetch_for(profile);
    }
    let value = catalog::codex_request(profile, "account/rateLimits/read", serde_json::json!({}))
        .map_err(|message| {
        let lower = message.to_lowercase();
        if [
            "not installed",
            "not signed in",
            "not authenticated",
            "authentication required",
        ]
        .iter()
        .any(|s| lower.contains(s))
        {
            FetchError::unavailable("Sign in with codex login, then refresh usage.")
        } else {
            FetchError::failed(message)
        }
    })?;
    let snapshot = parse_codex(&value);
    if snapshot.windows.is_empty() && snapshot.reset_credits.is_none() {
        return Err(FetchError::unavailable(
            "No Codex subscription usage available for this account.",
        ));
    }
    Ok(snapshot)
}

fn fetch_devin() -> Result<Snapshot, FetchError> {
    let version =
        crate::accounts::login::cli_output(Kind::Devin, &["--version"], &Default::default())
            .map_err(FetchError::unavailable)?;
    let version = regex::Regex::new(r"\d+(?:\.\d+)+")
        .unwrap()
        .find(&version)
        .ok_or_else(|| FetchError::failed("Devin CLI version unknown"))?
        .as_str();
    let result =
        devin::fetch_sync(env!("CARGO_PKG_VERSION"), version).map_err(FetchError::failed)?;
    let Some(usage) = result.usage else {
        let message = result
            .error
            .unwrap_or_else(|| "Devin usage unavailable".into());
        return Err(if result.status == "unavailable" {
            FetchError::unavailable(message)
        } else {
            FetchError::failed(message)
        });
    };
    let mut windows = Vec::new();
    for (remaining, resets_at, minutes) in [
        (usage.daily_remaining_percent, usage.daily_resets_at, 1440),
        (
            usage.weekly_remaining_percent,
            usage.weekly_resets_at,
            10080,
        ),
    ] {
        if let Some(remaining) = remaining.filter(|n| n.is_finite()) {
            windows.push(Window {
                used_percent: (100. - remaining).clamp(0., 100.),
                minutes,
                resets_at,
            });
        }
    }
    Ok(Snapshot {
        windows,
        updated_at: now_ms(),
        extra_usage_balance: usage
            .extra_usage_balance_micros
            .map(|n| n as f64 / 1_000_000.),
        identity: Some(crate::accounts::Identity {
            name: usage.name,
            email: usage.email,
            plan: usage.plan,
            organization: None,
        }),
        ..Default::default()
    })
}
fn fetch_opencode() -> Result<Snapshot, FetchError> {
    let result = opencode::fetch().map_err(FetchError::failed)?;
    if result.status != "ok" {
        let message = result
            .error
            .unwrap_or_else(|| "OpenCode Go usage unavailable".into());
        return Err(if result.status == "unavailable" {
            FetchError::unavailable(message)
        } else {
            FetchError::failed(message)
        });
    }
    let value: Value = serde_json::from_str(result.body.as_deref().unwrap_or(""))
        .map_err(|_| FetchError::failed("OpenCode Go response was not JSON"))?;
    let snapshot = parse_opencode(&value);
    if snapshot.windows.is_empty() {
        return Err(FetchError::failed(
            "OpenCode Go usage response was unexpected",
        ));
    }
    Ok(snapshot)
}
fn parse_opencode(value: &Value) -> Snapshot {
    let value = value.get("usage").unwrap_or(value);
    Snapshot {
        windows: [("rolling", 300), ("weekly", 10080), ("monthly", 43200)]
            .into_iter()
            .filter_map(|(key, minutes)| {
                let raw = &value[key];
                if !matches!(raw["status"].as_str(), Some("ok" | "rate-limited")) {
                    return None;
                }
                window(raw, minutes, &["percent", "usedPercent"])
            })
            .collect(),
        updated_at: now_ms(),
        ..Default::default()
    }
}
fn fetch_pi(model: Option<&str>) -> Result<Snapshot, FetchError> {
    let model = model.unwrap_or("").trim_start_matches("pi:");
    let billing = if model.starts_with("anthropic/") {
        pi::PiUsageProvider::Anthropic
    } else if model.starts_with("openai-codex/") {
        pi::PiUsageProvider::OpenaiCodex
    } else {
        return Err(FetchError::unavailable(
            "Subscription usage is unavailable for this Pi model",
        ));
    };
    match pi::fetch_pi_usage(billing) {
        pi::PiUsageResult::Ok { windows } => Ok(Snapshot {
            windows: [windows.session, windows.weekly]
                .into_iter()
                .flatten()
                .map(|w| Window {
                    used_percent: w.used_percent,
                    minutes: w.window_minutes,
                    resets_at: w.resets_at.map(|n| n as i64),
                })
                .collect(),
            updated_at: now_ms(),
            ..Default::default()
        }),
        pi::PiUsageResult::Unavailable { message } => Err(FetchError::unavailable(message)),
        pi::PiUsageResult::Error { message } => Err(FetchError::failed(message)),
    }
}
/// Uses `credit` on the account's provider: Codex's banked reset through
/// its app server, Claude's through its own reset endpoint. The result is
/// the outcome Codex names (`reset`, `nothingToReset`, `noCredit`,
/// `alreadyRedeemed`), which Claude's answers map onto.
pub fn use_reset(
    kind: Kind,
    profile: &crate::accounts::Profile,
    credit: Option<&ResetCredit>,
) -> Result<String, String> {
    match kind {
        Kind::Claude => {
            let credit = credit.ok_or("Choose which Claude reset to use.")?;
            claude::claim(profile, credit)
        }
        Kind::Codex => consume_reset(
            profile,
            credit
                .map(|credit| credit.id.as_str())
                .filter(|id| !id.is_empty()),
        ),
        _ => Err(format!("{} has no limit resets.", kind.label())),
    }
}
pub fn consume_reset(
    profile: &crate::accounts::Profile,
    credit: Option<&str>,
) -> Result<String, String> {
    let mut params = serde_json::json!({"idempotencyKey": crate::accounts::request_id()});
    if let Some(credit) = credit {
        params["creditId"] = credit.into();
    }
    let result = catalog::codex_request(profile, "account/rateLimitResetCredit/consume", params)?;
    match result["outcome"].as_str() {
        Some(outcome @ ("reset" | "nothingToReset" | "noCredit" | "alreadyRedeemed")) => {
            Ok(outcome.into())
        }
        _ => Err("Codex returned an unknown reset result".into()),
    }
}
impl Snapshot {
    pub fn headroom(&self, now: i64) -> Option<f64> {
        self.windows
            .iter()
            .map(|w| {
                if w.resets_at.is_some_and(|at| at <= now) {
                    100.
                } else {
                    100. - w.used_percent.clamp(0., 100.)
                }
            })
            .reduce(f64::min)
    }
    pub fn exhausted_reset(&self) -> Option<i64> {
        let exhausted: Vec<_> = self
            .windows
            .iter()
            .filter(|w| w.used_percent >= 100.)
            .collect();
        if exhausted.is_empty() || exhausted.iter().any(|w| w.resets_at.is_none()) {
            None
        } else {
            exhausted.iter().filter_map(|w| w.resets_at).max()
        }
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}
pub fn duration(ms: i64) -> String {
    if ms <= 0 {
        return "now".into();
    }
    let minutes = ms / 60_000;
    let hours = minutes / 60;
    if hours >= 24 {
        let days = hours / 24;
        let rest = hours % 24;
        if rest > 0 {
            format!("{days}d {rest}h")
        } else {
            format!("{days}d")
        }
    } else if hours > 0 {
        let rest = minutes % 60;
        if rest > 0 {
            format!("{hours}h {rest}m")
        } else {
            format!("{hours}h")
        }
    } else {
        format!("{minutes}m")
    }
}
fn number(value: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| value[*key].as_f64().filter(|v| v.is_finite()))
}
fn timestamp(value: &Value) -> Option<i64> {
    let numeric = value
        .as_f64()
        .or_else(|| value.as_str()?.parse::<f64>().ok());
    if let Some(n) = numeric.filter(|n| n.is_finite() && *n > 0.) {
        return Some(if n > 10_000_000_000. { n } else { n * 1000. } as i64);
    }
    chrono::DateTime::parse_from_rfc3339(value.as_str()?)
        .ok()
        .map(|d| d.timestamp_millis())
}
fn window(value: &Value, minutes: u64, percent_keys: &[&str]) -> Option<Window> {
    Some(Window {
        used_percent: number(value, percent_keys)?.clamp(0., 100.),
        minutes,
        resets_at: timestamp(&value["resetsAt"]).or_else(|| timestamp(&value["resets_at"])),
    })
}
fn parse_codex(value: &Value) -> Snapshot {
    let wrapper = value.get("rateLimits").unwrap_or(value);
    let mut windows: Vec<Window> = Vec::new();
    // An explicitly identified window always wins over a positional fallback.
    for fallback_pass in [false, true] {
        for (key, fallback) in [("primary", 300), ("secondary", 10_080)] {
            let raw = &wrapper[key];
            let classified =
                number(raw, &["windowDurationMins", "window_duration_mins"]).and_then(|n| {
                    [300_u64, 10_080, 43_200]
                        .into_iter()
                        .find(|m| (n - *m as f64).abs() <= 1.)
                });
            if classified.is_none() != fallback_pass {
                continue;
            }
            let minutes = classified.unwrap_or(fallback);
            if !windows.iter().any(|w| w.minutes == minutes) {
                if let Some(w) = window(
                    raw,
                    minutes,
                    &["usedPercent", "used_percent", "used_percentage"],
                ) {
                    windows.push(w);
                }
            }
        }
    }
    windows.sort_by_key(|w| w.minutes);
    Snapshot {
        windows,
        updated_at: now_ms(),
        reset_credits: parse_reset_credits(value),
        ..Default::default()
    }
}
fn parse_claude(value: &Value) -> Result<Snapshot, FetchError> {
    if !value.is_object() {
        return Err(FetchError::failed("Claude usage response was unexpected."));
    }
    let mut windows = [("five_hour", 300), ("seven_day", 10_080)]
        .into_iter()
        .filter_map(|(key, minutes)| {
            window(
                &value[key],
                minutes,
                &["utilization", "used_percentage", "usedPercent", "percent"],
            )
        })
        .collect::<Vec<_>>();
    let mut models: Vec<ModelWindow> = Vec::new();
    if let Some(limits) = value["limits"].as_array() {
        for item in limits {
            // A weekly limit of one model's own ("Fable"). Claude Code shows
            // it as "Current week (Fable)" whether or not it binds yet.
            let model = item["scope"]["model"]["display_name"]
                .as_str()
                .map(str::trim)
                .filter(|name| !name.is_empty() && name.chars().count() <= 40);
            if let Some(model) = model {
                let weekly = matches!(item["kind"].as_str(), Some("weekly_scoped" | "seven_day"))
                    || item["group"] == "weekly";
                if weekly && !models.iter().any(|m| m.model == model) {
                    if let Some(window) = window(
                        item,
                        10_080,
                        &["percent", "utilization", "used_percentage", "usedPercent"],
                    ) {
                        models.push(ModelWindow {
                            model: model.to_owned(),
                            window,
                        });
                    }
                }
                continue;
            }
            if item["is_active"].as_bool() == Some(false) {
                continue;
            }
            let minutes = match (item["kind"].as_str(), item["group"].as_str()) {
                (Some("five_hour" | "session_all"), _) | (_, Some("session")) => Some(300),
                (Some("weekly_all" | "seven_day"), _) | (_, Some("weekly"))
                    if item.get("scope").is_none_or(Value::is_null) =>
                {
                    Some(10_080)
                }
                _ => None,
            };
            if let Some(minutes) = minutes {
                if !windows.iter().any(|w| w.minutes == minutes) {
                    if let Some(w) = window(
                        item,
                        minutes,
                        &["percent", "utilization", "used_percentage", "usedPercent"],
                    ) {
                        windows.push(w);
                    }
                }
            }
        }
        windows.sort_by_key(|w| w.minutes);
    }
    if windows.is_empty() {
        return Err(FetchError::unavailable(
            "No Claude subscription usage available for this account.",
        ));
    }
    Ok(Snapshot {
        windows,
        updated_at: now_ms(),
        models,
        ..Default::default()
    })
}

fn parse_reset_credits(value: &Value) -> Option<ResetCredits> {
    let raw = value
        .get("rateLimitResetCredits")
        .or_else(|| value.get("rate_limit_reset_credits"))?;
    let count = number(raw, &["availableCount", "available_count"])?;
    let credits = raw["credits"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let id = row["id"].as_str().filter(|s| !s.trim().is_empty())?;
            Some(ResetCredit {
                id: id.into(),
                status: row["status"]
                    .as_str()
                    .filter(|s| matches!(*s, "available" | "redeeming" | "redeemed"))
                    .unwrap_or("unknown")
                    .into(),
                reset_type: if row["resetType"] == "codexRateLimits" {
                    "codexRateLimits"
                } else {
                    "unknown"
                }
                .into(),
                granted_at: timestamp(
                    row.get("grantedAt")
                        .or_else(|| row.get("granted_at"))
                        .unwrap_or(&Value::Null),
                ),
                expires_at: timestamp(
                    row.get("expiresAt")
                        .or_else(|| row.get("expires_at"))
                        .unwrap_or(&Value::Null),
                ),
                title: row["title"].as_str().map(str::to_owned),
                description: row["description"].as_str().map(str::to_owned),
                ..Default::default()
            })
        })
        .collect();
    Some(ResetCredits {
        available_count: count.max(0.).floor() as u64,
        credits,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn explicit_codex_windows_win_before_positional_fallbacks() {
        let snapshot = parse_codex(
            &json!({"primary":{"usedPercent":99},"secondary":{"usedPercent":12,"windowDurationMins":300}}),
        );
        assert_eq!(snapshot.windows.len(), 1);
        assert_eq!(snapshot.windows[0].used_percent, 12.);
        let snapshot = parse_codex(
            &json!({"primary":{"usedPercent":44,"windowDurationMins":10080},"secondary":{"usedPercent":88}}),
        );
        assert_eq!(snapshot.windows.len(), 1);
        assert_eq!(snapshot.windows[0].used_percent, 44.);
    }
    #[test]
    fn reset_credit_details_keep_aliases_and_unknown_statuses() {
        let credits = parse_reset_credits(&json!({"rate_limit_reset_credits":{"available_count":2.9,"credits":[
            {"id":"a","status":"available","resetType":"codexRateLimits","expires_at":1800000000},
            {"id":"b","status":"future-status","resetType":"future-type","grantedAt":"2026-10-18T12:30:00Z"},
            {"id":""}
        ]}})).unwrap();
        assert_eq!(credits.available_count, 2);
        assert_eq!(credits.credits.len(), 2);
        assert_eq!(credits.credits[0].expires_at, Some(1_800_000_000_000));
        assert_eq!(credits.credits[1].status, "unknown");
        assert_eq!(credits.credits[1].reset_type, "unknown");
        assert!(parse_reset_credits(&json!({"rateLimitResetCredits":{"credits":[]}})).is_none());
    }
    #[test]
    fn exhausted_accounts_wait_for_all_windows_and_resume_only_when_armed() {
        let mut snapshot = Snapshot {
            windows: vec![
                Window {
                    used_percent: 100.,
                    minutes: 300,
                    resets_at: Some(100_000),
                },
                Window {
                    used_percent: 100.,
                    minutes: 10080,
                    resets_at: Some(200_000),
                },
            ],
            ..Default::default()
        };
        assert_eq!(snapshot.headroom(99_000), Some(0.));
        assert_eq!(snapshot.headroom(150_000), Some(0.));
        assert_eq!(snapshot.headroom(200_000), Some(100.));
        assert_eq!(snapshot.exhausted_reset(), Some(200_000));
        snapshot.windows[1].resets_at = None;
        assert_eq!(snapshot.exhausted_reset(), None);
        let mut limit = Limit {
            resets_at: Some(200_000),
            resume_at_reset: false,
        };
        assert!(!limit.resume_due(230_000, false));
        limit.resume_at_reset = true;
        assert!(!limit.resume_due(229_999, false));
        assert!(!limit.resume_due(230_000, true));
        assert!(limit.resume_due(230_000, false));
        assert!(Limit::from_error("usage_limit_reached").is_some());
        assert!(Limit::from_error("insufficient_quota").is_some());
        assert!(Limit::from_error("rate_limit").is_some());
        assert!(Limit::from_error("You've hit your session limit · resets 5:30pm").is_some());
        assert!(Limit::from_error("429: rate limit exceeded, retry in 3 seconds").is_none());
        assert!(Limit::from_error("Network timeout").is_none());
    }
    #[test]
    fn opencode_go_requires_known_window_statuses() {
        let snapshot = parse_opencode(
            &json!({"usage":{"rolling":{"status":"ok","percent":15,"resetsAt":"1800000000"},"weekly":{"status":"rate-limited","percent":100},"monthly":{"status":"future","percent":20}}}),
        );
        assert_eq!(snapshot.windows.len(), 2);
        assert_eq!(snapshot.windows[0].resets_at, Some(1_800_000_000_000));
        assert_eq!(snapshot.windows[1].minutes, 10080);
    }

    #[test]
    fn codex_classifies_swapped_and_monthly_windows() {
        let snapshot = parse_codex(&json!({"rateLimits": {
            "primary": {"usedPercent": 35, "windowDurationMins": 10080, "resetsAt": 1800000000},
            "secondary": {"used_percent": 105, "window_duration_mins": 300, "resets_at": 1800000000000_i64}
        }}));
        assert_eq!(snapshot.windows.len(), 2);
        assert_eq!(snapshot.windows[0].minutes, 300);
        assert_eq!(snapshot.windows[0].used_percent, 100.);
        assert_eq!(snapshot.windows[1].minutes, 10_080);
        assert_eq!(snapshot.windows[0].resets_at, snapshot.windows[1].resets_at);
        let monthly =
            parse_codex(&json!({"primary": {"usedPercent": 12, "windowDurationMins": 43200}}));
        assert_eq!(monthly.windows[0].label(), "mo");
        assert!(parse_codex(&json!({"primary": {"usedPercent": "bad"}}))
            .windows
            .is_empty());
    }
    #[test]
    fn claude_reads_fables_weekly_limit_apart_from_the_accounts() {
        // The shape Claude answered on Nov 11, 2026, ids left out.
        let value = json!({
            "five_hour": null,
            "seven_day": {"utilization": 52.0, "resets_at": "2026-11-15T05:59:59.821315+00:00"},
            "limits": [
                {"group": "weekly", "is_active": true, "kind": "weekly_all", "percent": 52,
                 "resets_at": "2026-11-15T05:59:59.821315+00:00", "scope": null},
                {"group": "weekly", "is_active": false, "kind": "weekly_scoped", "percent": 36,
                 "resets_at": "2026-11-15T05:59:59.821538+00:00",
                 "scope": {"model": {"display_name": "Fable"}, "surface": null}}
            ]
        });
        let snapshot = parse_claude(&value).unwrap();
        assert_eq!(snapshot.windows.len(), 1);
        assert_eq!(snapshot.windows[0].used_percent, 52.);
        assert_eq!(snapshot.models.len(), 1);
        assert_eq!(snapshot.models[0].model, "Fable");
        assert_eq!(snapshot.models[0].window.used_percent, 36.);
        assert_eq!(snapshot.models[0].window.minutes, 10_080);
        // Fable at its limit does not stop the account.
        let mut full = snapshot.clone();
        full.models[0].window.used_percent = 100.;
        assert_eq!(full.headroom(0), snapshot.headroom(0));
        // Older snapshots without models still read.
        let old: Snapshot = serde_json::from_str(r#"{"windows":[],"updated_at":1}"#).unwrap();
        assert!(old.models.is_empty());
    }

    #[test]
    fn claude_parses_iso_resets_and_rejects_empty_responses() {
        let snapshot = parse_claude(&json!({
            "five_hour": {"utilization": 23.4, "resets_at": "2026-10-18T12:30:00Z"},
            "seven_day": {"utilization": -2, "resets_at": null}
        }))
        .unwrap();
        assert_eq!(snapshot.windows[0].used_percent, 23.4);
        assert_eq!(snapshot.windows[0].resets_at, Some(1792326600000));
        assert_eq!(snapshot.windows[1].used_percent, 0.);
        assert!(parse_claude(&json!({})).unwrap_err().unavailable);
        assert!(!parse_claude(&json!(null)).unwrap_err().unavailable);
    }
    #[test]
    fn reset_countdowns_match_the_footer() {
        for (ms, expected) in [
            (0, "now"),
            (-1, "now"),
            (47 * 60_000, "47m"),
            (234 * 60_000, "3h 54m"),
            (9060 * 60_000, "6d 7h"),
        ] {
            assert_eq!(duration(ms), expected);
        }
        let w = Window {
            used_percent: 4.,
            minutes: 300,
            resets_at: None,
        };
        assert_eq!(w.countdown(0), "5h");
    }
}
