//! What agents did on each device: every tap, swipe, text, key, launch and
//! link, with the screen half a second after it. The MCP server (one
//! process per agent session) appends; the app's Simulator pane reads it
//! and can run the latest stretch again ("Again · N actions").
//!
//! One JSON line per step in `simulator-history/<udid>/steps.jsonl` in the
//! user's caches, and a small JPEG per step beside it, named by the step's
//! time. Only the newest [`KEEP`] steps stay.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// How many steps a device keeps.
pub const KEEP: usize = 120;
/// The longest pause inside one stretch of work; a longer one starts the
/// next stretch.
pub const STRETCH_GAP_MS: i64 = 5 * 60 * 1000;
/// How long after an action its screen is captured, for animations to end.
const SETTLE: Duration = Duration::from_millis(500);

/// The tools a step can be.
pub const RECORDED: [&str; 9] = [
    "launch",
    "open_url",
    "appearance",
    "tap",
    "swipe",
    "type_text",
    "press_key",
    "press_button",
    "rotate",
];

/// One action an agent took.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    /// Milliseconds since 1970: also the name of its screenshot.
    pub at: i64,
    pub tool: String,
    /// The tool's arguments, without the device.
    pub input: Value,
}

fn root() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("com.monocode.desktop/simulator-history")
}

fn device_dir(udid: &str) -> Option<PathBuf> {
    // A UDID is hex and dashes; anything else must not become a path.
    (!udid.is_empty() && udid.chars().all(|c| c.is_ascii_hexdigit() || c == '-'))
        .then(|| root().join(udid))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn clip(text: &str, limit: usize) -> String {
    let flat = text.replace('\n', " ");
    if flat.chars().count() > limit {
        format!("{}…", flat.chars().take(limit).collect::<String>())
    } else {
        flat
    }
}

impl Step {
    /// The row's title: "Tapped “Sign in”", "Typed “hello”"…
    pub fn title(&self) -> String {
        let input = &self.input;
        let string = |key: &str| input[key].as_str().unwrap_or_default();
        let number = |key: &str| input[key].as_f64().unwrap_or_default().round() as i64;
        match self.tool.as_str() {
            "tap" => {
                let verb = if input["count"].as_u64() == Some(2) {
                    "Double-tapped"
                } else if input["duration"].as_f64().unwrap_or(0.) >= 0.4 {
                    "Long-pressed"
                } else {
                    "Tapped"
                };
                match input["element"].as_str() {
                    Some(element) => format!("{verb} “{}”", clip(element, 40)),
                    None => format!("{verb} ({}, {})", number("x"), number("y")),
                }
            }
            "swipe" => {
                let dx = number("to_x") - number("from_x");
                let dy = number("to_y") - number("from_y");
                let way = if dx.abs() > dy.abs() {
                    if dx > 0 {
                        "right"
                    } else {
                        "left"
                    }
                } else if dy > 0 {
                    "down"
                } else {
                    "up"
                };
                format!("Swiped {way}")
            }
            "type_text" => format!("Typed “{}”", clip(string("text"), 40)),
            "press_key" => {
                let key = string("key");
                let mut name = key.replace('_', " ");
                if let Some(first) = name.get(..1) {
                    name = first.to_uppercase() + &name[1..];
                }
                format!("Pressed {name}")
            }
            "press_button" => "Went to the Home Screen".into(),
            "rotate" => format!("Rotated to {}", string("orientation").replace('_', " ")),
            "launch" => match input["bundle_id"].as_str() {
                Some(bundle) => format!("Launched {bundle}"),
                None => "Launched an app".into(),
            },
            "open_url" => format!("Opened {}", clip(string("url"), 60)),
            "appearance" => format!("Switched to {} mode", string("mode")),
            other => other.replace('_', " "),
        }
    }

    /// The screen after the step, once it has been captured.
    pub fn screenshot(&self, udid: &str) -> Option<PathBuf> {
        let path = device_dir(udid)?.join(format!("{}.jpg", self.at));
        path.exists().then_some(path)
    }
}

/// Appends a step and captures the screen after it, in the background.
/// Failures are dropped: the history is a convenience, never the reason a
/// tool call fails.
pub fn record(udid: &str, tool: &str, input: &Value) {
    if !RECORDED.contains(&tool) {
        return;
    }
    let Some(dir) = device_dir(udid) else { return };
    let mut input = input.clone();
    if let Some(object) = input.as_object_mut() {
        object.remove("device");
    }
    let step = Step {
        at: now_ms(),
        tool: tool.to_owned(),
        input,
    };
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let file = dir.join("steps.jsonl");
    let Ok(line) = serde_json::to_string(&step) else {
        return;
    };
    let appended = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .and_then(|mut f| writeln!(f, "{line}"));
    if appended.is_err() {
        return;
    }
    prune(&dir);
    let udid = udid.to_owned();
    std::thread::spawn(move || {
        std::thread::sleep(SETTLE);
        if let Ok(result) = super::controls::act(
            &udid,
            "screen",
            &serde_json::json!({"scale": 1, "quality": 0.6}),
        ) {
            use base64::Engine as _;
            if let Some(jpeg) = result["jpeg"]
                .as_str()
                .and_then(|data| base64::engine::general_purpose::STANDARD.decode(data).ok())
            {
                let _ = fs::write(dir.join(format!("{}.jpg", step.at)), jpeg);
            }
        }
    });
}

/// Keeps the newest steps and their screenshots.
fn prune(dir: &std::path::Path) {
    let file = dir.join("steps.jsonl");
    let steps = parse(&fs::read_to_string(&file).unwrap_or_default());
    if steps.len() <= KEEP * 2 {
        return;
    }
    let kept = &steps[steps.len() - KEEP..];
    let text: String = kept
        .iter()
        .filter_map(|s| serde_json::to_string(s).ok())
        .map(|line| line + "\n")
        .collect();
    if fs::write(&file, text).is_err() {
        return;
    }
    let oldest = kept.first().map_or(0, |s| s.at);
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let Some(at) = name
            .to_str()
            .and_then(|n| n.strip_suffix(".jpg"))
            .and_then(|n| n.parse::<i64>().ok())
        else {
            continue;
        };
        if at < oldest {
            let _ = fs::remove_file(entry.path());
        }
    }
}

