use super::{now_ms, FetchError, Snapshot, Window};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const DEFAULT_CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) fn fetch_for(_profile: &crate::accounts::Profile) -> Result<Snapshot, FetchError> {
    let home = kimi_home()
        .ok_or_else(|| FetchError::unavailable("Sign in with kimi login, then refresh usage."))?;

    let (oauth_host, base_url, cred_file) = resolve_config_and_credentials(&home);

    let cred_path = cred_file
        .ok_or_else(|| FetchError::unavailable("Sign in with kimi login, then refresh usage."))?;

    let mut creds = read_credentials(&cred_path)
        .ok_or_else(|| FetchError::unavailable("Sign in with kimi login, then refresh usage."))?;

    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let expires_at = creds["expires_at"]
        .as_i64()
        .or_else(|| creds["expires_at"].as_str()?.parse::<i64>().ok())
        .unwrap_or(0);

    let mut access_token = creds["access_token"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    let refresh_token = creds["refresh_token"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    // If access token is expired (or expires within 60s), refresh it.
    if access_token.is_empty() || (expires_at > 0 && expires_at <= now_secs + 60) {
        if refresh_token.is_empty() {
            return Err(FetchError::unavailable(
                "Kimi sign-in expired. Run kimi login, then refresh.",
            ));
        }
        match refresh_oauth_token(&oauth_host, &refresh_token) {
            Ok(new_tokens) => {
                if let Some(tok) = new_tokens["access_token"].as_str() {
                    access_token = tok.to_string();
                }
                creds["access_token"] = json!(access_token);
                if let Some(new_refresh) = new_tokens["refresh_token"].as_str() {
                    creds["refresh_token"] = json!(new_refresh);
                }
                let expires_in = new_tokens["expires_in"].as_i64().unwrap_or(900);
                creds["expires_in"] = json!(expires_in);
                creds["expires_at"] = json!(now_secs + expires_in);
                write_credentials(&cred_path, &creds);
            }
            Err(err) => {
                if access_token.is_empty() {
                    return Err(err);
                }
            }
        }
    }

    if access_token.is_empty() {
        return Err(FetchError::unavailable(
            "Sign in with kimi login, then refresh usage.",
        ));
    }

    // Attempt usage fetch
    let usage_val = fetch_usages(
        &base_url,
        &oauth_host,
        &access_token,
        &refresh_token,
        &cred_path,
    )?;
    let identity = fetch_identity(&base_url, &access_token);

    parse_kimi_usage(&usage_val, identity)
}

fn kimi_home() -> Option<PathBuf> {
    std::env::var_os("KIMI_CODE_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| crate::accounts::home().map(|h| h.join(".kimi-code")))
}

fn resolve_config_and_credentials(home: &Path) -> (String, String, Option<PathBuf>) {
    let mut oauth_host = "https://auth.kimi.com".to_string();
    let mut base_url = "https://api.kimi.com/coding/v1".to_string();
    let mut configured_key = None;

    let region_file = home.join("region");
    if let Ok(region) = std::fs::read_to_string(&region_file) {
        if region.trim() == "global" {
            oauth_host = "https://auth.kimi.ai".to_string();
            base_url = "https://api.kimi.ai/coding/v1".to_string();
        }
    }

    let config_file = home.join("config.toml");
    if let Ok(content) = std::fs::read_to_string(&config_file) {
        if let Ok(table) = content.parse::<toml::Table>() {
            if let Some(providers) = table.get("providers").and_then(toml::Value::as_table) {
                if let Some(managed) = providers
                    .get("managed:kimi-code")
                    .and_then(toml::Value::as_table)
                {
                    if let Some(url) = managed.get("base_url").and_then(toml::Value::as_str) {
                        base_url = url.trim_end_matches('/').to_string();
                    }
                    if let Some(oauth) = managed.get("oauth").and_then(toml::Value::as_table) {
                        if let Some(host) = oauth.get("oauth_host").and_then(toml::Value::as_str) {
                            oauth_host = host.trim_end_matches('/').to_string();
                        }
                        if let Some(k) = oauth.get("key").and_then(toml::Value::as_str) {
                            configured_key = Some(k.to_string());
                        }
                    }
                }
            }
        }
    }

    let cred_dir = home.join("credentials");
    let cred_file = if let Some(key) = configured_key {
        let file_name = key.strip_prefix("oauth/").unwrap_or(&key);
        let path = cred_dir.join(format!("{file_name}.json"));
        if path.is_file() {
            Some(path)
        } else {
            find_any_credential_file(&cred_dir)
        }
    } else {
        find_any_credential_file(&cred_dir)
    };

    (oauth_host, base_url, cred_file)
}

fn find_any_credential_file(cred_dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(cred_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            if let Ok(raw) = std::fs::read_to_string(&path) {
                if raw.contains("access_token") || raw.contains("refresh_token") {
                    return Some(path);
                }
            }
        }
    }
    None
}

fn read_credentials(path: &Path) -> Option<Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_credentials(path: &Path, creds: &Value) {
    if let Ok(content) = serde_json::to_string_pretty(creds) {
        let temp = path.with_extension("tmp");
        if std::fs::write(&temp, content).is_ok() {
            let _ = std::fs::rename(temp, path);
        }
    }
}

fn refresh_oauth_token(oauth_host: &str, refresh_token: &str) -> Result<Value, FetchError> {
    let url = format!("{}/api/oauth/token", oauth_host.trim_end_matches('/'));
    let agent = monobot_http::agent().timeout(HTTP_TIMEOUT).build();
    let response = agent.post(&url).send_form(&[
        ("client_id", DEFAULT_CLIENT_ID),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
    ]);

    match response {
        Ok(resp) => {
            let body = resp.into_string().unwrap_or_default();
            serde_json::from_str(&body)
                .map_err(|_| FetchError::failed("Kimi token refresh returned invalid JSON."))
        }
        Err(ureq::Error::Status(401 | 403, _)) => Err(FetchError::unavailable(
            "Kimi sign-in expired. Run kimi login, then refresh.",
        )),
        Err(ureq::Error::Status(status, _)) => Err(FetchError::failed(format!(
            "Kimi token refresh failed (HTTP {status})."
        ))),
        Err(e) => Err(FetchError::failed(format!(
            "Could not reach Kimi OAuth: {e}"
        ))),
    }
}

fn fetch_usages(
    base_url: &str,
    oauth_host: &str,
    access_token: &str,
    refresh_token: &str,
    cred_path: &Path,
) -> Result<Value, FetchError> {
    let agent = monobot_http::agent().timeout(HTTP_TIMEOUT).build();
    let url = format!("{}/usages", base_url.trim_end_matches('/'));

    let res = agent
        .get(&url)
        .set("Authorization", &format!("Bearer {access_token}"))
        .set("Accept", "application/json")
        .call();

    match res {
        Ok(resp) => {
            let body = resp.into_string().unwrap_or_default();
            serde_json::from_str(&body)
                .map_err(|_| FetchError::failed("Kimi usage response was not JSON."))
        }
        Err(ureq::Error::Status(401, _)) => {
            // Token might just have expired; try one refresh
            if !refresh_token.is_empty() {
                if let Ok(new_tokens) = refresh_oauth_token(oauth_host, refresh_token) {
                    if let Some(tok) = new_tokens["access_token"].as_str() {
                        let now_secs = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(0);
                        if let Some(mut creds) = read_credentials(cred_path) {
                            creds["access_token"] = json!(tok);
                            let expires_in = new_tokens["expires_in"].as_i64().unwrap_or(900);
                            creds["expires_in"] = json!(expires_in);
                            creds["expires_at"] = json!(now_secs + expires_in);
                            write_credentials(cred_path, &creds);
                        }

                        let retry = agent
                            .get(&url)
                            .set("Authorization", &format!("Bearer {tok}"))
                            .set("Accept", "application/json")
                            .call();

                        if let Ok(resp) = retry {
                            let body = resp.into_string().unwrap_or_default();
                            return serde_json::from_str(&body).map_err(|_| {
                                FetchError::failed("Kimi usage response was not JSON.")
                            });
                        }
                    }
                }
            }
            Err(FetchError::unavailable(
                "Kimi sign-in expired. Run kimi login, then refresh.",
            ))
        }
        Err(ureq::Error::Status(status, _)) => {
            // Try fallback host if base_url is api.kimi.com -> api.kimi.ai or vice versa
            let alt_base = if base_url.contains("api.kimi.com") {
                base_url.replace("api.kimi.com", "api.kimi.ai")
            } else {
                base_url.replace("api.kimi.ai", "api.kimi.com")
            };
            if alt_base != base_url {
                let alt_url = format!("{}/usages", alt_base.trim_end_matches('/'));
                if let Ok(resp) = agent
                    .get(&alt_url)
                    .set("Authorization", &format!("Bearer {access_token}"))
                    .set("Accept", "application/json")
                    .call()
                {
                    let body = resp.into_string().unwrap_or_default();
                    return serde_json::from_str(&body)
                        .map_err(|_| FetchError::failed("Kimi usage response was not JSON."));
                }
            }
            Err(FetchError::failed(format!(
                "Kimi usage request failed ({status})."
            )))
        }
        Err(e) => Err(FetchError::failed(format!("Could not reach Kimi: {e}"))),
    }
}

fn fetch_identity(base_url: &str, access_token: &str) -> Option<crate::accounts::Identity> {
    let url = format!("{}/me", base_url.trim_end_matches('/'));
    let agent = monobot_http::agent()
        .timeout(Duration::from_secs(5))
        .build();
    let res = agent
        .get(&url)
        .set("Authorization", &format!("Bearer {access_token}"))
        .set("Accept", "application/json")
        .call()
        .ok()?;

    let body = res.into_string().ok()?;
    let val: Value = serde_json::from_str(&body).ok()?;

    Some(crate::accounts::Identity {
        name: val["nickname"].as_str().map(str::to_string),
        email: val["email"].as_str().map(str::to_string),
        plan: val["user_level_name"].as_str().map(str::to_string),
        organization: None,
    })
}

fn parse_kimi_usage(
    val: &Value,
    identity: Option<crate::accounts::Identity>,
) -> Result<Snapshot, FetchError> {
    let mut windows = Vec::new();
    let usages = val.get("usages").unwrap_or(val);

    // 5h window
    if let Some(limit_5h) = usages.get("limit_5h").and_then(Value::as_object) {
        if let Some(ratio) = limit_5h.get("used_ratio").and_then(Value::as_f64) {
            windows.push(Window {
                used_percent: (ratio * 100.0).clamp(0.0, 100.0),
                minutes: 300,
                resets_at: limit_5h.get("reset_time").and_then(super::timestamp),
            });
        }
    }

    // 7d window (if available)
    if let Some(limit_7d) = usages.get("limit_7d").and_then(Value::as_object) {
        if let Some(ratio) = limit_7d.get("used_ratio").and_then(Value::as_f64) {
            windows.push(Window {
                used_percent: (ratio * 100.0).clamp(0.0, 100.0),
                minutes: 10080,
                resets_at: limit_7d.get("reset_time").and_then(super::timestamp),
            });
        }
    }

    // Monthly window
    if let Some(month) = usages.get("limit_month_total").and_then(Value::as_object) {
        if let Some(ratio) = month.get("used_ratio").and_then(Value::as_f64) {
            windows.push(Window {
                used_percent: (ratio * 100.0).clamp(0.0, 100.0),
                minutes: 43200,
                resets_at: month.get("reset_time").and_then(super::timestamp),
            });
        }
    }

    // Fallback: limits array if usages was empty
    if windows.is_empty() {
        if let Some(limits) = val.get("limits").and_then(Value::as_array) {
            for item in limits {
                let duration = item["window"]["duration"].as_u64().unwrap_or(0);
                let time_unit = item["window"]["timeUnit"].as_str().unwrap_or("");
                let minutes = match time_unit {
                    "TIME_UNIT_MINUTE" => duration,
                    "TIME_UNIT_HOUR" => duration * 60,
                    "TIME_UNIT_DAY" => duration * 1440,
                    _ => duration,
                };
                if minutes > 0 {
                    let limit: f64 = item["detail"]["limit"]
                        .as_str()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0.0);
                    let remaining: f64 = item["detail"]["remaining"]
                        .as_str()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0.0);
                    let used_percent = if limit > 0.0 {
                        ((limit - remaining) / limit * 100.0).clamp(0.0, 100.0)
                    } else {
                        0.0
                    };
                    let resets_at = item["detail"].get("resetTime").and_then(super::timestamp);
                    windows.push(Window {
                        used_percent,
                        minutes,
                        resets_at,
                    });
                }
            }
        }
    }

    let extra_usage_balance = val
        .get("boosterWallet")
        .or_else(|| val.get("booster_wallet"))
        .or_else(|| val.get("extraUsage"))
        .and_then(|w| w.get("balanceCents").or_else(|| w.get("balance_cents")))
        .and_then(Value::as_f64)
        .map(|cents| cents / 100.0);

    if windows.is_empty() {
        return Err(FetchError::unavailable(
            "No Kimi subscription usage available for this account.",
        ));
    }

    Ok(Snapshot {
        windows,
        updated_at: now_ms(),
        reset_credits: None,
        extra_usage_balance,
        identity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_kimi_payload() {
        let payload = json!({
            "usages": {
                "limit_5h": {
                    "used_ratio": 0.25,
                    "reset_time": "2026-11-07T19:55:11Z"
                },
                "limit_month_total": {
                    "used_ratio": 0.5426,
                    "reset_time": "2026-11-26T00:00:00Z"
                }
            },
            "boosterWallet": {
                "balanceCents": 1250
            }
        });

        let snapshot = parse_kimi_usage(&payload, None).unwrap();
        assert_eq!(snapshot.windows.len(), 2);
        assert_eq!(snapshot.windows[0].minutes, 300);
        assert!((snapshot.windows[0].used_percent - 25.0).abs() < 1e-4);
        assert_eq!(snapshot.windows[1].minutes, 43200);
        assert!((snapshot.windows[1].used_percent - 54.26).abs() < 1e-4);
        assert_eq!(snapshot.extra_usage_balance, Some(12.50));
    }

    #[test]
    #[ignore = "requires local ~/.kimi-code credentials"]
    fn test_live_kimi_fetch_integration() {
        let profile = crate::accounts::Profile::default();
        let res = fetch_for(&profile);
        println!("Live Kimi fetch result: {res:?}");
        assert!(
            res.is_ok(),
            "Expected Kimi usage fetch to succeed, got: {res:?}"
        );
        let snapshot = res.unwrap();
        assert!(!snapshot.windows.is_empty(), "Expected at least one window");
        println!("Snapshot windows: {:?}", snapshot.windows);
        println!("Snapshot identity: {:?}", snapshot.identity);
    }
}
