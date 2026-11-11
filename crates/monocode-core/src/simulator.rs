//! The iOS Simulator for coding agents, first phase: everything `simctl` and
//! `xcodebuild` do without Apple's private frameworks. It boots a device,
//! builds an Xcode project for the simulator, installs and launches the app,
//! takes screenshots, reads the device log, opens URLs and switches the
//! appearance. The `ios_simulator` connector serves it to agents over MCP
//! ([`mcp`]); touches and the live pane come later (docs/in-progress/ios-simulator.md).
//!
//! Everything here blocks. It runs in the MCP server process the agent CLI
//! starts, never on the app's interface thread.

mod build;
mod input;
pub mod mcp;
#[cfg(test)]
mod tests;

use serde::Serialize;
use serde_json::Value;
use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    thread::JoinHandle,
    time::{Duration, Instant},
};

const XCRUN: &str = "/usr/bin/xcrun";
const XCODE_SELECT: &str = "/usr/bin/xcode-select";
const OPEN: &str = "/usr/bin/open";
const PLUTIL: &str = "/usr/bin/plutil";
/// Installed by Xcode's first launch; without it nothing here works.
const CORE_SIMULATOR: &str = "/Library/Developer/PrivateFrameworks/CoreSimulator.framework";
/// The longest edge of the screenshot the agent receives. The full image can
/// be saved to a file; the agent only needs enough to read the screen.
const SCREENSHOT_EDGE: u32 = 1280;
/// What one command's output may hold before the rest is dropped.
const OUTPUT_LIMIT: usize = 8 * 1024 * 1024;
/// The log text one call returns, its newest end.
const LOG_LIMIT: usize = 48 * 1024;

/// A finished command.
pub(crate) struct Output {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    /// Its stdout when it succeeded, else what it said about the failure.
    pub fn ok(self, what: &str) -> Result<String, String> {
        if self.code == Some(0) {
            return Ok(self.stdout);
        }
        let said = if self.stderr.trim().is_empty() {
            self.stdout.trim()
        } else {
            self.stderr.trim()
        };
        Err(format!(
            "{what} failed ({}): {}",
            self.code
                .map_or_else(|| "killed".to_owned(), |code| format!("exit {code}")),
            tail(said, 2000)
        ))
    }
}

/// Runs a program to the end, or kills it at `timeout`.
pub(crate) fn run(program: &str, args: &[&str], timeout: Duration) -> Result<Output, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let command = std::iter::once(program)
                .chain(args.iter().copied())
                .take(4)
                .collect::<Vec<_>>()
                .join(" ");
            return Err(format!(
                "`{command}` did not finish within {} s and was stopped",
                timeout.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let text = |handle: JoinHandle<Vec<u8>>| {
        String::from_utf8_lossy(&handle.join().unwrap_or_default()).into_owned()
    };
    Ok(Output {
        code: status.code(),
        stdout: text(stdout),
        stderr: text(stderr),
    })
}

fn drain(pipe: Option<impl Read + Send + 'static>) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut out = Vec::new();
        if let Some(pipe) = pipe {
            let _ = pipe.take(OUTPUT_LIMIT as u64).read_to_end(&mut out);
        }
        out
    })
}

/// The end of `text`, at most `limit` bytes, cut at a line when it can be.
pub(crate) fn tail(text: &str, limit: usize) -> &str {
    if text.len() <= limit {
        return text;
    }
    let mut start = text.len() - limit;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let rest = &text[start..];
    rest.find('\n').map_or(rest, |line| &rest[line + 1..])
}

/// Why the simulator cannot run, from what `xcode-select -p` printed and
/// what is on disk. `None` when it can.
pub(crate) fn diagnose(
    selected: Option<&str>,
    has_xcodebuild: bool,
    has_core_simulator: bool,
) -> Option<&'static str> {
    match selected {
        None => Some("Xcode not found. Install Xcode from the App Store, open it once, then run `sudo xcode-select -s /Applications/Xcode.app/Contents/Developer`."),
        Some(path) if path.contains("CommandLineTools") => Some("Only the Command Line Tools are selected, and the iOS Simulator needs the full Xcode. Install Xcode, then run `sudo xcode-select -s /Applications/Xcode.app/Contents/Developer`."),
        Some(_) if !has_xcodebuild => Some("The selected developer folder has no xcodebuild. Run `sudo xcode-select -s /Applications/Xcode.app/Contents/Developer`."),
        Some(_) if !has_core_simulator => Some("Xcode is installed but has not finished setting up. Open Xcode once to install its components, or run `xcodebuild -runFirstLaunch`."),
        Some(_) => None,
    }
}

