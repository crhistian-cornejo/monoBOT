//! What the bezel's buttons do, besides touches: rotate, switch light and
//! dark, save a screenshot or a screen recording next to Simulator.app's
//! (on the Desktop, with its names) and connect or disconnect the keyboard.
//! Everything here blocks; the pane runs it off the interface thread.
use super::{run, XCRUN};
use chrono::{DateTime, Local};
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

/// Which way the device is held. The framebuffer keeps the portrait layout
/// whatever the orientation, and iOS draws the interface turned inside it,
/// so a view turns the picture with the device.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    #[default]
    Portrait,
    /// Turned a quarter clockwise: the Home indicator on the left.
    LandscapeRight,
    PortraitUpsideDown,
    /// Turned a quarter counter-clockwise: the Home indicator on the right.
    LandscapeLeft,
}

impl Orientation {
    const CLOCKWISE: [Self; 4] = [
        Self::Portrait,
        Self::LandscapeRight,
        Self::PortraitUpsideDown,
        Self::LandscapeLeft,
    ];

    /// Clockwise quarter turns from portrait.
    pub fn quarter_turns(self) -> u8 {
        Self::CLOCKWISE.iter().position(|o| *o == self).unwrap_or(0) as u8
    }

    fn from_turns(turns: u8) -> Self {
        Self::CLOCKWISE[usize::from(turns % 4)]
    }

    /// Simulator.app's Rotate Right (⌘→).
    pub fn rotated_right(self) -> Self {
        Self::from_turns(self.quarter_turns() + 1)
    }

    /// Simulator.app's Rotate Left (⌘←).
    pub fn rotated_left(self) -> Self {
        Self::from_turns(self.quarter_turns() + 3)
    }

    pub fn landscape(self) -> bool {
        self.quarter_turns() % 2 == 1
    }

    /// The name the runner's `rotate` action takes.
    pub fn runner_name(self) -> &'static str {
        match self {
            Self::Portrait => "portrait",
            Self::LandscapeRight => "landscape_right",
            Self::PortraitUpsideDown => "portrait_upside_down",
            Self::LandscapeLeft => "landscape_left",
        }
    }

    pub fn from_runner_name(name: &str) -> Option<Self> {
        Self::CLOCKWISE
            .into_iter()
            .find(|o| o.runner_name() == name)
    }
}

/// Runs one runner action (a touch, typing, a key, a button) on the booted
/// device `udid` that the caller follows, without listing the devices
/// first. The live pane's input goes this way; agents name devices loosely
/// and go through `simulator::act`.
pub fn act(
    udid: &str,
    action: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    super::input::act_known(udid, action, body, Duration::from_secs(60))
}

/// Turns the device; the runner sets the orientation through XCUIDevice.
pub fn rotate(udid: &str, to: Orientation) -> Result<(), String> {
    act(
        udid,
        "rotate",
        &serde_json::json!({ "orientation": to.runner_name() }),
    )
    .map(|_| ())
}

/// The appearance after a switch from `current` ("light" or "dark").
pub(crate) fn other_appearance(current: &str) -> &'static str {
    if current.trim().eq_ignore_ascii_case("dark") {
        "light"
    } else {
        "dark"
    }
}

/// Switches between light and dark; answers the new appearance.
pub fn toggle_appearance(udid: &str) -> Result<&'static str, String> {
    let current = run(
        XCRUN,
        &["simctl", "ui", udid, "appearance"],
        Duration::from_secs(20),
    )?
    .ok("simctl ui appearance")?;
    let next = other_appearance(&current);
    run(
        XCRUN,
        &["simctl", "ui", udid, "appearance", next],
        Duration::from_secs(20),
    )?
    .ok("simctl ui appearance")?;
    Ok(next)
}

/// Simulator.app's file name for a capture: "Simulator Screenshot -
/// iPhone 18 Pro - 2026-11-18 at 19.02.11.png".
pub(crate) fn capture_name(
    kind: &str,
    device: &str,
    at: DateTime<Local>,
    extension: &str,
) -> String {
    let device: String = device
        .chars()
        .map(|c| if matches!(c, '/' | ':') { '-' } else { c })
        .collect();
    format!(
        "Simulator {kind} - {device} - {}.{extension}",
        at.format("%Y-%m-%d at %H.%M.%S")
    )
}

