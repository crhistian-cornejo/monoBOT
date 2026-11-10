//! Spotify's own queue for the notch's player ("Playing next"). The system's
//! media session tells what plays, never what comes after; Spotify's Web API
//! does, once the user signs in with their own app's Client ID (Spotify
//! gives every developer account one). The sign-in is OAuth with PKCE, so
//! there is no client secret; the refresh token goes to the system keychain
//! and nowhere else. Everything here blocks: run it off the interface thread.
use base64::Engine;
use sha2::Digest;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

/// Where Spotify sends the browser back. The user registers exactly this in
/// their Spotify app's settings.
pub const REDIRECT: &str = "http://127.0.0.1:43917/callback";
const PORT: u16 = 43917;
const SCOPES: &str = "user-read-playback-state user-read-currently-playing";
const ACCOUNT: &str = "spotify-refresh";
/// How long the browser's sign-in may take.
const SIGN_IN: Duration = Duration::from_secs(180);
/// How many tracks the notch lists.
pub const SHOWN: usize = 4;
const NOT_THE_ANSWER: &str = "Not the sign-in's answer.";

/// One track or episode waiting in the queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Next {
    pub title: String,
    pub artist: String,
}

/// A Client ID as Spotify writes them: 32 hex digits.
pub fn valid_client(id: &str) -> bool {
    id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit())
}

/// Whether the user signed in (a refresh token is in the keychain).
pub fn connected() -> bool {
    crate::voice::keys::secret(ACCOUNT).is_some()
}

/// Forgets the sign-in.
pub fn disconnect() {
    let _ = crate::voice::keys::set_secret(ACCOUNT, None);
}

fn url_safe(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// PKCE's challenge for a verifier: its SHA-256, base64url without padding.
pub fn challenge(verifier: &str) -> String {
    url_safe(&sha2::Sha256::digest(verifier.as_bytes()))
}

fn random() -> String {
    let bytes: [u8; 32] = rand::random();
    url_safe(&bytes)
}

/// The page the browser opens for the user to allow the app.
pub fn authorize_url(client: &str, challenge: &str, state: &str) -> String {
    let mut url = url::Url::parse("https://accounts.spotify.com/authorize").expect("a static URL");
    url.query_pairs_mut()
        .append_pair("client_id", client)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", REDIRECT)
        .append_pair("code_challenge_method", "S256")
        .append_pair("code_challenge", challenge)
        .append_pair("scope", SCOPES)
        .append_pair("state", state);
    url.into()
}

/// The code in the browser's request back (`GET /callback?code=…&state=…`),
/// checked against the state this sign-in started with.
pub fn code_from_request(line: &str, state: &str) -> Result<String, String> {
    let unread = || "Spotify's answer could not be read.".to_owned();
    let path = line.split_whitespace().nth(1).ok_or_else(unread)?;
    let url = url::Url::parse(&format!("http://127.0.0.1{path}")).map_err(|_| unread())?;
    if url.path() != "/callback" {
        return Err(NOT_THE_ANSWER.into());
    }
    let find = |name: &str| {
        url.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };
    if let Some(error) = find("error") {
        return Err(if error == "access_denied" {
            "The sign-in was cancelled.".into()
        } else {
            format!("Spotify refused the sign-in ({error}).")
        });
    }
    if find("state").as_deref() != Some(state) {
        return Err("The sign-in's answer did not match its request.".into());
    }
    find("code").ok_or_else(|| "Spotify sent no code.".into())
}

/// A sign-in under way: the listener the browser comes back to.
pub struct SignIn {
    client: String,
    verifier: String,
    state: String,
    listener: TcpListener,
    /// The page to open in the browser.
    pub url: String,
}

/// Starts a sign-in: listens for the browser's return and says which page
/// to open.
pub fn begin(client: &str) -> Result<SignIn, String> {
    if !valid_client(client) {
        return Err("A Spotify Client ID is 32 letters and digits.".into());
    }
    let listener = TcpListener::bind(("127.0.0.1", PORT))
        .map_err(|_| format!("Port {PORT} is busy; close what uses it and try again."))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let (verifier, state) = (random(), random());
    let url = authorize_url(client, &challenge(&verifier), &state);
    Ok(SignIn {
        client: client.to_owned(),
        verifier,
        state,
        listener,
        url,
    })
}

impl SignIn {
    /// Waits for the browser, trades the code for tokens and keeps the
    /// refresh token in the keychain.
    pub fn finish(self) -> Result<(), String> {
        let code = self.wait()?;
        let tokens = token_request(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", REDIRECT),
            ("client_id", &self.client),
            ("code_verifier", &self.verifier),
        ])?;
        let refresh = tokens
            .refresh
            .ok_or_else(|| "Spotify sent no refresh token.".to_owned())?;
        crate::voice::keys::set_secret(ACCOUNT, Some(&refresh)).map_err(|error| error.to_string())
    }

    fn wait(&self) -> Result<String, String> {
        let deadline = Instant::now() + SIGN_IN;
        loop {
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                    let mut buffer = [0u8; 4096];
                    let read = stream.read(&mut buffer).unwrap_or(0);
                    let request = String::from_utf8_lossy(&buffer[..read]);
                    let result =
                        code_from_request(request.lines().next().unwrap_or(""), &self.state);
                    let said = match &result {
                        Ok(_) => "monoBOT is connected to Spotify. You can close this tab.",
                        Err(_) => "monoBOT could not connect to Spotify. Try again from the notch.",
                    };
                    let body = format!(
                        "<!doctype html><meta charset=\"utf-8\"><title>monoBOT</title><body style=\"font:16px system-ui;margin:3em\">{said}</body>"
                    );
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    // A browser may ask for its icon too; only the callback
                    // ends the wait.
                    match result {
                        Err(error) if error == NOT_THE_ANSWER => {}
                        done => return done,
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        return Err("The sign-in took too long.".into());
                    }
                    std::thread::sleep(Duration::from_millis(150));
                }
                Err(error) => return Err(error.to_string()),
            }
        }
    }
}