/// What keeps this Mac from running the simulator, if anything.
pub fn xcode_problem() -> Option<String> {
    if !cfg!(target_os = "macos") {
        return Some("The iOS Simulator needs a Mac with Xcode.".into());
    }
    let selected = run(XCODE_SELECT, &["-p"], Duration::from_secs(10))
        .ok()
        .and_then(|out| out.ok("xcode-select").ok())
        .map(|path| path.trim().to_owned())
        .filter(|path| !path.is_empty());
    let has_xcodebuild = selected
        .as_deref()
        .is_some_and(|path| Path::new(path).join("usr/bin/xcodebuild").exists());
    diagnose(
        selected.as_deref(),
        has_xcodebuild,
        Path::new(CORE_SIMULATOR).exists(),
    )
    .map(str::to_owned)
}

/// One iOS simulator device.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Device {
    pub udid: String,
    pub name: String,
    pub state: String,
    /// As people say it: "iOS 27.1".
    pub runtime: String,
    #[serde(skip)]
    version: Vec<u32>,
}

impl Device {
    pub fn booted(&self) -> bool {
        self.state == "Booted"
    }
}

/// The available iOS devices in `simctl list devices available --json`:
/// booted ones first, then the newest runtime, iPhones before iPads.
pub(crate) fn parse_devices(json: &str) -> Result<Vec<Device>, String> {
    let value: Value =
        serde_json::from_str(json).map_err(|e| format!("simctl device list: {e}"))?;
    let runtimes = value["devices"]
        .as_object()
        .ok_or("simctl device list has no devices")?;
    let mut devices = Vec::new();
    for (runtime, list) in runtimes {
        let Some(version) = runtime
            .rsplit('.')
            .next()
            .and_then(|r| r.strip_prefix("iOS-"))
        else {
            continue;
        };
        let version: Vec<u32> = version.split('-').filter_map(|n| n.parse().ok()).collect();
        let label = format!(
            "iOS {}",
            version
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(".")
        );
        for device in list.as_array().into_iter().flatten() {
            if device["isAvailable"] == Value::Bool(false) {
                continue;
            }
            let (Some(udid), Some(name), Some(state)) = (
                device["udid"].as_str(),
                device["name"].as_str(),
                device["state"].as_str(),
            ) else {
                continue;
            };
            devices.push(Device {
                udid: udid.to_owned(),
                name: name.to_owned(),
                state: state.to_owned(),
                runtime: label.clone(),
                version: version.clone(),
            });
        }
    }
    devices.sort_by(|a, b| {
        (
            !a.booted(),
            std::cmp::Reverse(&a.version),
            !a.name.starts_with("iPhone"),
        )
            .cmp(&(
                !b.booted(),
                std::cmp::Reverse(&b.version),
                !b.name.starts_with("iPhone"),
            ))
    });
    Ok(devices)
}

pub fn devices() -> Result<Vec<Device>, String> {
    let json = run(
        XCRUN,
        &["simctl", "list", "devices", "available", "--json"],
        Duration::from_secs(30),
    )?
    .ok("simctl list devices")?;
    parse_devices(&json)
}

/// The device a call names by UDID or name, or else the first booted one,
/// or else the newest iPhone.
pub(crate) fn pick<'a>(devices: &'a [Device], wanted: Option<&str>) -> Result<&'a Device, String> {
    match wanted.map(str::trim).filter(|w| !w.is_empty()) {
        Some(wanted) => devices
            .iter()
            .find(|d| d.udid.eq_ignore_ascii_case(wanted))
            .or_else(|| devices.iter().find(|d| d.name.eq_ignore_ascii_case(wanted)))
            .ok_or_else(|| {
                format!("No available iOS simulator is called '{wanted}'. Call status to list them.")
            }),
        None => devices.first().ok_or_else(|| {
            "No iOS simulator is available. Open Xcode › Settings › Components and download iOS, or run `xcodebuild -downloadPlatform iOS`.".into()
        }),
    }
}

