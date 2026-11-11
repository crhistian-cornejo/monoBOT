//! The `ios_simulator` connector's MCP server (`monocode-gpui
//! ios-simulator-mcp`): newline-delimited JSON-RPC on stdio, one process per
//! agent session, so a build started in one call is still known to the next.
use super::{build_status, find_projects, schemes, start_build, Device};
use base64::Engine as _;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{BufRead, Read, Write},
    sync::Mutex,
    time::Duration,
};

const MAX_FRAME: usize = 1_048_576;
pub const SERVER: &str = "ios_simulator";

/// Sent once as the server's instructions; the tool descriptions stay short.
const INSTRUCTIONS: &str = "iOS Simulator tools for building and checking iOS apps on this Mac. \
Typical loop: status (Xcode and devices) → find_projects / schemes → build → build_status until it is \
not running → launch with the app_path it returned → screenshot to see the result → logs when something \
fails. boot shows the device in Xcode's Simulator app. Paths must be absolute. To use the app: describe_ui \
lists the elements on screen with the point at each centre; tap takes those points (or an element's label or \
identifier), and swipe, type_text and press_key act like a finger and the keyboard. Screenshots come at the \
screen's size in points, so a spot in the image is the point to tap. The first touch of a session builds and \
starts monoBOT's test runner on the device, which takes about a minute. When status reports an Xcode setup \
problem, tell the user the exact fix it names; those commands need their password.";

/// The app each device last launched here: the one that typing, keys and
/// describe_ui address unless the call names another.
static LAST_APP: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn remember_app(udid: &str, bundle: &str) {
    if let Ok(mut apps) = LAST_APP.lock() {
        apps.get_or_insert_with(HashMap::new)
            .insert(udid.to_owned(), bundle.to_owned());
    }
}

fn last_app(udid: &str) -> Option<String> {
    LAST_APP.lock().ok()?.as_ref()?.get(udid).cloned()
}

/// The arguments a runner action takes from a tool call, with the app it
/// addresses filled in.
pub(crate) fn runner_body(input: &Value, keys: &[&str], app: Option<String>) -> Value {
    let mut body = serde_json::Map::new();
    for key in keys {
        if let Some(value) = input.get(*key).filter(|value| !value.is_null()) {
            body.insert((*key).to_owned(), value.clone());
        }
    }
    match input["bundle_id"].as_str().filter(|id| !id.is_empty()) {
        Some(named) => {
            body.insert("bundle_id".into(), named.into());
        }
        // The app launched last may have left the screen since.
        None => {
            if let Some(app) = app {
                body.insert("bundle_id".into(), app.into());
                body.insert("guessed".into(), true.into());
            }
        }
    }
    Value::Object(body)
}

fn device_arg(input: &Value) -> Option<&str> {
    input["device"].as_str()
}

fn properties(extra: Value) -> Value {
    let mut properties = json!({
        "device": {"type":"string","description":"Simulator name or UDID. Default: the booted one, else the newest iPhone."}
    });
    if let (Some(all), Some(extra)) = (properties.as_object_mut(), extra.as_object()) {
        all.extend(extra.clone());
    }
    properties
}

fn tool(
    name: &str,
    description: &str,
    properties: Value,
    required: &[&str],
    read_only: bool,
) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {"type":"object","properties":properties,"required":required,"additionalProperties":false},
        "annotations": {"readOnlyHint":read_only,"destructiveHint":false,"openWorldHint":false},
    })
}

