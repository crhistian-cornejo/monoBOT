use super::{now_ms, parse_claude, FetchError, ResetCredit, ResetCredits, Snapshot};
use serde_json::Value;
use std::{io::Read, path::PathBuf, time::Duration};

/// The Claude account's API base: the same host Claude Code reads usage and
/// spends resets on.
const API: &str = "https://api.anthropic.com";
/// Usage plus Claude's reset grants, without counting this read as spend.
const USAGE_WITH_RESETS: &str = "/api/oauth/usage?cedar_ember=1&skip_spend=1";
/// The same at a limit: it also says whether the weekly 5-hour reset is there.
const USAGE_AT_LIMIT: &str = "/api/oauth/usage?at_wall=1&skip_spend=1";
const PLAIN_USAGE: &str = "/api/oauth/usage";

/// The signed-in account's token and its config folder. The CLI owns its
/// rotating refresh token: it is never refreshed or written here.
fn session(profile: &crate::accounts::Profile) -> Result<(Option<PathBuf>, String), FetchError> {
    let config = profile
        .directory(crate::provider::Kind::Claude)
        .map_err(FetchError::failed)?
        .or_else(config_dir);
    let credentials = credentials(config.clone()).ok_or_else(|| {
        FetchError::unavailable("Sign in with claude auth login, then refresh usage.")
    })?;
    let oauth = credentials.get("claudeAiOauth").unwrap_or(&credentials);
    let token = oauth["accessToken"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| FetchError::unavailable("Claude is not signed in."))?;
    let expiry = oauth["expiresAt"]
        .as_i64()
        .or_else(|| oauth["expiresAt"].as_str()?.parse::<i64>().ok());
    if expiry.is_some_and(|at| at <= now_ms()) {
        return Err(expired_error(oauth));
    }
    Ok((config, token.to_owned()))
}

fn read(token: &str, path: &str) -> Result<Value, FetchError> {
    let response = monobot_http::agent()
        .timeout(Duration::from_secs(10))
        .build()
        .get(&format!("{API}{path}"))
        .set("Authorization", &format!("Bearer {token}"))
        .set("anthropic-beta", "oauth-2025-04-20")
        .set("User-Agent", "claude-code/2.1.0")
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(401, _) => {
                FetchError::failed("Claude usage updates after your next Claude message.")
            }
            ureq::Error::Status(403, _) => {
                FetchError::unavailable("Claude usage is unavailable for this account.")
            }
            ureq::Error::Status(status, _) => {
                FetchError::failed(format!("Claude usage request failed ({status})."))
            }
            _ => FetchError::failed("Could not reach Claude usage. Try refreshing again."),
        })?;
    let mut body = String::new();
    response
        .into_reader()
        .take(1024 * 1024)
        .read_to_string(&mut body)
        .map_err(|_| FetchError::failed("Could not read Claude usage response."))?;
    serde_json::from_str(&body)
        .map_err(|_| FetchError::failed("Claude usage response was not JSON."))
}

pub(super) fn fetch_for(profile: &crate::accounts::Profile) -> Result<Snapshot, FetchError> {
    let (_, token) = session(profile)?;
    let value = read(&token, USAGE_WITH_RESETS)?;
    // The reset read may come without the windows: then ask for them alone.
    let mut snapshot = match parse_claude(&value) {
        Ok(snapshot) => snapshot,
        Err(_) => parse_claude(&read(&token, PLAIN_USAGE)?)?,
    };
    let mut resets = parse_resets(&value);
    // At a limit, the weekly 5-hour reset may be there too.
    let reached = snapshot.windows.iter().any(|w| w.used_percent >= 100.)
        || resets.as_ref().is_some_and(|r| !r.exhausted.is_empty());
    let wall = if reached {
        read(&token, USAGE_AT_LIMIT).ok()
    } else {
        None
    };
    if let Some(weekly) = wall.as_ref().and_then(weekly_five_hour) {
        let resets = resets.get_or_insert_with(Default::default);
        resets.available_count += 1;
        resets.credits.push(weekly);
    }
    snapshot.reset_credits = resets;
    Ok(snapshot)
}