/// A booted device, or why there is none.
fn booted(wanted: Option<&str>) -> Result<Device, String> {
    let list = devices()?;
    let device = pick(&list, wanted)?;
    if device.booted() {
        Ok(device.clone())
    } else if wanted.is_some() {
        Err(format!("{} is not booted. Call boot first.", device.name))
    } else {
        Err("No simulator is booted. Call boot first.".into())
    }
}

/// Boots a device (a booted one stays as it is) and waits until it has
/// finished starting. `open_app` also shows it in Xcode's Simulator app.
pub fn boot(wanted: Option<&str>, open_app: bool) -> Result<Device, String> {
    let list = devices()?;
    let mut device = pick(&list, wanted)?.clone();
    if !device.booted() {
        let out = run(
            XCRUN,
            &["simctl", "boot", &device.udid],
            Duration::from_secs(120),
        )?;
        if out.code != Some(0) && !out.stderr.contains("current state: Booted") {
            out.ok("simctl boot")?;
        }
    }
    run(
        XCRUN,
        &["simctl", "bootstatus", &device.udid, "-b"],
        Duration::from_secs(300),
    )?
    .ok("simctl bootstatus")?;
    if open_app {
        // Showing the window is a convenience; the device works without it.
        let _ = open_simulator_app(&device.udid);
    }
    device.state = "Booted".into();
    Ok(device)
}

/// Shows the device in Xcode's Simulator app.
pub fn open_simulator_app(udid: &str) -> Result<(), String> {
    run(
        OPEN,
        &["-a", "Simulator", "--args", "-CurrentDeviceUDID", udid],
        Duration::from_secs(30),
    )?
    .ok("open Simulator")
    .map(|_| ())
}

/// The first booted iOS device, if any.
pub fn booted_device() -> Result<Option<Device>, String> {
    Ok(devices()?.into_iter().find(Device::booted))
}

/// One frame for the live pane: the screen at twice its size in points, as
/// BGRA pixels ready for a `RenderImage`, and that size in points.
pub struct LiveFrame {
    pub bgra: image::RgbaImage,
    pub point_width: f32,
    pub point_height: f32,
}

pub fn live_frame(udid: &str) -> Result<LiveFrame, String> {
    use base64::Engine as _;
    let (_, result) = act(
        Some(udid),
        "screen",
        |_| serde_json::json!({"scale": 2, "quality": 0.7}),
    )?;
    let jpeg = base64::engine::general_purpose::STANDARD
        .decode(result["jpeg"].as_str().ok_or("The runner sent no frame")?)
        .map_err(|e| e.to_string())?;
    let mut bgra = image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg)
        .map_err(|e| format!("The frame could not be read: {e}"))?
        .to_rgba8();
    for pixel in bgra.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Ok(LiveFrame {
        bgra,
        point_width: result["width"].as_f64().unwrap_or(0.0) as f32,
        point_height: result["height"].as_f64().unwrap_or(0.0) as f32,
    })
}

fn absolute<'a>(path: &'a str, what: &str) -> Result<&'a Path, String> {
    let path_ref = Path::new(path);
    if path.contains('\0') || !path_ref.is_absolute() {
        return Err(format!(
            "{what} must be an absolute path (got '{}')",
            tail(path, 200)
        ));
    }
    Ok(path_ref)
}

/// A bundle's identifier, from its Info.plist.
fn bundle_id(app: &Path) -> Result<String, String> {
    let plist = app.join("Info.plist");
    let plist = plist.to_string_lossy();
    let id = run(
        PLUTIL,
        &["-extract", "CFBundleIdentifier", "raw", "-o", "-", &plist],
        Duration::from_secs(10),
    )?
    .ok("reading the app's bundle identifier")?;
    let id = id.trim();
    if id.is_empty() {
        return Err("The app's Info.plist has no CFBundleIdentifier".into());
    }
    Ok(id.to_owned())
}