/// Where captures go: the Desktop, as Simulator.app does.
fn captures() -> PathBuf {
    dirs::desktop_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir)
}

/// Saves the full screen as a PNG on the Desktop.
pub fn save_screenshot(udid: &str, device: &str) -> Result<PathBuf, String> {
    let path = captures().join(capture_name("Screenshot", device, Local::now(), "png"));
    let target = path.to_string_lossy().into_owned();
    run(
        XCRUN,
        &["simctl", "io", udid, "screenshot", "--type=png", &target],
        Duration::from_secs(30),
    )?
    .ok("simctl io screenshot")?;
    Ok(path)
}

/// A screen recording in progress; stopping it writes the movie.
pub struct Recording {
    child: Child,
    pub path: PathBuf,
    started: Instant,
}

impl Recording {
    /// Starts `simctl io recordVideo` into a movie on the Desktop.
    pub fn start(udid: &str, device: &str) -> Result<Self, String> {
        let path = captures().join(capture_name(
            "Screen Recording",
            device,
            Local::now(),
            "mov",
        ));
        let child = Command::new(XCRUN)
            .args([
                "simctl",
                "io",
                udid,
                "recordVideo",
                "--codec=h264",
                "--force",
            ])
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("simctl io recordVideo: {e}"))?;
        Ok(Self {
            child,
            path,
            started: Instant::now(),
        })
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Stops the recording the way ⌃C would, so simctl finishes the file.
    pub fn stop(mut self) -> Result<PathBuf, String> {
        #[cfg(unix)]
        unsafe {
            libc::kill(self.child.id() as i32, libc::SIGINT);
        }
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50))
                }
                _ => {
                    let _ = self.child.kill();
                    return Err("The recording did not finish in time".into());
                }
            }
        }
        if self.path.exists() {
            Ok(self.path.clone())
        } else {
            Err("simctl wrote no movie".into())
        }
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            #[cfg(unix)]
            unsafe {
                libc::kill(self.child.id() as i32, libc::SIGINT);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn rotating_right_goes_clockwise_and_left_undoes_it() {
        let mut o = Orientation::Portrait;
        let mut seen = Vec::new();
        for _ in 0..4 {
            o = o.rotated_right();
            seen.push(o);
        }
        assert_eq!(
            seen,
            [
                Orientation::LandscapeRight,
                Orientation::PortraitUpsideDown,
                Orientation::LandscapeLeft,
                Orientation::Portrait
            ]
        );
        for o in Orientation::CLOCKWISE {
            assert_eq!(o.rotated_right().rotated_left(), o);
            assert_eq!(Orientation::from_runner_name(o.runner_name()), Some(o));
        }
        assert!(Orientation::LandscapeLeft.landscape());
        assert!(!Orientation::PortraitUpsideDown.landscape());
        assert_eq!(Orientation::LandscapeLeft.quarter_turns(), 3);
    }

    #[test]
    fn appearance_switches_to_the_other_one() {
        assert_eq!(other_appearance("light\n"), "dark");
        assert_eq!(other_appearance("Dark"), "light");
        assert_eq!(other_appearance("unsupported"), "dark");
    }

    #[test]
    fn captures_are_named_like_simulator_app_names_them() {
        let at = Local.with_ymd_and_hms(2026, 11, 18, 19, 2, 11).unwrap();
        assert_eq!(
            capture_name("Screenshot", "iPhone 18 Pro", at, "png"),
            "Simulator Screenshot - iPhone 18 Pro - 2026-11-18 at 19.02.11.png"
        );
        assert_eq!(
            capture_name("Screen Recording", "a/b:c", at, "mov"),
            "Simulator Screen Recording - a-b-c - 2026-11-18 at 19.02.11.mov"
        );
    }
}
