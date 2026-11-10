//! What an agent may do on the user's Mac, as IVY's `ivy-hands` does it:
//! open an app or a page, search, control the music, set the volume, read
//! or set the clipboard, type into the front window, and look at the screen.
//!
//! Each action is first turned into a `Plan` (a program and its arguments),
//! a pure function that checks the input and is tested; only then is it run.
//! Nothing goes through a shell, and every value reaching AppleScript is
//! escaped as an AppleScript string. Only agents the user gave the
//! `computer` access may call these (`mono::bot_tools::ACCESS`).
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;

/// A program to run and its arguments; never through a shell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub program: &'static str,
    pub args: Vec<String>,
    /// Text for the program's standard input (pbcopy).
    pub stdin: Option<String>,
}

impl Plan {
    fn new(program: &'static str, args: impl IntoIterator<Item = String>) -> Self {
        Self {
            program,
            args: args.into_iter().collect(),
            stdin: None,
        }
    }
}

const OPEN: &str = "/usr/bin/open";
const OSASCRIPT: &str = "/usr/bin/osascript";
/// The longest text `keys.type` types and the clipboard takes.
const MAX_TEXT: usize = 10_000;

/// Text as an AppleScript string literal.
pub fn apple_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\" & linefeed & \""),
            '\r' => {}
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn text<'a>(input: &'a Value, key: &str) -> Result<&'a str, String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{key} is required"))
}

/// An app's name as the user would say it: letters, digits, spaces and
/// a few marks, never a path or an option.
fn app_name(input: &Value) -> Result<String, String> {
    let name = text(input, "name")?;
    let fine = name.len() <= 80
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || " .&+'-_()".contains(c));
    if fine {
        Ok(name.to_owned())
    } else {
        Err(format!("{name:?} is not an app name"))
    }
}

/// `apps.open {name}` / `apps.quit {name}`.
pub fn app_plan(action: &str, input: &Value) -> Result<Plan, String> {
    let name = app_name(input)?;
    match action {
        "apps.open" => Ok(Plan::new(OPEN, ["-a".into(), name])),
        "apps.quit" => Ok(Plan::new(
            OSASCRIPT,
            [
                "-e".into(),
                format!("tell application {} to quit", apple_string(&name)),
            ],
        )),
        _ => Err("Unknown apps action".into()),
    }
}

/// A web address to open: http or https only, never a file or a script.
pub fn web_url(url: &str) -> Result<String, String> {
    let url = url.trim();
    let lower = url.to_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://"))
        || url.chars().any(char::is_whitespace)
        || url.len() > 4_000
    {
        return Err("Only http and https addresses open".into());
    }
    Ok(url.to_owned())
}