/// Claude's reset grants (`cedar_ember`): each with how many uses it has
/// left, the limits it refills and until when. A grant that gives back what
/// Fable used is the Fable reset.
pub(super) fn parse_resets(value: &Value) -> Option<ResetCredits> {
    let block = value.get("cedar_ember")?;
    if block["eligible"] != true {
        return None;
    }
    let keys = |value: &Value| -> Vec<String> {
        value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|key| is_key(key))
            .map(str::to_owned)
            .collect()
    };
    let mut resets = ResetCredits {
        exhausted: keys(&block["exhausted"]),
        ..Default::default()
    };
    for grant in block["grants"].as_array().into_iter().flatten() {
        let Some(id) = grant["id"].as_str().filter(|id| is_grant_id(id)) else {
            continue;
        };
        let left = grant["resets_left"].as_u64().unwrap_or(0);
        let paused = grant["paused"] == true;
        let clears = keys(&grant["clears"]);
        if let Some(used) = grant["percent_used"].as_object() {
            for (key, percent) in used.iter().filter(|(key, _)| is_key(key)) {
                if let Some(percent) = percent.as_f64().filter(|p| p.is_finite()) {
                    let kept = resets.used.entry(key.clone()).or_insert(0.);
                    *kept = kept.max(percent.clamp(0., 100.));
                }
            }
        }
        let kind = if grant["gives_back_weekly"] == true {
            "fable"
        } else {
            match grant["type"].as_str() {
                Some(kind @ ("session" | "weekly" | "full")) => kind,
                _ => {
                    let session = clears.iter().any(|key| key == "five_hour");
                    let other = clears.iter().any(|key| key != "five_hour");
                    match (session, other) {
                        (true, false) => "session",
                        (false, true) => "weekly",
                        _ => "full",
                    }
                }
            }
        };
        if !paused {
            resets.available_count += left;
        }
        resets.credits.push(ResetCredit {
            id: id.into(),
            status: if paused || left == 0 {
                "paused"
            } else {
                "available"
            }
            .into(),
            reset_type: format!("claude:{kind}"),
            expires_at: grant["ends_at"].as_str().and_then(|at| {
                chrono::DateTime::parse_from_rfc3339(at)
                    .ok()
                    .map(|at| at.timestamp_millis())
            }),
            title: grant["label"]
                .as_str()
                .map(str::trim)
                .filter(|label| !label.is_empty())
                .map(str::to_owned),
            clears,
            left: Some(left),
            needs_limit: grant["use_requires_limit"] != false,
            ..Default::default()
        });
    }
    Some(resets)
}

/// The weekly 5-hour reset (`juniper_tide`), where it is there to use now.
fn weekly_five_hour(value: &Value) -> Option<ResetCredit> {
    let block = value.get("juniper_tide")?;
    (block["eligible"] == true && block["available"] == true).then(|| ResetCredit {
        id: WEEKLY_FIVE_HOUR.into(),
        status: "available".into(),
        reset_type: "claude:session".into(),
        title: Some("Weekly 5-hour reset".into()),
        description: Some("Once a week; it still counts toward your weekly limit.".into()),
        clears: vec!["five_hour".into()],
        left: Some(1),
        needs_limit: true,
        ..Default::default()
    })
}

/// The id given to the weekly 5-hour reset, which has none of its own.
const WEEKLY_FIVE_HOUR: &str = "juniper_tide";

fn is_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Claude's own rule for a grant's id.
fn is_grant_id(id: &str) -> bool {
    (1..=40).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// The organization the account belongs to, as Claude Code records it.
fn organization(config: Option<PathBuf>) -> Option<String> {
    let path = match config {
        Some(dir) => dir.join(".claude.json"),
        None => crate::accounts::home()?.join(".claude.json"),
    };
    let value: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    value["oauthAccount"]["organizationUuid"]
        .as_str()
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 64
                && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
        })
        .map(str::to_owned)
}

/// The request that spends `credit`: the weekly 5-hour reset alone, or a
/// grant by its id with a fresh request id, so a retry is never a second use.
pub(super) fn claim_body(credit: &ResetCredit, request: &str) -> Result<Value, String> {
    if credit.id == WEEKLY_FIVE_HOUR {
        return Ok(serde_json::json!({ "program": "juniper_tide" }));
    }
    if !is_grant_id(&credit.id) {
        return Err("That Claude reset can't be used from here.".into());
    }
    Ok(serde_json::json!({
        "program": "cedar_ember",
        "grant_id": credit.id,
        "request_id": request,
    }))
}