struct Tokens {
    access: String,
    refresh: Option<String>,
    expires_in: u64,
}

fn agent() -> ureq::Agent {
    monobot_http::agent()
        .timeout(Duration::from_secs(10))
        .build()
}

fn token_request(form: &[(&str, &str)]) -> Result<Tokens, String> {
    let response = agent()
        .post("https://accounts.spotify.com/api/token")
        .send_form(form)
        .map_err(|error| match error {
            ureq::Error::Status(400, _) => {
                "Spotify no longer accepts this sign-in; connect again.".to_owned()
            }
            other => other.to_string(),
        })?;
    let body = response.into_string().map_err(|error| error.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|error| error.to_string())?;
    Ok(Tokens {
        access: json["access_token"]
            .as_str()
            .ok_or("Spotify sent no access token.")?
            .to_owned(),
        refresh: json["refresh_token"].as_str().map(str::to_owned),
        expires_in: json["expires_in"].as_u64().unwrap_or(3600),
    })
}

/// The queue's first tracks out of `GET /me/player/queue`'s answer.
pub fn parse_queue(body: &str) -> Vec<Next> {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    json["queue"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let title = item["name"].as_str()?.trim();
            if title.is_empty() {
                return None;
            }
            let artists: Vec<&str> = item["artists"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|artist| artist["name"].as_str())
                .collect();
            let artist = if artists.is_empty() {
                // An episode names its show instead.
                item["show"]["name"].as_str().unwrap_or("").to_owned()
            } else {
                artists.join(", ")
            };
            Some(Next {
                title: title.to_owned(),
                artist,
            })
        })
        .take(SHOWN)
        .collect()
}

