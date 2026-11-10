//! What only Spotify itself knows, for the notch's player: its queue
//! ("Playing next"), where the track and its artists live in Spotify, whether
//! the track is among the Liked Songs, and how shuffle stands. The system's
//! media session tells what plays, never any of that; Spotify's Web API
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
/// Reading what plays and its queue, turning shuffle on or off, and reading
/// and changing the Liked Songs. A sign-in from before the last three keeps
/// working for the queue and is asked to connect again for the rest.
const SCOPES: &str = "user-read-playback-state user-read-currently-playing \
    user-modify-playback-state user-library-read user-library-modify";
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
    /// `spotify:track:…` or an episode's; empty when Spotify gave none.
    pub uri: String,
    /// Its cover's address on Spotify's image servers, the smallest that
    /// still fills a row's thumbnail.
    pub art: Option<String>,
}

/// The largest cover a row asks for: a thumbnail is a few kilobytes.
const ART_LIMIT: u64 = 512 << 10;
/// The side, in pixels, a row's thumbnail needs at twice its size.
const ART_SIDE: u64 = 60;

/// Whether `address` is a picture on Spotify's own image servers, the only
/// place a cover is fetched from.
pub fn is_art(address: &str) -> bool {
    url::Url::parse(address).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some_and(|host| {
                host == "i.scdn.co"
                    || host.ends_with(".scdn.co")
                    || host.ends_with(".spotifycdn.com")
            })
    })
}

/// The cover to show beside a queued item out of its `images`: the
/// smallest one that still fills the thumbnail, else the largest there is.
fn art_of(images: &serde_json::Value) -> Option<String> {
    let mut sized: Vec<(u64, &str)> = images
        .as_array()?
        .iter()
        .filter_map(|image| {
            let address = image["url"].as_str().filter(|address| is_art(address))?;
            Some((image["width"].as_u64().unwrap_or(0), address))
        })
        .collect();
    sized.sort_by_key(|(width, _)| *width);
    sized
        .iter()
        .find(|(width, _)| *width >= ART_SIDE)
        .or(sized.last())
        .map(|(_, address)| (*address).to_owned())
}

/// A cover's bytes (JPEG, PNG or WebP) from Spotify's image servers.
pub fn fetch_art(address: &str) -> Result<Vec<u8>, String> {
    if !is_art(address) {
        return Err("Not one of Spotify's pictures.".into());
    }
    let response = agent()
        .get(address)
        .call()
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(ART_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.is_empty() || bytes.len() as u64 > ART_LIMIT {
        return Err("Not a thumbnail.".into());
    }
    Ok(bytes)
}

/// How many times "next" reaches the queued item `uri`, which the notch
/// showed in place `shown` (0 for the first): its place in `queue` as
/// Spotify has it now, since the queue may have changed since it was shown;
/// its shown place when it is no longer there or has no address.
pub fn skips_to(queue: &[Next], uri: &str, shown: usize) -> usize {
    let place = (!uri.is_empty())
        .then(|| queue.iter().position(|next| next.uri == uri))
        .flatten();
    place.unwrap_or(shown) + 1
}

/// One of the playing track's artists, with Spotify's own address for them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artist {
    pub name: String,
    /// `spotify:artist:…`
    pub uri: String,
}

/// What Spotify itself says plays now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Now {
    pub title: String,
    /// `spotify:track:…`, or an episode's.
    pub uri: String,
    pub artists: Vec<Artist>,
    pub shuffle: bool,
    /// Whether Spotify lets shuffle be changed where this plays from: it
    /// does in a playlist or an album, not in a radio or between its own
    /// suggestions.
    pub can_shuffle: bool,
}

impl Now {
    /// Whether this is the track the system's controls call `title`. The
    /// Web API learns of a new track a moment after the player does, so an
    /// answer may still be about the track before.
    pub fn is(&self, title: &str) -> bool {
        self.title.trim().to_lowercase() == title.trim().to_lowercase()
    }

    /// Whether it can be kept among the Liked Songs: a track can, an
    /// episode cannot.
    pub fn likeable(&self) -> bool {
        self.uri.starts_with("spotify:track:")
    }
}