pub fn tool_definitions() -> Vec<Value> {
    let path = |what: &str| json!({"type":"string","description":what});
    vec![
        tool("status", "Whether Xcode is ready, the iOS simulators on this Mac (booted first) and which one the other tools use by default.", json!({}), &[], true),
        tool("boot", "Boot a simulator and wait until it has started; shows it in Xcode's Simulator app unless open_simulator_app is false.", properties(json!({"open_simulator_app":{"type":"boolean"}})), &[], false),
        tool("find_projects", "List the .xcworkspace and .xcodeproj under a folder (workspaces first).", json!({"directory": path("Absolute folder, usually the project root.")}), &["directory"], true),
        tool("schemes", "The schemes, targets and configurations of a project or workspace.", json!({"project_path": path("Absolute path to the .xcodeproj or .xcworkspace.")}), &["project_path"], true),
        tool("build", "Start building a scheme for the simulator in the background and return its build_id. Follow it with build_status.", properties(json!({
            "project_path": path("Absolute path to the .xcodeproj or .xcworkspace. Prefer the workspace when there is one."),
            "scheme": {"type":"string"},
            "configuration": {"type":"string","description":"Default Debug."},
        })), &["project_path", "scheme"], false),
        tool("build_status", "A build's state, compile errors and warnings, waiting up to wait_seconds (default 30) for it to finish. On success it has the app_path to launch.", json!({
            "build_id": {"type":"string"},
            "wait_seconds": {"type":"integer","minimum":0,"maximum":60},
        }), &["build_id"], true),
        tool("launch", "Install a built .app (app_path) and start it, or restart an installed app (bundle_id). Boots the device when needed.", properties(json!({
            "app_path": path("Absolute path to the built .app."),
            "bundle_id": {"type":"string"},
        })), &[], false),
        tool("screenshot", "See the booted simulator's screen. save_to keeps the full-resolution PNG at that absolute path.", properties(json!({"save_to": path("Optional absolute path ending in .png.")})), &[], true),
        tool("logs", "The device log of the last seconds (default 60), for one process when named, to find crashes and print output.", properties(json!({
            "process": {"type":"string","description":"The app's executable name."},
            "seconds": {"type":"integer","minimum":1,"maximum":600},
            "debug": {"type":"boolean"},
        })), &[], true),
        tool("open_url", "Open a deep link or web URL on the booted simulator.", properties(json!({"url":{"type":"string"}})), &["url"], false),
        tool("appearance", "Switch the booted simulator to light or dark mode.", properties(json!({"mode":{"type":"string","enum":["light","dark"]}})), &["mode"], false),
        tool("describe_ui", "The elements on screen (kind, label, identifier, value, state) with the point at each centre, for the app launched last or bundle_id. Use it to find what to tap.", properties(json!({
            "bundle_id": {"type":"string","description":"Default: the app launched last; com.apple.springboard for the home screen."},
            "max_lines": {"type":"integer","minimum":20,"maximum":3000},
        })), &[], true),
        tool("tap", "Tap a point (x, y in points) or the element with this label or identifier. duration holds it (long press); double taps twice.", properties(json!({
            "x": {"type":"number"}, "y": {"type":"number"},
            "element": {"type":"string","description":"Label or accessibility identifier, instead of x and y."},
            "duration": {"type":"number","minimum":0,"maximum":10},
            "double": {"type":"boolean"},
            "bundle_id": {"type":"string"},
        })), &[], false),
        tool("swipe", "Drag a finger from one point to another, in points: scrolls, pages and dismisses.", properties(json!({
            "from_x": {"type":"number"}, "from_y": {"type":"number"},
            "to_x": {"type":"number"}, "to_y": {"type":"number"},
            "speed": {"type":"string","enum":["slow","default","fast"]},
        })), &["from_x", "from_y", "to_x", "to_y"], false),
        tool("type_text", "Type text into the focused field (tap the field first).", properties(json!({
            "text": {"type":"string"}, "bundle_id": {"type":"string"},
        })), &["text"], false),
        tool("press_key", "Press a key: return, delete, tab, escape, space, up, down, left, right, home, end or one character; modifiers need the hardware keyboard.", properties(json!({
            "key": {"type":"string"},
            "count": {"type":"integer","minimum":1,"maximum":100},
            "modifiers": {"type":"array","items":{"type":"string","enum":["command","shift","option","control"]}},
            "bundle_id": {"type":"string"},
        })), &["key"], false),
        tool("press_button", "Press the device's Home button (goes to the home screen).", properties(json!({"button":{"type":"string","enum":["home"]}})), &["button"], false),
        tool("rotate", "Turn the device.", properties(json!({"orientation":{"type":"string","enum":["portrait","portrait_upside_down","landscape_left","landscape_right"]}})), &["orientation"], false),
    ]
}