/// What Claude answered, as the outcome names Codex uses.
pub(super) fn claim_outcome(value: &Value) -> Result<String, String> {
    match value["result"].as_str() {
        Some("reset") => Ok("reset".into()),
        Some("not_limited") => Ok("nothingToReset".into()),
        Some("already_used") => Ok("alreadyRedeemed".into()),
        Some("cooldown") => Err("Claude allows another reset a little later.".into()),
        Some("ineligible") => Ok("noCredit".into()),
        _ => Err("Claude couldn't reset your limits right now. Nothing was used.".into()),
    }
}

/// Spends `credit` on the account.
pub(super) fn claim(
    profile: &crate::accounts::Profile,
    credit: &ResetCredit,
) -> Result<String, String> {
    let (config, token) = session(profile).map_err(|error| error.message)?;
    let organization =
        organization(config).ok_or("Claude Code hasn't recorded this account's organization.")?;
    let request: String = crate::accounts::request_id()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(64)
        .collect();
    let body = claim_body(credit, &request)?;
    let response = monobot_http::agent()
        .timeout(Duration::from_secs(25))
        .build()
        .post(&format!(
            "{API}/api/organizations/{organization}/reset_rate_limits"
        ))
        .set("Authorization", &format!("Bearer {token}"))
        .set("anthropic-beta", "oauth-2025-04-20")
        .set("User-Agent", "claude-code/2.1.0")
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|error| match error {
            ureq::Error::Status(429, _) => {
                "Claude is busy. Try the reset again in a moment.".to_owned()
            }
            ureq::Error::Status(401 | 403, _) => {
                "Claude needs you to sign in again before using a reset.".to_owned()
            }
            _ => "Couldn't reach Claude. Nothing was used.".to_owned(),
        })?;
    let mut text = String::new();
    response
        .into_reader()
        .take(64 * 1024)
        .read_to_string(&mut text)
        .map_err(|_| "Couldn't read Claude's answer. Check your usage in a moment.".to_owned())?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|_| "Couldn't read Claude's answer. Check your usage in a moment.".to_owned())?;
    claim_outcome(&value)
}

/// The access token lasts about 8 hours. While a refresh token is present the
/// CLI renews it on its next run, so that is not a sign-out: a failed (not
/// unavailable) result keeps the last usage and hides the sign-in panel.
fn expired_error(oauth: &Value) -> FetchError {
    if oauth["refreshToken"]
        .as_str()
        .is_some_and(|s| !s.trim().is_empty())
    {
        FetchError::failed("Claude usage updates after your next Claude message.")
    } else {
        FetchError::unavailable("Claude sign-in expired. Run claude auth login, then refresh.")
    }
}

fn config_dir() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}
fn credentials(config: Option<PathBuf>) -> Option<Value> {
    #[cfg(target_os = "macos")]
    {
        if let Some(value) = keychain(config.as_deref()) {
            return Some(value);
        }
    }
    let dir = config.or_else(|| {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(|home| PathBuf::from(home).join(".claude"))
    })?;
    let raw = std::fs::read_to_string(dir.join(".credentials.json")).ok()?;
    credential_blob(&raw)
}
fn credential_blob(raw: &str) -> Option<Value> {
    let value: Value = serde_json::from_str(raw.trim()).ok()?;
    value.get("claudeAiOauth").unwrap_or(&value)["accessToken"]
        .as_str()
        .filter(|s| !s.trim().is_empty())?;
    Some(value)
}