/// What plays out of `GET /me/player`'s answer; none while nothing does
/// (Spotify then answers with no body at all).
pub fn parse_now(body: &str) -> Option<Now> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let item = &json["item"];
    let uri = item["uri"].as_str().filter(|uri| is_uri(uri))?;
    let title = item["name"].as_str()?.trim();
    if title.is_empty() {
        return None;
    }
    let artists = item["artists"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|artist| {
            Some(Artist {
                name: artist["name"].as_str()?.trim().to_owned(),
                uri: artist["uri"].as_str().filter(|uri| is_uri(uri))?.to_owned(),
            })
        })
        .filter(|artist| !artist.name.is_empty())
        .collect();
    Some(Now {
        title: title.to_owned(),
        uri: uri.to_owned(),
        artists,
        shuffle: json["shuffle_state"].as_bool().unwrap_or(false),
        can_shuffle: !json["actions"]["disallows"]["toggling_shuffle"]
            .as_bool()
            .unwrap_or(false),
    })
}

/// Whether the first URI asked of `GET /me/library/contains` is saved.
pub fn parse_contains(body: &str) -> Option<bool> {
    serde_json::from_str::<serde_json::Value>(body).ok()?[0].as_bool()
}

/// A Spotify address as its app opens them: `spotify:` and only the
/// letters, digits and colons Spotify writes, so nothing read from the
/// network is handed to the system as some other kind of link.
fn is_uri(uri: &str) -> bool {
    uri.strip_prefix("spotify:").is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b':')
    })
}

fn encoded(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Spotify's own search for `text`, opened in its app: where a title or an
/// artist leads while Spotify's address for it is not known.
pub fn search_link(text: &str) -> String {
    format!("spotify:search:{}", encoded(text.trim()))
}

/// Where the playing track's title leads: the track itself once Spotify
/// said which one it is, else a search for it.
pub fn track_link(now: Option<&Now>, title: &str, artist: &str) -> String {
    match now.filter(|now| now.is(title)) {
        Some(now) => now.uri.clone(),
        None => search_link(&format!("{} {}", title.trim(), artist.trim())),
    }
}

/// The playing track's artists, each with where its name leads: Spotify's
/// own list and addresses once it said them, else the names the system's
/// controls give in one line ("A, B"), each leading to a search.
pub fn artist_links(now: Option<&Now>, title: &str, artist: &str) -> Vec<(String, String)> {
    match now.filter(|now| now.is(title) && !now.artists.is_empty()) {
        Some(now) => now
            .artists
            .iter()
            .map(|artist| (artist.name.clone(), artist.uri.clone()))
            .collect(),
        None => artist
            .split(", ")
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| (name.to_owned(), search_link(name)))
            .collect(),
    }
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
            // A track's cover is its album's; an episode has its own, or
            // its show's.
            let art = art_of(&item["album"]["images"])
                .or_else(|| art_of(&item["images"]))
                .or_else(|| art_of(&item["show"]["images"]));
            Some(Next {
                title: title.to_owned(),
                artist,
                uri: item["uri"]
                    .as_str()
                    .filter(|uri| is_uri(uri))
                    .unwrap_or_default()
                    .to_owned(),
                art,
            })
        })
        .take(SHOWN)
        .collect()
}

/// Everything the player asks Spotify for about the track that plays.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// What plays next, in order.
    pub next: Vec<Next>,
    pub now: Option<Now>,
    /// Whether the track is among the Liked Songs; none when it is not a
    /// track or Spotify would not say.
    pub liked: Option<bool>,
    /// The sign-in is from before the Liked Songs were asked for.
    pub reconnect: bool,
    /// What went wrong first, if anything did.
    pub problem: Option<String>,
}

/// Why Spotify did not do what was asked.
enum Refusal {
    /// The access ran out or was taken back.
    SignIn,
    /// Not allowed: the sign-in lacks the permission, or the account does.
    Denied,
    Other(String),
}

impl Refusal {
    /// What to tell the user, with `denied` for what was not allowed.
    fn said(self, denied: &str) -> String {
        match self {
            Refusal::SignIn => "Spotify asked to sign in again.".into(),
            Refusal::Denied => denied.into(),
            Refusal::Other(error) => error,
        }
    }
}