fn text(value: Value) -> Value {
    json!({"content":[{"type":"text","text":value.to_string()}],"isError":false})
}

fn device_json(device: &Device) -> Value {
    json!({"name": device.name, "udid": device.udid, "runtime": device.runtime, "state": device.state})
}

fn status() -> Result<Value, String> {
    if let Some(problem) = super::xcode_problem() {
        return Ok(json!({"ready": false, "problem": problem}));
    }
    let devices = super::devices()?;
    let default = devices.first().map(device_json);
    Ok(json!({
        "ready": true,
        "default_device": default,
        "devices": devices.iter().take(40).map(device_json).collect::<Vec<_>>(),
    }))
}

fn destination(input: &Value) -> Result<String, String> {
    let list = super::devices()?;
    let device = super::pick(&list, device_arg(input))?;
    Ok(format!("platform=iOS Simulator,id={}", device.udid))
}

fn call(name: &str, input: &Value) -> Result<Value, String> {
    let device = device_arg(input);
    let string = |key: &str| -> Result<&str, String> {
        input[key]
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| format!("{key} is required"))
    };
    if name != "status" && name != "find_projects" {
        if let Some(problem) = super::xcode_problem() {
            return Err(problem);
        }
    }
    match name {
        "status" => status().map(text),
        "boot" => {
            let open = input["open_simulator_app"].as_bool().unwrap_or(true);
            super::boot(device, open).map(|d| text(json!({"booted": device_json(&d)})))
        }
        "find_projects" => find_projects(string("directory")?).map(|list| text(json!({"projects": list}))),
        "schemes" => schemes(string("project_path")?).map(text),
        "build" => {
            let destination = destination(input)?;
            let configuration = input["configuration"].as_str().unwrap_or("Debug");
            start_build(string("project_path")?, string("scheme")?, configuration, &destination)
                .map(|status| text(json!({"build_id": status.build_id, "state": status.state, "destination": status.destination, "log_path": status.log_path})))
        }
        "build_status" => {
            let wait = input["wait_seconds"].as_u64().unwrap_or(30).min(60);
            build_status(string("build_id")?, Duration::from_secs(wait))
                .and_then(|status| serde_json::to_value(status).map_err(|e| e.to_string()))
                .map(text)
        }
        "launch" => super::launch(device, input["app_path"].as_str(), input["bundle_id"].as_str())
            .inspect(|launched| remember_app(&launched.device.udid, &launched.bundle_id))
            .map(|launched| text(json!({
                "device": device_json(&launched.device),
                "bundle_id": launched.bundle_id,
                "pid": launched.pid,
                "installed": launched.installed,
            }))),
        "screenshot" => super::screenshot(device, input["save_to"].as_str()).map(|shot| {
            let mut note = json!({
                "device": device_json(&shot.device),
                "image": {"width": shot.width, "height": shot.height},
                "screen_points": {"width": shot.point_width, "height": shot.point_height},
                "screen_pixels": {"width": shot.pixel_width, "height": shot.pixel_height},
            });
            if shot.width == shot.point_width {
                note["coordinates"] = "A spot in this image is the same point for tap and swipe.".into();
            }
            if let Some(saved) = shot.saved {
                note["saved_to"] = saved.into();
            }
            json!({"content":[
                {"type":"image","data":base64::engine::general_purpose::STANDARD.encode(&shot.jpeg),"mimeType":"image/jpeg"},
                {"type":"text","text":note.to_string()},
            ],"isError":false})
        }),
        "logs" => super::logs(
            device,
            input["process"].as_str(),
            input["seconds"].as_u64().unwrap_or(60),
            input["debug"].as_bool().unwrap_or(false),
        )
        .map(|log| json!({"content":[{"type":"text","text": if log.trim().is_empty() { "(no log lines)".to_owned() } else { log }}],"isError":false})),
        "open_url" => super::open_url(device, string("url")?).map(|d| text(json!({"opened": true, "device": device_json(&d)}))),
        "appearance" => super::appearance(device, string("mode")?).map(|d| text(json!({"appearance": input["mode"], "device": device_json(&d)}))),
        "describe_ui" => runner(device, "describe", input, &["max_lines"], true)
            .map(|(_, result)| json!({"content":[{"type":"text","text": result["text"].as_str().unwrap_or("(nothing on screen)")}],"isError":false})),
        "tap" => {
            let (action_input, keys): (Value, &[&str]) = if input["double"] == Value::Bool(true) {
                let mut with_count = input.clone();
                with_count["count"] = 2.into();
                (with_count, &["x", "y", "element", "duration", "count"])
            } else {
                (input.clone(), &["x", "y", "element", "duration"])
            };
            let addressed = action_input["element"].is_string();
            runner(device, "tap", &action_input, keys, addressed).map(|(d, r)| done(&d, r))
        }
        "swipe" => runner(device, "swipe", input, &["from_x", "from_y", "to_x", "to_y", "speed"], false)
            .map(|(d, r)| done(&d, r)),
        "type_text" => runner(device, "type_text", input, &["text"], true).map(|(d, r)| done(&d, r)),
        "press_key" => runner(device, "press_key", input, &["key", "count", "modifiers"], true)
            .map(|(d, r)| done(&d, r)),
        "press_button" => runner(device, "press_button", input, &["button"], false)
            .map(|(d, r)| done(&d, r)),
        "rotate" => runner(device, "rotate", input, &["orientation"], false).map(|(d, r)| done(&d, r)),
        _ => Err(format!("Unknown iOS Simulator tool '{name}'")),
    }
}

