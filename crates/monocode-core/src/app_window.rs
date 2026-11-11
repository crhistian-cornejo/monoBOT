//! The workspace window's life outside the window itself: what closing it
//! does, and (Windows) one running copy of the app per data folder, so a
//! second launch brings back the window the first one hid.
//!
//! The GPUI side (`monocode-gpui/src/app_window.rs`) hides and shows the
//! window; this decides when, and finds the copy that is already running.

use std::path::Path;

/// What closing the workspace window does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnClose {
    /// Hide it: the app, the notch, the tray and the agents keep going.
    Hide,
    /// Ask first: a file has unsaved edits (quit without saving, or cancel).
    AskAboutUnsaved,
    /// Close it, which quits the app once only the notch is left.
    Close,
}

/// `keep_running` is Settings › General's "Keep monoBOT running in the
/// background when the window closes"; `can_hide` is whether this system has
/// a way back to a hidden window (the notch, the tray, the Dock: macOS and
/// Windows). Unsaved edits always ask, as quitting does.
pub fn on_close(keep_running: bool, unsaved: bool, can_hide: bool) -> OnClose {
    if unsaved {
        OnClose::AskAboutUnsaved
    } else if keep_running && can_hide {
        OnClose::Hide
    } else {
        OnClose::Close
    }
}

/// The name both copies of the app agree on for this data folder: one
/// running copy per database, so the development data and the installed
/// app's never meet. Windows paths ignore case, and so does the name.
pub fn instance_name(database: &Path) -> String {
    let path = database.to_string_lossy();
    let path = path.trim_start_matches(r"\\?\").replace('/', "\\");
    // FNV-1a: stable across runs and builds, unlike the std hasher.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.to_lowercase().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!(r"Local\monoBOT-show-{hash:016x}")
}

#[cfg(windows)]
pub use windows_instance::{claim, Claim, Instance};

#[cfg(windows)]
mod windows_instance {
    use std::path::Path;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Threading::{
        CreateEventW, SetEvent, WaitForSingleObject, INFINITE,
    };

    /// What a launch found.
    pub enum Claim {
        /// No other copy runs: this one is it, and hears the next launch.
        First(Instance),
        /// Another copy runs and was asked to show its window: exit.
        Running,
        /// The system refused the name; run as before, without the check.
        Unavailable,
    }

    /// The running copy's side: a named event a later launch sets.
    pub struct Instance(isize);

    // SAFETY: an event handle may be waited on from any thread.
    unsafe impl Send for Instance {}
    unsafe impl Sync for Instance {}

    /// Claims the name for `database`, or wakes the copy that holds it.
    pub fn claim(database: &Path) -> Claim {
        let name: Vec<u16> = super::instance_name(database)
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // SAFETY: a NUL-terminated name and no security attributes; an
        // auto-reset event, so each launch wakes the running copy once.
        let handle = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
        if handle.is_null() {
            return Claim::Unavailable;
        }
        // SAFETY: read straight after the call that set it.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            // SAFETY: the handle just opened, set once and closed.
            unsafe {
                SetEvent(handle);
                CloseHandle(handle);
            }
            return Claim::Running;
        }
        Claim::First(Instance(handle as isize))
    }

    impl Instance {
        /// Blocks until a later launch asks for the window; false if the
        /// wait failed (then stop listening).
        pub fn wait(&self) -> bool {
            // SAFETY: the handle lives as long as `self`.
            unsafe { WaitForSingleObject(self.0 as HANDLE, INFINITE) == WAIT_OBJECT_0 }
        }
    }

    impl Drop for Instance {
        fn drop(&mut self) {
            // SAFETY: closed once, here.
            unsafe { CloseHandle(self.0 as HANDLE) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_hides_only_when_asked_and_a_way_back_exists() {
        assert_eq!(on_close(true, false, true), OnClose::Hide);
        assert_eq!(on_close(false, false, true), OnClose::Close);
        // Linux: no notch or tray to come back from.
        assert_eq!(on_close(true, false, false), OnClose::Close);
        // Unsaved edits ask, whatever the setting.
        assert_eq!(on_close(true, true, true), OnClose::AskAboutUnsaved);
        assert_eq!(on_close(false, true, true), OnClose::AskAboutUnsaved);
        assert_eq!(on_close(false, true, false), OnClose::AskAboutUnsaved);
    }

    #[test]
    fn the_instance_name_is_one_per_database_and_ignores_case_and_slashes() {
        let installed = instance_name(Path::new(
            r"C:\Users\Ana\AppData\Roaming\monoBOT\monocode.db",
        ));
        assert!(installed.starts_with(r"Local\monoBOT-show-"));
        assert_eq!(installed.len(), r"Local\monoBOT-show-".len() + 16);
        assert_eq!(
            installed,
            instance_name(Path::new(
                r"c:/users/ana/appdata/roaming/MONOBOT/monocode.db"
            ))
        );
        assert_eq!(
            installed,
            instance_name(Path::new(
                r"\\?\C:\Users\Ana\AppData\Roaming\monoBOT\monocode.db"
            ))
        );
        let development = instance_name(Path::new(
            r"C:\Users\Ana\AppData\Roaming\MonoBOT Development\monocode.db",
        ));
        assert_ne!(installed, development);
        // Stable across runs: a later launch must compute the same name.
        assert_eq!(
            installed,
            instance_name(Path::new(
                r"C:\Users\Ana\AppData\Roaming\monoBOT\monocode.db"
            ))
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_second_launch_wakes_the_first_and_steps_aside() {
        let database = std::env::temp_dir()
            .join(format!("monobot-instance-{}", std::process::id()))
            .join("monocode.db");
        let Claim::First(first) = claim(&database) else {
            panic!("the first launch holds the name");
        };
        assert!(matches!(claim(&database), Claim::Running));
        // The second launch set the event: the first hears it once.
        assert!(first.wait());
        let other = database.with_file_name("other.db");
        assert!(matches!(claim(&other), Claim::First(_)));
        drop(first);
        // Once the first copy is gone, the next launch is first again.
        assert!(matches!(claim(&database), Claim::First(_)));
    }
}