fn encode(query: &str) -> String {
    query
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            b' ' => "+".into(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// `web.open {url, browser?}` / `web.search {query, site?}` where site is
/// google (the default) or youtube.
pub fn web_plan(action: &str, input: &Value) -> Result<Plan, String> {
    let url = match action {
        "web.open" => web_url(text(input, "url")?)?,
        "web.search" => {
            let query = text(input, "query")?;
            match input
                .get("site")
                .and_then(Value::as_str)
                .unwrap_or("google")
            {
                "google" => format!("https://www.google.com/search?q={}", encode(query)),
                "youtube" => format!(
                    "https://www.youtube.com/results?search_query={}",
                    encode(query)
                ),
                other => {
                    return Err(format!(
                        "{other} is not a search site; use google or youtube"
                    ))
                }
            }
        }
        _ => return Err("Unknown web action".into()),
    };
    let mut args = Vec::new();
    if let Some(browser) = input.get("browser") {
        args.push("-a".into());
        args.push(app_name(&json!({ "name": browser }))?);
    }
    args.push(url);
    Ok(Plan::new(OPEN, args))
}

/// The music app an action talks to: Spotify unless the user said Music.
fn player(input: &Value) -> Result<&'static str, String> {
    match input
        .get("app")
        .and_then(Value::as_str)
        .unwrap_or("spotify")
    {
        "spotify" | "Spotify" => Ok("Spotify"),
        "music" | "Music" => Ok("Music"),
        other => Err(format!("{other} is not a music app; use spotify or music")),
    }
}

/// `media.control {command, app?}`: play, pause, toggle, next, previous.
/// `media.play {query}` searches Spotify for it. `media.now {app?}` says
/// what is playing.
pub fn media_plan(action: &str, input: &Value) -> Result<Plan, String> {
    match action {
        "media.control" => {
            let app = player(input)?;
            let verb = match text(input, "command")? {
                "play" => "play",
                "pause" => "pause",
                "toggle" => "playpause",
                "next" => "next track",
                "previous" => "previous track",
                other => return Err(format!("{other} is not a player command")),
            };
            Ok(Plan::new(
                OSASCRIPT,
                [
                    "-e".into(),
                    format!("tell application {} to {verb}", apple_string(app)),
                ],
            ))
        }
        "media.play" => {
            let query = text(input, "query")?;
            if query.len() > 300 {
                return Err("That search is too long".into());
            }
            Ok(Plan::new(
                OPEN,
                [format!(
                    "spotify:search:{}",
                    encode(query).replace('+', "%20")
                )],
            ))
        }
        "media.now" => {
            let app = apple_string(player(input)?);
            Ok(Plan::new(
                OSASCRIPT,
                [
                    "-e".into(),
                    format!(
                        "if application {app} is running then tell application {app} to if player state is playing then return (name of current track) & \" — \" & (artist of current track)"
                    ),
                ],
            ))
        }
        _ => Err("Unknown media action".into()),
    }
}

/// `system.volume {level}` (0–100), `clipboard.read {}`, `clipboard.write {text}`.
pub fn system_plan(action: &str, input: &Value) -> Result<Plan, String> {
    match action {
        "system.volume" => {
            let level = input
                .get("level")
                .and_then(Value::as_u64)
                .filter(|level| *level <= 100)
                .ok_or("level is a number from 0 to 100")?;
            Ok(Plan::new(
                OSASCRIPT,
                ["-e".into(), format!("set volume output volume {level}")],
            ))
        }
        "clipboard.read" => Ok(Plan::new("/usr/bin/pbpaste", [])),
        "clipboard.write" => {
            let value = text(input, "text")?;
            if value.len() > MAX_TEXT {
                return Err("That text is too long for the clipboard".into());
            }
            let mut plan = Plan::new("/usr/bin/pbcopy", []);
            plan.stdin = Some(value.to_owned());
            Ok(plan)
        }
        _ => Err("Unknown system action".into()),
    }
}

/// `keys.type {text, app?}`: types into the front window, or into `app`
/// after bringing it to the front. Needs the Accessibility permission.
pub fn keys_plan(input: &Value) -> Result<Plan, String> {
    let value = text(input, "text")?;
    if value.len() > MAX_TEXT {
        return Err("That text is too long to type".into());
    }
    let mut script = String::new();
    if input.get("app").is_some() {
        let app = app_name(&json!({ "name": input["app"] }))?;
        script.push_str(&format!(
            "tell application {} to activate\ndelay 0.3\n",
            apple_string(&app)
        ));
    }
    script.push_str(&format!(
        "tell application \"System Events\" to keystroke {}",
        apple_string(value)
    ));
    Ok(Plan::new(OSASCRIPT, ["-e".into(), script]))
}

/// `screen.look {}`: a picture of the main screen, saved for the agent to
/// read. Needs the Screen Recording permission.
pub fn screen_plan(path: &std::path::Path) -> Plan {
    Plan::new(
        "/usr/sbin/screencapture",
        [
            "-x".into(),
            "-t".into(),
            "jpg".into(),
            "-m".into(),
            path.to_string_lossy().into_owned(),
        ],
    )
}

/// Every action here, for the bridge's validation.
pub const ACTIONS: [&str; 12] = [
    "apps.open",
    "apps.quit",
    "web.open",
    "web.search",
    "media.control",
    "media.play",
    "media.now",
    "system.volume",
    "clipboard.read",
    "clipboard.write",
    "keys.type",
    "screen.look",
];

/// The plan for `action`, with where a screenshot goes.
pub fn plan(
    action: &str,
    input: &Value,
    shots: &std::path::Path,
) -> Result<(Plan, Option<PathBuf>), String> {
    let plan = match action {
        "apps.open" | "apps.quit" => app_plan(action, input)?,
        "web.open" | "web.search" => web_plan(action, input)?,
        "media.control" | "media.play" | "media.now" => media_plan(action, input)?,
        "system.volume" | "clipboard.read" | "clipboard.write" => system_plan(action, input)?,
        "keys.type" => keys_plan(input)?,
        "screen.look" => {
            let path = shots.join(format!("screen-{}.jpg", crate::time::now_ms()));
            return Ok((screen_plan(&path), Some(path)));
        }
        _ => return Err("Unknown computer action".into()),
    };
    Ok((plan, None))
}

/// Runs `action` on this Mac and says what happened.
pub fn run(action: &str, input: &Value, shots: &std::path::Path) -> Result<Value, String> {
    if !cfg!(target_os = "macos") {
        return Err("Computer actions work on macOS for now".into());
    }
    let (plan, shot) = plan(action, input, shots)?;
    if let Some(parent) = shot.as_ref().and_then(|path| path.parent()) {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let output = execute(&plan)?;
    if let Some(path) = shot {
        if !path.exists() {
            return Err("No picture was taken. Allow monoBOT under System Settings › Privacy & Security › Screen Recording.".into());
        }
        return Ok(json!({"path": path, "note": "Open this image to see the screen."}));
    }
    Ok(match action {
        "clipboard.read" | "media.now" => json!({"text": output.trim()}),
        _ => json!({"done": true}),
    })
}

fn execute(plan: &Plan) -> Result<String, String> {
    use std::io::Write;
    let mut command = Command::new(plan.program);
    command.args(&plan.args);
    command.stdin(if plan.stdin.is_some() {
        std::process::Stdio::piped()
    } else {
        std::process::Stdio::null()
    });
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    if let (Some(text), Some(mut stdin)) = (&plan.stdin, child.stdin.take()) {
        stdin
            .write_all(text.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let why = String::from_utf8_lossy(&output.stderr);
    Err(
        if why.contains("not allowed assistive access") || why.contains("1002") {
            "Allow monoBOT under System Settings › Privacy & Security › Accessibility to type."
                .into()
        } else if why.contains("Unable to find application")
            || why.contains("Can’t get application")
        {
            "That app is not on this Mac.".into()
        } else {
            format!("It did not work: {}", why.trim())
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apple_strings_cannot_break_out() {
        assert_eq!(apple_string("Spotify"), "\"Spotify\"");
        assert_eq!(apple_string(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(apple_string("one\ntwo"), "\"one\" & linefeed & \"two\"");
        assert_eq!(apple_string("x\u{7}\ry"), "\"xy\"");
    }

    #[test]
    fn apps_open_and_quit_by_name_only() {
        assert_eq!(
            app_plan("apps.open", &json!({"name": "Visual Studio Code"})).unwrap(),
            Plan::new(OPEN, ["-a".into(), "Visual Studio Code".into()])
        );
        let quit = app_plan("apps.quit", &json!({"name": "Notes"})).unwrap();
        assert_eq!(quit.args[1], "tell application \"Notes\" to quit");
        for bad in [
            "/Applications/Calculator.app",
            "-n",
            "x; rm -rf ~",
            "",
            "a\"b",
        ] {
            assert!(
                app_plan("apps.open", &json!({"name": bad})).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn only_web_addresses_open() {
        assert!(web_plan("web.open", &json!({"url": "https://example.com/a?b=c"})).is_ok());
        for bad in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ftp://x",
            "https://a b",
            "spotify:x",
        ] {
            assert!(web_plan("web.open", &json!({"url": bad})).is_err(), "{bad}");
        }
        let in_safari = web_plan(
            "web.open",
            &json!({"url": "https://a.dev", "browser": "Safari"}),
        )
        .unwrap();
        assert_eq!(in_safari.args, ["-a", "Safari", "https://a.dev"]);
    }

    #[test]
    fn searches_become_encoded_addresses() {
        let google = web_plan("web.search", &json!({"query": "jazz & café"})).unwrap();
        assert_eq!(
            google.args,
            ["https://www.google.com/search?q=jazz+%26+caf%C3%A9"]
        );
        let youtube = web_plan("web.search", &json!({"query": "lofi", "site": "youtube"})).unwrap();
        assert_eq!(
            youtube.args,
            ["https://www.youtube.com/results?search_query=lofi"]
        );
        assert!(web_plan("web.search", &json!({"query": "x", "site": "bing"})).is_err());
    }

    #[test]
    fn music_goes_to_spotify_or_music() {
        let next = media_plan("media.control", &json!({"command": "next"})).unwrap();
        assert_eq!(next.args[1], "tell application \"Spotify\" to next track");
        let pause = media_plan(
            "media.control",
            &json!({"command": "pause", "app": "music"}),
        )
        .unwrap();
        assert_eq!(pause.args[1], "tell application \"Music\" to pause");
        assert!(media_plan("media.control", &json!({"command": "delete"})).is_err());
        assert!(media_plan("media.control", &json!({"command": "play", "app": "vlc"})).is_err());
        let play = media_plan("media.play", &json!({"query": "Bad Bunny"})).unwrap();
        assert_eq!(play.args, ["spotify:search:Bad%20Bunny"]);
        assert!(media_plan("media.now", &json!({})).unwrap().args[1].contains("current track"));
    }

    #[test]
    fn volume_and_clipboard_are_checked() {
        assert_eq!(
            system_plan("system.volume", &json!({"level": 30}))
                .unwrap()
                .args[1],
            "set volume output volume 30"
        );
        assert!(system_plan("system.volume", &json!({"level": 101})).is_err());
        assert!(system_plan("system.volume", &json!({"level": "loud"})).is_err());
        let write = system_plan("clipboard.write", &json!({"text": "hola"})).unwrap();
        assert_eq!(write.program, "/usr/bin/pbcopy");
        assert_eq!(write.stdin.as_deref(), Some("hola"));
        assert!(write.args.is_empty(), "the text never goes in an argument");
        assert!(system_plan(
            "clipboard.write",
            &json!({"text": "x".repeat(MAX_TEXT + 1)})
        )
        .is_err());
    }

    #[test]
    fn typing_escapes_the_text_and_can_focus_an_app() {
        let plan = keys_plan(&json!({"text": "Hola \"mundo\"", "app": "TextEdit"})).unwrap();
        let script = &plan.args[1];
        assert!(script.starts_with("tell application \"TextEdit\" to activate"));
        assert!(script.ends_with("keystroke \"Hola \\\"mundo\\\"\""));
        assert!(keys_plan(&json!({"text": "x", "app": "../evil"})).is_err());
        assert!(keys_plan(&json!({})).is_err());
    }

    #[test]
    fn every_action_has_a_plan_and_unknown_ones_are_refused() {
        let shots = std::env::temp_dir();
        let inputs = [
            json!({"name": "Notes"}),
            json!({"name": "Notes"}),
            json!({"url": "https://a.dev"}),
            json!({"query": "x"}),
            json!({"command": "toggle"}),
            json!({"query": "x"}),
            json!({}),
            json!({"level": 10}),
            json!({}),
            json!({"text": "x"}),
            json!({"text": "x"}),
            json!({}),
        ];
        for (action, input) in ACTIONS.iter().zip(inputs) {
            assert!(plan(action, &input, &shots).is_ok(), "{action}");
        }
        let (look, path) = plan("screen.look", &json!({}), &shots).unwrap();
        assert_eq!(look.program, "/usr/sbin/screencapture");
        assert!(path.unwrap().starts_with(&shots));
        assert!(plan("system.shutdown", &json!({}), &shots).is_err());
    }

    /// Runs the one action that changes nothing on this Mac: what Spotify is
    /// playing. `cargo xtask cargo test -p monocode-core --profile native-dev
    /// -- --ignored --nocapture hands::tests::asks`.
    #[test]
    #[ignore]
    fn asks_this_mac_what_is_playing() {
        let heard = run("media.now", &json!({}), &std::env::temp_dir()).unwrap();
        eprintln!("now playing: {heard}");
        assert!(heard.get("text").is_some());
    }
}