/// Runs an action on the device's runner. `addressed` actions go to the
/// app launched last unless the call names one.
fn runner(
    device: Option<&str>,
    action: &str,
    input: &Value,
    keys: &[&str],
    addressed: bool,
) -> Result<(Device, Value), String> {
    super::act(device, action, |device| {
        runner_body(
            input,
            keys,
            addressed.then(|| last_app(&device.udid)).flatten(),
        )
    })
}

fn done(device: &Device, result: Value) -> Value {
    let mut answer = json!({"done": true, "device": device.name});
    if let Some(frame) = result.get("frame") {
        answer["frame"] = frame.clone();
    }
    text(answer)
}

pub(crate) fn response(request: &Value) -> Option<Value> {
    let id = request.get("id")?.clone();
    let result = match request["method"].as_str().unwrap_or("") {
        "initialize" => Ok(json!({
            "protocolVersion": request["params"]["protocolVersion"].as_str().unwrap_or("2024-11-05"),
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {"name": SERVER, "version": env!("CARGO_PKG_VERSION")},
            "instructions": INSTRUCTIONS,
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tool_definitions()})),
        "tools/call" => {
            let name = request["params"]["name"].as_str().unwrap_or("");
            let input = &request["params"]["arguments"];
            Ok(call(name, input).unwrap_or_else(
                |error| json!({"content":[{"type":"text","text":error}],"isError":true}),
            ))
        }
        _ => Err(json!({"code":-32601,"message":"Method not found"})),
    };
    Some(match result {
        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
        Err(error) => json!({"jsonrpc":"2.0","id":id,"error":error}),
    })
}

/// Runs before the interface starts. stdout carries MCP frames only.
pub fn stdio() -> Result<(), String> {
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    loop {
        let mut line = String::new();
        (&mut input)
            .take((MAX_FRAME + 1) as u64)
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        if line.is_empty() {
            return Ok(());
        }
        if line.len() > MAX_FRAME || !line.ends_with('\n') {
            return Err("Oversized or incomplete MCP frame".into());
        }
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(request) => response(&request),
            Err(_) => Some(
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}),
            ),
        };
        if let Some(response) = response {
            writeln!(output, "{response}")
                .and_then(|_| output.flush())
                .map_err(|e| e.to_string())?;
        }
    }
}