/// What `launch` started.
#[derive(Debug, Serialize)]
pub struct Launched {
    pub device: Device,
    pub bundle_id: String,
    pub pid: Option<u32>,
    pub installed: bool,
}

/// The pid in `simctl launch`'s answer (`com.example.app: 1234`).
pub(crate) fn launched_pid(stdout: &str) -> Option<u32> {
    stdout
        .lines()
        .rev()
        .find_map(|line| line.rsplit(':').next()?.trim().parse().ok())
}

/// Installs `app_path` when given, then starts the app, ending a running
/// copy first. Boots the device when it is not running yet.
pub fn launch(
    wanted: Option<&str>,
    app_path: Option<&str>,
    bundle: Option<&str>,
) -> Result<Launched, String> {
    let device = match booted(wanted) {
        Ok(device) => device,
        Err(_) => boot(wanted, true)?,
    };
    let mut installed = false;
    let mut id = bundle.map(str::to_owned);
    if let Some(app) = app_path {
        let app = absolute(app, "app_path")?;
        if app.extension().is_none_or(|ext| ext != "app") || !app.is_dir() {
            return Err(format!("{} is not a built .app bundle", app.display()));
        }
        run(
            XCRUN,
            &["simctl", "install", &device.udid, &app.to_string_lossy()],
            Duration::from_secs(300),
        )?
        .ok("simctl install")?;
        installed = true;
        if id.is_none() {
            id = Some(bundle_id(app)?);
        }
    }
    let id = id.ok_or("Pass app_path (a built .app) or bundle_id")?;
    let stdout = run(
        XCRUN,
        &[
            "simctl",
            "launch",
            "--terminate-running-process",
            &device.udid,
            &id,
        ],
        Duration::from_secs(90),
    )?
    .ok("simctl launch")?;
    Ok(Launched {
        pid: launched_pid(&stdout),
        device,
        bundle_id: id,
        installed,
    })
}

/// A screenshot as the agent receives it, and the size it was taken at.
pub struct Screenshot {
    pub jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
    /// The screen in points, the unit `tap` and `swipe` take.
    pub point_width: u32,
    pub point_height: u32,
    pub device: Device,
    pub saved: Option<String>,
}

/// Pixels per point on the device's main screen (3 on most iPhones).
fn screen_scale(udid: &str) -> Option<f64> {
    run(
        XCRUN,
        &["simctl", "getenv", udid, "SIMULATOR_MAINSCREEN_SCALE"],
        Duration::from_secs(10),
    )
    .ok()?
    .ok("simctl getenv")
    .ok()?
    .trim()
    .parse::<f64>()
    .ok()
    .filter(|scale| *scale >= 1.0 && *scale <= 4.0)
}

/// The longest edge of the image the agent gets: the screen in points, so
/// a spot in the image is the point to tap, unless that is too large.
pub(crate) fn agent_edge(pixel_width: u32, pixel_height: u32, scale: f64) -> (u32, u32, u32) {
    let points = |pixels: u32| (f64::from(pixels) / scale).round() as u32;
    let (width, height) = (points(pixel_width), points(pixel_height));
    (width, height, width.max(height).min(SCREENSHOT_EDGE))
}

/// Fits `image` inside `edge` × `edge`, keeping its proportions, as JPEG.
pub(crate) fn agent_jpeg(
    image: &image::DynamicImage,
    edge: u32,
) -> Result<(Vec<u8>, u32, u32), String> {
    let image = if image.width().max(image.height()) > edge {
        image.resize(edge, edge, image::imageops::FilterType::Triangle)
    } else {
        image.clone()
    };
    let rgb = image.to_rgb8();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 80)
        .encode_image(&rgb)
        .map_err(|e| e.to_string())?;
    Ok((jpeg, rgb.width(), rgb.height()))
}