pub(crate) fn parse(text: &str) -> Vec<Step> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<Step>(line).ok())
        .collect()
}

/// The device's steps, oldest first.
pub fn read(udid: &str) -> Vec<Step> {
    device_dir(udid)
        .and_then(|dir| fs::read_to_string(dir.join("steps.jsonl")).ok())
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// Changes whenever a step is added: the pane reads again only then.
pub fn version(udid: &str) -> Option<(u64, SystemTime)> {
    let meta = fs::metadata(device_dir(udid)?.join("steps.jsonl")).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

/// The latest stretch of work, the steps "Again" runs: back to the last
/// launch, without crossing a pause longer than [`STRETCH_GAP_MS`].
pub fn last_stretch(steps: &[Step]) -> &[Step] {
    let mut start = steps.len();
    while start > 0 {
        let step = &steps[start - 1];
        if start < steps.len() && steps[start].at - step.at > STRETCH_GAP_MS {
            break;
        }
        start -= 1;
        if step.tool == "launch" {
            break;
        }
    }
    &steps[start..]
}

/// Runs `steps` again on `udid`, in order, with a short pause between
/// them. Stops at the first failure and says which step it was.
pub fn replay(udid: &str, steps: &[Step], mut progress: impl FnMut(usize)) -> Result<(), String> {
    for (index, step) in steps.iter().enumerate() {
        progress(index);
        let mut input = step.input.clone();
        if let Some(object) = input.as_object_mut() {
            object.insert("device".into(), udid.into());
        }
        super::mcp::perform(&step.tool, &input)
            .and_then(|answer| {
                if answer["isError"] == Value::Bool(true) {
                    Err(answer["content"][0]["text"]
                        .as_str()
                        .unwrap_or("failed")
                        .to_owned())
                } else {
                    Ok(())
                }
            })
            .map_err(|error| format!("Step {} ({}) failed: {error}", index + 1, step.title()))?;
        std::thread::sleep(Duration::from_millis(350));
    }
    progress(steps.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn step(at: i64, tool: &str, input: Value) -> Step {
        Step {
            at,
            tool: tool.into(),
            input,
        }
    }

    #[test]
    fn each_step_reads_as_what_the_agent_did() {
        let cases = [
            (
                step(0, "tap", json!({"element": "Sign in"})),
                "Tapped “Sign in”",
            ),
            (
                step(0, "tap", json!({"x": 10.4, "y": 20.6})),
                "Tapped (10, 21)",
            ),
            (
                step(0, "tap", json!({"x": 1, "y": 2, "duration": 1.0})),
                "Long-pressed (1, 2)",
            ),
            (
                step(0, "tap", json!({"x": 1, "y": 2, "count": 2})),
                "Double-tapped (1, 2)",
            ),
            (
                step(
                    0,
                    "swipe",
                    json!({"from_x": 100, "from_y": 600, "to_x": 110, "to_y": 100}),
                ),
                "Swiped up",
            ),
            (
                step(
                    0,
                    "swipe",
                    json!({"from_x": 300, "from_y": 400, "to_x": 20, "to_y": 410}),
                ),
                "Swiped left",
            ),
            (
                step(0, "type_text", json!({"text": "hello\nworld"})),
                "Typed “hello world”",
            ),
            (
                step(0, "press_key", json!({"key": "return"})),
                "Pressed Return",
            ),
            (
                step(0, "press_button", json!({"button": "home"})),
                "Went to the Home Screen",
            ),
            (
                step(0, "rotate", json!({"orientation": "landscape_left"})),
                "Rotated to landscape left",
            ),
            (
                step(0, "launch", json!({"bundle_id": "com.example.app"})),
                "Launched com.example.app",
            ),
            (
                step(0, "appearance", json!({"mode": "dark"})),
                "Switched to dark mode",
            ),
        ];
        for (step, title) in cases {
            assert_eq!(step.title(), title);
        }
        let long = step(0, "type_text", json!({"text": "x".repeat(60)}));
        assert_eq!(
            long.title().chars().count(),
            "Typed “”…".chars().count() + 40
        );
    }

    #[test]
    fn again_runs_back_to_the_last_launch_without_crossing_a_long_pause() {
        let minute = 60_000;
        let steps = vec![
            step(0, "tap", json!({})),
            step(minute, "launch", json!({"bundle_id": "a"})),
            step(2 * minute, "tap", json!({})),
            step(3 * minute, "type_text", json!({"text": "x"})),
        ];
        assert_eq!(last_stretch(&steps), &steps[1..]);
        // A long pause starts a new stretch, launch or not.
        let paused = vec![
            step(0, "launch", json!({})),
            step(minute, "tap", json!({})),
            step(minute + STRETCH_GAP_MS + 1, "tap", json!({})),
            step(minute + STRETCH_GAP_MS + 2, "swipe", json!({})),
        ];
        assert_eq!(last_stretch(&paused), &paused[2..]);
        // No launch and no pause: everything.
        assert_eq!(last_stretch(&steps[2..]), &steps[2..]);
        assert!(last_stretch(&[]).is_empty());
    }

    #[test]
    fn lines_that_do_not_parse_are_skipped() {
        let line = serde_json::to_string(&step(5, "tap", json!({"x": 1}))).unwrap();
        let steps = parse(&format!("{line}\nnot json\n{line}\n"));
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].at, 5);
    }

    #[test]
    fn only_a_udid_becomes_a_folder() {
        assert!(device_dir("D45843DB-CD73-48D1-8A48-02FC2640B370").is_some());
        assert!(device_dir("../etc").is_none());
        assert!(device_dir("").is_none());
    }
}