/// The signed-in user's access, renewed when it runs out.
pub struct Session {
    client: String,
    access: Option<(String, Instant)>,
}

impl Session {
    pub fn new(client: &str) -> Self {
        Self {
            client: client.to_owned(),
            access: None,
        }
    }

    fn access(&mut self) -> Result<String, String> {
        if let Some((token, until)) = &self.access {
            if Instant::now() < *until {
                return Ok(token.clone());
            }
        }
        let refresh = crate::voice::keys::secret(ACCOUNT)
            .ok_or_else(|| "Not connected to Spotify.".to_owned())?;
        let tokens = token_request(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
            ("client_id", &self.client),
        ])?;
        // Spotify may hand out a new refresh token with each renewal.
        if let Some(next) = tokens.refresh.filter(|next| *next != refresh) {
            let _ = crate::voice::keys::set_secret(ACCOUNT, Some(&next));
        }
        let until = Instant::now() + Duration::from_secs(tokens.expires_in.saturating_sub(60));
        self.access = Some((tokens.access.clone(), until));
        Ok(tokens.access)
    }

    /// What plays next, in order.
    pub fn queue(&mut self) -> Result<Vec<Next>, String> {
        let access = self.access()?;
        let response = agent()
            .get("https://api.spotify.com/v1/me/player/queue")
            .set("Authorization", &format!("Bearer {access}"))
            .call();
        match response {
            Ok(response) => Ok(parse_queue(
                &response.into_string().map_err(|error| error.to_string())?,
            )),
            Err(ureq::Error::Status(401, _)) => {
                self.access = None;
                Err("Spotify asked to sign in again.".into())
            }
            Err(ureq::Error::Status(403, _)) => {
                Err("Spotify does not share this account's queue.".into())
            }
            Err(ureq::Error::Status(429, _)) => Err("Spotify asked to slow down.".into()),
            Err(error) => Err(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_challenge_is_the_verifiers_hash_as_the_rfc_shows() {
        // RFC 7636, appendix B.
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let url = authorize_url(&"a".repeat(32), "challenge", "state");
        assert!(url.starts_with("https://accounts.spotify.com/authorize?client_id=aaaa"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A43917%2Fcallback"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(valid_client(&"0f".repeat(16)));
        assert!(!valid_client("my client id"));
        assert!(begin("nope").is_err());
    }

    #[test]
    fn the_browsers_return_gives_the_code_only_for_this_sign_in() {
        let ok = "GET /callback?code=abc123&state=s1 HTTP/1.1";
        assert_eq!(code_from_request(ok, "s1").as_deref(), Ok("abc123"));
        assert!(code_from_request(ok, "other").is_err());
        assert_eq!(
            code_from_request("GET /callback?error=access_denied&state=s1 HTTP/1.1", "s1"),
            Err("The sign-in was cancelled.".into())
        );
        assert_eq!(
            code_from_request("GET /favicon.ico HTTP/1.1", "s1"),
            Err(NOT_THE_ANSWER.into())
        );
        assert!(code_from_request("", "s1").is_err());
    }

    #[test]
    fn the_queue_lists_tracks_and_episodes_in_order() {
        let body = r#"{"currently_playing":{"name":"Now"},"queue":[
            {"name":"One","artists":[{"name":"A"},{"name":"B"}]},
            {"name":"  ","artists":[]},
            {"name":"Talk","show":{"name":"The Show"}},
            {"name":"3","artists":[{"name":"C"}]},{"name":"4","artists":[]},
            {"name":"5","artists":[]},{"name":"6","artists":[]}]}"#;
        let queue = parse_queue(body);
        assert_eq!(queue.len(), SHOWN);
        assert_eq!(
            queue[0],
            Next {
                title: "One".into(),
                artist: "A, B".into()
            }
        );
        assert_eq!(queue[1].artist, "The Show");
        assert!(parse_queue("not json").is_empty());
        assert!(parse_queue("{}").is_empty());
    }
}