/// The booted device's screen. `save_to` keeps the full PNG there too.
pub fn screenshot(wanted: Option<&str>, save_to: Option<&str>) -> Result<Screenshot, String> {
    let device = booted(wanted)?;
    let file = tempfile::Builder::new()
        .prefix("monobot-simulator-")
        .suffix(".png")
        .tempfile()
        .map_err(|e| e.to_string())?;
    let path = file.path().to_string_lossy().into_owned();
    run(
        XCRUN,
        &[
            "simctl",
            "io",
            &device.udid,
            "screenshot",
            "--type=png",
            &path,
        ],
        Duration::from_secs(30),
    )?
    .ok("simctl screenshot")?;
    let png = std::fs::read(file.path()).map_err(|e| e.to_string())?;
    let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
        .map_err(|e| format!("The screenshot could not be read: {e}"))?;
    let saved = match save_to {
        Some(target) => {
            let target = absolute(target, "save_to")?;
            if target.extension().is_none_or(|ext| ext != "png") {
                return Err("save_to must end in .png".into());
            }
            std::fs::write(target, &png).map_err(|e| format!("{}: {e}", target.display()))?;
            Some(target.to_string_lossy().into_owned())
        }
        None => None,
    };
    let scale = screen_scale(&device.udid).unwrap_or(1.0);
    let (point_width, point_height, edge) = agent_edge(image.width(), image.height(), scale);
    let (jpeg, width, height) = agent_jpeg(&image, edge)?;
    Ok(Screenshot {
        jpeg,
        width,
        height,
        pixel_width: image.width(),
        pixel_height: image.height(),
        point_width,
        point_height,
        device,
        saved,
    })
}

/// A process name that fits inside a log predicate as it is.
pub(crate) fn valid_process(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-'))
}

/// The device log of the last `seconds`, newest end, for one process when named.
pub fn logs(
    wanted: Option<&str>,
    process: Option<&str>,
    seconds: u64,
    debug: bool,
) -> Result<String, String> {
    let device = booted(wanted)?;
    let last = format!("{}s", seconds.clamp(1, 600));
    let mut args = vec![
        "simctl",
        "spawn",
        &device.udid,
        "log",
        "show",
        "--last",
        &last,
        "--style",
        "compact",
        "--info",
    ];
    if debug {
        args.push("--debug");
    }
    let predicate;
    if let Some(process) = process {
        if !valid_process(process) {
            return Err(
                "process must be an executable name (letters, digits, spaces, '.', '_' or '-')"
                    .into(),
            );
        }
        predicate = format!("process == \"{process}\"");
        args.extend(["--predicate", &predicate]);
    }
    let text = run(XCRUN, &args, Duration::from_secs(90))?.ok("log show")?;
    Ok(tail(&text, LOG_LIMIT).to_owned())
}

/// Opens a URL (a deep link or a web page) on the booted device.
pub fn open_url(wanted: Option<&str>, url: &str) -> Result<Device, String> {
    let url = url.trim();
    if url.len() > 2048 || !url.contains(':') || url.chars().any(char::is_control) {
        return Err(
            "url must be a full URL with its scheme, such as myapp://path or https://example.com"
                .into(),
        );
    }
    let device = booted(wanted)?;
    run(
        XCRUN,
        &["simctl", "openurl", &device.udid, url],
        Duration::from_secs(30),
    )?
    .ok("simctl openurl")?;
    Ok(device)
}

/// Switches the booted device to light or dark mode.
pub fn appearance(wanted: Option<&str>, mode: &str) -> Result<Device, String> {
    if !matches!(mode, "light" | "dark") {
        return Err("mode must be 'light' or 'dark'".into());
    }
    let device = booted(wanted)?;
    run(
        XCRUN,
        &["simctl", "ui", &device.udid, "appearance", mode],
        Duration::from_secs(30),
    )?
    .ok("simctl ui appearance")?;
    Ok(device)
}

/// Runs one runner action on a booted device: a touch, typing, a button,
/// a rotation or reading the interface.
pub fn act(
    wanted: Option<&str>,
    action: &str,
    body: impl FnOnce(&Device) -> Value,
) -> Result<(Device, Value), String> {
    let device = booted(wanted)?;
    let body = body(&device);
    let result = input::act(&device.udid, action, &body, Duration::from_secs(60))?;
    Ok((device, result))
}

pub use build::{build_status, find_projects, schemes, start_build, BuildStatus};
