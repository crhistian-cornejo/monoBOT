//! Antigravity's plan usage, from `agy -p /quota --output-format json`.
//!
//! The CLI groups its models (Gemini, Claude and GPT, Argon), each with a
//! weekly and a five-hour window. The footer shows the group of the model in
//! use, or Gemini's when there is none.

use std::time::Duration;

use serde_json::Value;

use super::{now_ms, timestamp, FetchError, Snapshot, Window};
use crate::provider::Kind;

/// The quota group a model draws from: the bucket ids' prefix.
pub fn group_for(model: &str) -> &'static str {
    let id = model.trim().to_ascii_lowercase();
    if id.contains("argon") || id.contains("g1-pro") {
        "gemini-argon"
    } else if id.starts_with("claude") || id.starts_with("gpt") {
        "3p"
    } else {
        "gemini"
    }
}

pub(super) fn fetch(group: Option<&str>) -> Result<Snapshot, FetchError> {
    let binary = crate::provider::resolve_agy().ok_or_else(|| {
        FetchError::unavailable("Install the Antigravity CLI (agy) to see usage.")
    })?;
    let output = crate::accounts::login::binary_output(
        &binary,
        Kind::Antigravity,
        &["-p", "/quota", "--output-format", "json"],
        &Default::default(),
        Duration::from_secs(30),
    )
    .map_err(FetchError::failed)?;
    let value: Value = output
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str(line.trim()).ok())
        .ok_or_else(|| FetchError::failed("Antigravity's quota answer was not JSON"))?;
    parse(&value, group.unwrap_or("gemini"))
}

pub(super) fn parse(value: &Value, group: &str) -> Result<Snapshot, FetchError> {
    let groups = value["command"]["data"]["groups"]
        .as_array()
        .filter(|g| !g.is_empty());
    let Some(groups) = groups else {
        let message = value["response"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty() && value["status"] != "SUCCESS")
            .unwrap_or("No Antigravity plan usage for this sign-in.");
        return Err(FetchError::unavailable(message));
    };
    let windows_of = |entry: &Value| -> Vec<Window> {
        let mut windows: Vec<Window> = entry["buckets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|bucket| {
                let remaining = bucket["remaining_fraction"]
                    .as_f64()
                    .filter(|n| n.is_finite())?;
                let minutes = match bucket["window"].as_str()? {
                    "5h" => 300,
                    "weekly" => 10080,
                    "daily" => 1440,
                    "monthly" => 43200,
                    _ => return None,
                };
                Some(Window {
                    used_percent: ((1. - remaining) * 100.).clamp(0., 100.),
                    minutes,
                    resets_at: timestamp(&bucket["reset_time"]),
                })
            })
            .collect();
        windows.sort_by_key(|w| w.minutes);
        windows
    };
    // Bucket ids start with their group: `gemini-5h`, `3p-weekly`,
    // `gemini-argon-wtu-weekly`.
    let prefix = |entry: &Value| {
        let id = entry["buckets"][0]["id"].as_str().unwrap_or_default();
        ["gemini-argon", "3p", "gemini"]
            .into_iter()
            .find(|group| id.starts_with(&format!("{group}-")))
            .unwrap_or_default()
    };
    let chosen = groups
        .iter()
        .find(|g| prefix(g) == group)
        .or_else(|| groups.iter().find(|g| prefix(g) == "gemini"))
        .unwrap_or(&groups[0]);
    let windows = windows_of(chosen);
    if windows.is_empty() {
        return Err(FetchError::failed(
            "Antigravity's quota answer had no limits",
        ));
    }
    Ok(Snapshot {
        windows,
        updated_at: now_ms(),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer() -> Value {
        serde_json::json!({
            "status": "SUCCESS",
            "command": {"name": "usage", "data": {"groups": [
                {"name": "Gemini Models", "buckets": [
                    {"id": "gemini-weekly", "window": "weekly", "remaining_fraction": 0.75, "reset_time": "2026-11-25T18:00:59Z"},
                    {"id": "gemini-5h", "window": "5h", "remaining_fraction": 0.5, "reset_time": "2026-11-19T09:04:00Z"}
                ]},
                {"name": "Claude and GPT models", "buckets": [
                    {"id": "3p-weekly", "window": "weekly", "remaining_fraction": 1, "reset_time": "2026-11-26T04:04:49Z"},
                    {"id": "3p-5h", "window": "5h", "remaining_fraction": 0.9, "reset_time": "2026-11-19T09:04:49Z"}
                ]},
                {"name": "Gemini 4 Argon", "buckets": [
                    {"id": "gemini-argon-wtu-weekly", "window": "weekly", "remaining_fraction": 0.2, "reset_time": "2026-11-26T04:04:49Z"}
                ]}
            ]}}
        })
    }

    #[test]
    fn models_draw_from_their_own_group() {
        assert_eq!(group_for("gemini-4-neon"), "gemini");
        assert_eq!(group_for("gemini-3.8-flash"), "gemini");
        assert_eq!(group_for("claude-opus-5-5"), "3p");
        assert_eq!(group_for("gpt-oss-120b"), "3p");
        assert_eq!(group_for("gemini-4-argon"), "gemini-argon");
        assert_eq!(group_for("gemini-pro-orchestrator-g1-pro"), "gemini-argon");
    }

    #[test]
    fn the_quota_answer_becomes_five_hour_and_weekly_windows() {
        let gemini = parse(&answer(), "gemini").unwrap();
        let shape: Vec<_> = gemini
            .windows
            .iter()
            .map(|w| (w.minutes, w.used_percent.round() as i64))
            .collect();
        assert_eq!(shape, [(300, 50), (10080, 25)]);
        assert!(gemini.windows.iter().all(|w| w.resets_at.is_some()));

        let third = parse(&answer(), "3p").unwrap();
        assert_eq!(third.windows[0].used_percent.round() as i64, 10);

        // Argon keeps its own limit.
        let argon = parse(&answer(), "gemini-argon").unwrap();
        assert_eq!(argon.windows.len(), 1);
        assert_eq!(argon.windows[0].minutes, 10080);
        assert_eq!(argon.windows[0].used_percent.round() as i64, 80);

        // An unknown group falls back to Gemini's.
        assert_eq!(parse(&answer(), "other").unwrap().windows.len(), 2);
    }

    #[test]
    fn an_api_key_without_a_plan_is_unavailable() {
        let value =
            serde_json::json!({"status": "ERROR", "response": "API keys have no account quota."});
        let error = parse(&value, "gemini").unwrap_err();
        assert!(error.unavailable);
        assert_eq!(error.message, "API keys have no account quota.");
    }

    #[test]
    #[ignore = "runs the signed-in agy CLI"]
    fn live_quota_reads_the_signed_in_plan() {
        let snapshot = fetch(Some("gemini")).unwrap();
        eprintln!("{:?}", snapshot.windows);
        assert!(!snapshot.windows.is_empty());
    }
}