const NO_QUEUE: &str = "Spotify does not share this account's queue.";
pub const NO_LIBRARY: &str = "Connect Spotify again to save songs from here.";
const NO_SHUFFLE: &str = "Spotify changes shuffle from here only with Premium.";

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

    /// One request of the Web API as the signed-in user; the answer's body.
    fn ask(&mut self, method: &str, path: &str) -> Result<String, Refusal> {
        let access = self.access().map_err(Refusal::Other)?;
        let request = agent()
            .request(method, &format!("https://api.spotify.com/v1{path}"))
            .set("Authorization", &format!("Bearer {access}"));
        let response = if method == "GET" {
            request.call()
        } else {
            // Spotify wants a length even when nothing is sent.
            request.send_bytes(&[])
        };
        match response {
            Ok(response) => response
                .into_string()
                .map_err(|error| Refusal::Other(error.to_string())),
            Err(ureq::Error::Status(401, _)) => {
                self.access = None;
                Err(Refusal::SignIn)
            }
            Err(ureq::Error::Status(403, _)) => Err(Refusal::Denied),
            Err(ureq::Error::Status(429, _)) => {
                Err(Refusal::Other("Spotify asked to slow down.".into()))
            }
            Err(error) => Err(Refusal::Other(error.to_string())),
        }
    }

    /// What plays next, in order.
    pub fn queue(&mut self) -> Result<Vec<Next>, String> {
        self.ask("GET", "/me/player/queue")
            .map(|body| parse_queue(&body))
            .map_err(|refusal| refusal.said(NO_QUEUE))
    }

    /// The track that plays as Spotify knows it, whether it is liked, and
    /// what plays next. Each part is asked for by itself, so one that
    /// Spotify refuses does not take the others with it.
    pub fn snapshot(&mut self) -> Snapshot {
        let mut snapshot = Snapshot::default();
        match self.ask("GET", "/me/player") {
            Ok(body) => snapshot.now = parse_now(&body),
            Err(refusal) => snapshot.problem = Some(refusal.said(NO_QUEUE)),
        }
        if let Some(now) = snapshot.now.as_ref().filter(|now| now.likeable()) {
            let path = format!("/me/library/contains?uris={}", encoded(&now.uri));
            match self.ask("GET", &path) {
                Ok(body) => snapshot.liked = parse_contains(&body),
                Err(Refusal::Denied) => snapshot.reconnect = true,
                Err(refusal) => {
                    snapshot.problem.get_or_insert(refusal.said(NO_LIBRARY));
                }
            }
        }
        match self.queue() {
            Ok(next) => snapshot.next = next,
            Err(error) => {
                snapshot.problem.get_or_insert(error);
            }
        }
        snapshot
    }

    /// Keeps the track among the Liked Songs, or takes it out.
    pub fn like(&mut self, uri: &str, liked: bool) -> Result<(), String> {
        let method = if liked { "PUT" } else { "DELETE" };
        self.ask(method, &format!("/me/library?uris={}", encoded(uri)))
            .map(|_| ())
            .map_err(|refusal| refusal.said(NO_LIBRARY))
    }

    /// Turns shuffle on or off where Spotify plays.
    pub fn shuffle(&mut self, on: bool) -> Result<(), String> {
        self.ask("PUT", &format!("/me/player/shuffle?state={on}"))
            .map(|_| ())
            .map_err(|refusal| refusal.said(NO_SHUFFLE))
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
            {"name":"One","uri":"spotify:track:t1","artists":[{"name":"A"},{"name":"B"}],
                "album":{"images":[
                    {"url":"https://i.scdn.co/image/big","width":640},
                    {"url":"https://i.scdn.co/image/mid","width":300},
                    {"url":"https://i.scdn.co/image/small","width":64}]}},
            {"name":"  ","artists":[]},
            {"name":"Talk","uri":"spotify:episode:e1","show":{"name":"The Show",
                "images":[{"url":"https://i.scdn.co/image/show","width":640}]}},
            {"name":"3","uri":"javascript:alert(1)","artists":[{"name":"C"}],
                "album":{"images":[{"url":"https://evil.example/x.jpg","width":64},
                    {"url":"http://i.scdn.co/image/plain","width":64}]}},
            {"name":"4","artists":[]},
            {"name":"5","artists":[]},{"name":"6","artists":[]}]}"#;
        let queue = parse_queue(body);
        assert_eq!(queue.len(), SHOWN);
        assert_eq!(
            queue[0],
            Next {
                title: "One".into(),
                artist: "A, B".into(),
                uri: "spotify:track:t1".into(),
                // The smallest that fills a thumbnail, not the largest.
                art: Some("https://i.scdn.co/image/small".into()),
            }
        );
        assert_eq!(queue[1].artist, "The Show");
        assert_eq!(
            queue[1].art.as_deref(),
            Some("https://i.scdn.co/image/show")
        );
        // Neither an address that is not Spotify's nor a picture from
        // anywhere but its image servers is kept.
        assert_eq!((queue[2].uri.as_str(), &queue[2].art), ("", &None));
        assert!(is_art("https://image-cdn-ak.spotifycdn.com/image/x"));
        assert!(!is_art("https://i.scdn.co.evil.example/image/x"));
        assert!(fetch_art("https://evil.example/x.jpg").is_err());
        assert!(parse_queue("not json").is_empty());
        assert!(parse_queue("{}").is_empty());
    }

    #[test]
    fn a_chosen_song_is_reached_by_its_place_in_the_queue_as_it_is_now() {
        let next = |uri: &str| Next {
            title: uri.into(),
            artist: String::new(),
            uri: uri.into(),
            art: None,
        };
        let queue = [next("spotify:track:a"), next("spotify:track:b"), next("")];
        // Shown second and still second: two presses of next.
        assert_eq!(skips_to(&queue, "spotify:track:b", 1), 2);
        // The song before it has started since: it is now first.
        assert_eq!(skips_to(&queue[1..], "spotify:track:b", 1), 1);
        // No longer there, or no address to find it by: where it was shown.
        assert_eq!(skips_to(&queue, "spotify:track:z", 2), 3);
        assert_eq!(skips_to(&queue, "", 2), 3);
        assert_eq!(skips_to(&[], "spotify:track:a", 0), 1);
    }

    const PLAYER: &str = r#"{"shuffle_state":true,
        "context":{"type":"playlist","uri":"spotify:playlist:37i9dQ"},
        "item":{"name":"Sirens Tritonia 539","uri":"spotify:track:4uLU6h",
            "artists":[{"name":"anamē","uri":"spotify:artist:1aB"},
                {"name":"Tritonal","uri":"spotify:artist:2cD"},
                {"name":"Odd","uri":"https://evil.example/x"}]},
        "actions":{"disallows":{"resuming":true}}}"#;

    #[test]
    fn what_plays_names_the_track_its_artists_and_how_shuffle_stands() {
        let now = parse_now(PLAYER).unwrap();
        assert_eq!(now.uri, "spotify:track:4uLU6h");
        assert!(now.shuffle && now.can_shuffle && now.likeable());
        // An address that is not Spotify's own is never kept.
        let addresses: Vec<&str> = now.artists.iter().map(|a| a.uri.as_str()).collect();
        assert_eq!(addresses, ["spotify:artist:1aB", "spotify:artist:2cD"]);
        // A radio: Spotify says shuffle cannot be changed there.
        let radio = PLAYER.replace(r#""resuming":true"#, r#""toggling_shuffle":true"#);
        assert!(!parse_now(&radio).unwrap().can_shuffle);
        let episode = parse_now(r#"{"item":{"name":"Talk","uri":"spotify:episode:9zZ"}}"#).unwrap();
        assert!(!episode.likeable() && !episode.shuffle && episode.artists.is_empty());
        // Nothing plays: no body, or no item.
        assert_eq!(parse_now(""), None);
        assert_eq!(parse_now(r#"{"item":null}"#), None);
        assert_eq!(
            parse_now(r#"{"item":{"name":"X","uri":"file:///etc"}}"#),
            None
        );
        assert_eq!(parse_contains("[true]"), Some(true));
        assert_eq!(parse_contains("[false, true]"), Some(false));
        assert_eq!(parse_contains("[]"), None);
        assert_eq!(parse_contains(r#"{"error":{}}"#), None);
    }

    #[test]
    fn a_title_and_its_artists_lead_into_spotify() {
        let now = parse_now(PLAYER).unwrap();
        // Spotify said which track this is: its own addresses.
        assert_eq!(
            track_link(Some(&now), " sirens tritonia 539 ", "anamē"),
            "spotify:track:4uLU6h"
        );
        assert_eq!(
            artist_links(Some(&now), "Sirens Tritonia 539", "whatever"),
            [
                ("anamē".to_owned(), "spotify:artist:1aB".to_owned()),
                ("Tritonal".to_owned(), "spotify:artist:2cD".to_owned())
            ]
        );
        // Not signed in, or the answer is still about the track before: a
        // search, with nothing that could end the link early.
        assert_eq!(
            track_link(Some(&now), "Time / Reload", "Swedish House Mafia"),
            "spotify:search:Time%20%2F%20Reload%20Swedish%20House%20Mafia"
        );
        assert_eq!(
            artist_links(None, "Song", "AC/DC, Beyoncé"),
            [
                ("AC/DC".to_owned(), "spotify:search:AC%2FDC".to_owned()),
                (
                    "Beyoncé".to_owned(),
                    "spotify:search:Beyonc%C3%A9".to_owned()
                )
            ]
        );
        assert!(artist_links(None, "Song", "  ").is_empty());
        // The sign-in asks for the Liked Songs and shuffle too.
        let url = authorize_url(&"a".repeat(32), "challenge", "state");
        assert!(url.contains("user-library-modify") && url.contains("user-modify-playback-state"));
    }
}