#[cfg(target_os = "macos")]
pub(crate) fn delete_keychain(config: &std::path::Path) -> Result<(), String> {
    use crate::process;
    use std::{
        process::{Command, Stdio},
        time::Instant,
    };
    // Claude Code's own Keychain item is the user's: a test leaves it alone.
    if !crate::secrets::uses_system_keychain() {
        return Ok(());
    }
    let service = keychain_service(Some(config));
    let mut command = Command::new("/usr/bin/security");
    command
        .args(["delete-generic-password", "-s", &service])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    process::isolate(&mut command);
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let _registration = process::register(child.id());
    let deadline = Instant::now() + Duration::from_secs(5);
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() || status.code() == Some(44) => break Ok(()),
            Ok(Some(_)) => {
                break Err("Could not remove the account's Claude Keychain credentials".into());
            }
            Err(e) => break Err(e.to_string()),
            Ok(None) if Instant::now() >= deadline => {
                break Err("Removing Claude credentials timed out".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    process::terminate(&mut child);
    result
}

#[cfg(target_os = "macos")]
fn keychain_service(config: Option<&std::path::Path>) -> String {
    use sha2::{Digest, Sha256};
    use unicode_normalization::UnicodeNormalization;
    config
        .map(|dir| {
            let selector: String = dir.to_string_lossy().nfc().collect();
            let digest = format!("{:x}", Sha256::digest(selector.as_bytes()));
            format!("Claude Code-credentials-{}", &digest[..8])
        })
        .unwrap_or_else(|| "Claude Code-credentials".into())
}

#[cfg(target_os = "macos")]
fn keychain(config: Option<&std::path::Path>) -> Option<Value> {
    let service = keychain_service(config);
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "claude-code-user".into());
    for args in [
        vec!["find-generic-password", "-s", &service, "-w"],
        vec!["find-generic-password", "-s", &service, "-a", &user, "-w"],
        vec![
            "find-generic-password",
            "-s",
            &service,
            "-a",
            "claude-code-user",
            "-w",
        ],
    ] {
        if let Some(raw) = keychain_query(&args) {
            if let Some(value) = credential_blob(&raw) {
                return Some(value);
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn keychain_query(args: &[&str]) -> Option<String> {
    use crate::process;
    use std::{
        process::{Command, Stdio},
        sync::mpsc,
        thread,
        time::Instant,
    };

    // Claude Code's own Keychain item is the user's: a test never asks the
    // system for it.
    if !crate::secrets::uses_system_keychain() {
        return None;
    }
    let mut command = Command::new("/usr/bin/security");
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    process::isolate(&mut command);
    let mut child = command.spawn().ok()?;
    let _registration = process::register(child.id());
    let output = child.stdout.take()?;
    // Drain concurrently: waiting before reading can fill the pipe and hang.
    let (tx, rx) = mpsc::sync_channel(1);
    let reader = thread::spawn(move || {
        let mut body = String::new();
        let result = output.take(1024 * 1024).read_to_string(&mut body);
        let _ = tx.send(result.ok().map(|_| body));
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                break rx
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .ok()
                    .flatten();
            }
            Ok(Some(_)) | Err(_) => break None,
            Ok(None) if Instant::now() >= deadline => break None,
            Ok(None) => thread::sleep(Duration::from_millis(20)),
        }
    };
    process::terminate(&mut child);
    drop(rx);
    let _ = reader.join();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_grants_read_as_resets_with_their_kind_limits_and_uses() {
        let value = serde_json::json!({"cedar_ember": {
        "eligible": true, "at_limit": true, "exhausted": ["five_hour", "Bad Key"],
        "grants": [
            {"id": "g-session", "label": "", "resets_total": 3, "resets_left": 2,
             "ends_at": "2026-11-14T00:00:00Z", "clears": ["five_hour"], "paused": false,
             "usable_now": true, "use_requires_limit": true,
             "percent_used": {"five_hour": 100, "seven_day": 61}, "type": "session"},
            {"id": "g-fable", "label": "Fable reset", "resets_left": 1,
             "clears": ["seven_day_overage_included"], "gives_back_weekly": true,
             "percent_used": {"seven_day_overage_included": 97}},
            {"id": "g-mixed", "resets_left": 1, "clears": ["five_hour", "seven_day"]},
            {"id": "g-paused", "resets_left": 4, "clears": ["seven_day"], "paused": true},
            {"id": "NOT VALID", "resets_left": 9, "clears": ["five_hour"]}
        ]}});
        let resets = parse_resets(&value).unwrap();
        // Two of the session grant, the Fable and the mixed one; not the paused.
        assert_eq!(resets.available_count, 4);
        assert_eq!(resets.exhausted, ["five_hour"]);
        assert_eq!(resets.used["five_hour"], 100.);
        assert_eq!(resets.used["seven_day_overage_included"], 97.);
        let kinds: Vec<_> = resets
            .credits
            .iter()
            .map(|c| (c.id.as_str(), c.reset_type.as_str(), c.status.as_str()))
            .collect();
        assert_eq!(
            kinds,
            [
                ("g-session", "claude:session", "available"),
                ("g-fable", "claude:fable", "available"),
                ("g-mixed", "claude:full", "available"),
                ("g-paused", "claude:weekly", "paused"),
            ]
        );
        assert_eq!(resets.credits[0].left, Some(2));
        assert_eq!(resets.credits[0].expires_at, Some(1_794_614_400_000));
        assert!(resets.credits[0].needs_limit);
        assert_eq!(resets.credits[1].title.as_deref(), Some("Fable reset"));
        // Not offered to this account, or not in the answer: nothing.
        assert!(parse_resets(&serde_json::json!({"cedar_ember": {"eligible": false}})).is_none());
        assert!(parse_resets(&serde_json::json!({"five_hour": {}})).is_none());
        // The weekly 5-hour reset, when it is there to use.
        let weekly = weekly_five_hour(
            &serde_json::json!({"juniper_tide": {"eligible": true, "available": true}}),
        )
        .unwrap();
        assert_eq!(
            (weekly.id.as_str(), weekly.clears.as_slice()),
            ("juniper_tide", &["five_hour".to_owned()][..])
        );
        assert!(weekly_five_hour(
            &serde_json::json!({"juniper_tide": {"eligible": true, "available": false}})
        )
        .is_none());
    }

    #[test]
    fn a_claim_names_its_grant_once_and_reads_claudes_answer() {
        let grant = ResetCredit {
            id: "g-session".into(),
            ..Default::default()
        };
        assert_eq!(
            claim_body(&grant, "req-1").unwrap(),
            serde_json::json!({"program": "cedar_ember", "grant_id": "g-session", "request_id": "req-1"})
        );
        let weekly = ResetCredit {
            id: WEEKLY_FIVE_HOUR.into(),
            ..Default::default()
        };
        assert_eq!(
            claim_body(&weekly, "req-2").unwrap(),
            serde_json::json!({"program": "juniper_tide"})
        );
        let odd = ResetCredit {
            id: "../x".into(),
            ..Default::default()
        };
        assert!(claim_body(&odd, "r").is_err());
        let outcome = |result: &str| claim_outcome(&serde_json::json!({"result": result}));
        assert_eq!(outcome("reset").unwrap(), "reset");
        assert_eq!(outcome("not_limited").unwrap(), "nothingToReset");
        assert_eq!(outcome("already_used").unwrap(), "alreadyRedeemed");
        assert_eq!(outcome("ineligible").unwrap(), "noCredit");
        assert!(outcome("cooldown").is_err());
        assert!(outcome("unavailable").is_err());
        assert!(claim_outcome(&serde_json::json!({})).is_err());
    }

    #[test]
    fn the_organization_comes_from_claude_codes_own_record() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".claude.json"),
            r#"{"oauthAccount":{"organizationUuid":"0a1b2c3d-1111-2222-3333-444455556666"}}"#,
        )
        .unwrap();
        assert_eq!(
            organization(Some(dir.path().to_owned())).as_deref(),
            Some("0a1b2c3d-1111-2222-3333-444455556666")
        );
        std::fs::write(
            dir.path().join(".claude.json"),
            r#"{"oauthAccount":{"organizationUuid":"../../etc"}}"#,
        )
        .unwrap();
        assert!(organization(Some(dir.path().to_owned())).is_none());
    }

    #[test]
    fn an_expired_access_token_with_a_refresh_token_is_not_a_sign_out() {
        let renewable = serde_json::json!({"accessToken":"a","refreshToken":"r","expiresAt":1});
        assert!(!expired_error(&renewable).unavailable);
        // Claude Code blanks both tokens when a refresh fails.
        let wiped = serde_json::json!({"accessToken":"","refreshToken":"","expiresAt":0});
        assert!(expired_error(&wiped).unavailable);
        assert!(expired_error(&serde_json::json!({"accessToken":"a"})).unavailable);
    }
    #[test]
    fn accepts_nested_and_legacy_credentials_without_logging_secrets() {
        assert!(
            credential_blob(r#"{"claudeAiOauth":{"accessToken":"test","expiresAt":123}}"#)
                .is_some()
        );
        assert!(credential_blob(r#"{"accessToken":"test"}"#).is_some());
        assert!(credential_blob(r#"{"accessToken":" "}"#).is_none());
        assert!(credential_blob("invalid").is_none());
    }
}
