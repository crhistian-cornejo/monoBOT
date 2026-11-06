//! Keep the computer from going to sleep while MonoBOT has work under way.
//!
//! Port of `src-tauri/src/keep_awake.rs` (the platform holds, as they are),
//! `src/features/sessions/model/keepAwake.ts` (when to hold) and the
//! "Keep computer awake" preference of `settings.ts`.
//!
//! Only idle sleep is held off: the display can still turn off, and closing the
//! lid or choosing Sleep still sleeps. The hold ends with the process.
//!
//! The UI calls [`sync`] whenever a session's state changes (or on a timer):
//!
//! ```ignore
//! keep_awake::sync(keep_awake::load_preference(&store, Some(&prefs)), &work)?;
//! ```
//!
//! where `work` has one [`Work`] per open session.
//!
//! Persistence: the preference lives in the store setting [`SETTING_KEY`]
//! (`monocode.keepAwake`, "1" or "0"), the same key and encoding the Tauri app
//! keeps in `localStorage`; [`load_preference`] also reads Tauri's value while
//! the native app has none of its own. Default: on, where it is available.

use crate::desktop_preferences::DesktopPreferences;
use crate::store::Store;
use std::sync::Mutex;

/// Store setting (and Tauri `localStorage` key) of "Keep computer awake".
pub const SETTING_KEY: &str = "monocode.keepAwake";

/// The preference when nobody chose.
pub const DEFAULT: bool = true;

/// Windows and macOS can hold; Linux has no hold yet.
pub const AVAILABLE: bool = cfg!(any(windows, target_os = "macos"));

static HOLD: Mutex<Option<platform::Hold>> = Mutex::new(None);

/// Hold or release. Calling it again with the same value does nothing.
pub fn set(enabled: bool) -> Result<(), String> {
    let mut hold = HOLD.lock().map_err(|error| error.to_string())?;
    if enabled == hold.is_some() {
        return Ok(());
    }
    *hold = if enabled {
        Some(platform::Hold::acquire()?)
    } else {
        None
    };
    Ok(())
}

/// Whether the hold is on right now.
pub fn is_held() -> bool {
    HOLD.lock().map(|hold| hold.is_some()).unwrap_or(false)
}

/// What a session contributes to "work under way".
#[derive(Clone, Debug, Default)]
pub struct Work {
    /// The project path; a `remote://` session runs on its host and doesn't count.
    pub cwd: String,
    /// An agent is working.
    pub busy: bool,
    /// A provider retry is scheduled ([`crate::provider_retry::ProviderRetry::retry_at`] is set).
    pub retry_scheduled: bool,
    /// A usage-limit resume is armed ([`crate::usage::Limit::resume_at_reset`]).
    pub resume_at_reset: bool,
    /// A message waits for its time ([`crate::scheduled_send`]).
    pub scheduled_message: bool,
}

impl Work {
    fn is_remote(&self) -> bool {
        self.cwd.replace('\\', "/").starts_with("remote://")
    }
}

/// Work that stalls if the computer sleeps: an agent working, a provider retry
/// or a usage-limit resume waiting, or a message scheduled with Send later.
/// A remote session's work runs on its host, so it doesn't count.
pub fn has_work_underway(sessions: &[Work]) -> bool {
    sessions.iter().any(|session| {
        !session.is_remote()
            && (session.busy
                || session.retry_scheduled
                || session.resume_at_reset
                || session.scheduled_message)
    })
}

/// Hold exactly when the preference is on and there is work. Returns whether the
/// hold is on afterwards. A system that can't hold is not an error for the
/// caller to show: it returns `Ok(false)`.
pub fn sync(preference: bool, sessions: &[Work]) -> Result<bool, String> {
    if !AVAILABLE {
        return Ok(false);
    }
    set(preference && has_work_underway(sessions))?;
    Ok(is_held())
}

/// The saved preference: the native one, else the one the Tauri app saved, else
/// [`DEFAULT`]. Always false where [`AVAILABLE`] is false.
pub fn load_preference(store: &Store, tauri: Option<&DesktopPreferences>) -> bool {
    if !AVAILABLE {
        return false;
    }
    let raw = store.setting(SETTING_KEY).ok().flatten().or_else(|| {
        tauri
            .and_then(|prefs| prefs.get(SETTING_KEY))
            .map(String::from)
    });
    raw.map_or(DEFAULT, |raw| raw == "1" || raw == "true")
}

/// Save the preference ("1" or "0", as Tauri does).
pub fn save_preference(store: &Store, enabled: bool) -> Result<(), String> {
    store.save_setting(SETTING_KEY, if enabled { "1" } else { "0" })
}

#[cfg(windows)]
mod platform {
    use std::sync::mpsc;
    use std::thread;
    use windows_sys::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
    };

    /// Windows ties the request to the thread that made it, and callers' threads
    /// come and go, so one thread of our own makes it and holds it.
    pub struct Hold {
        stop: Option<mpsc::Sender<()>>,
        thread: Option<thread::JoinHandle<()>>,
    }

    impl Hold {
        pub fn acquire() -> Result<Self, String> {
            let (stop, stopped) = mpsc::channel::<()>();
            let (ready, held) = mpsc::channel::<bool>();
            let thread = thread::Builder::new()
                .name("keep-awake".into())
                .spawn(move || {
                    // SAFETY: a plain Win32 call that takes flags.
                    let ok =
                        unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) } != 0;
                    let _ = ready.send(ok);
                    if !ok {
                        return;
                    }
                    // Returns once the Hold drops its sender.
                    let _ = stopped.recv();
                    // SAFETY: as above; clears this thread's request.
                    unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
                })
                .map_err(|error| error.to_string())?;
            if held.recv().unwrap_or(false) {
                Ok(Self {
                    stop: Some(stop),
                    thread: Some(thread),
                })
            } else {
                let _ = thread.join();
                Err("Windows didn't let MonoBOT keep the computer awake".into())
            }
        }
    }

    impl Drop for Hold {
        fn drop(&mut self) {
            self.stop.take();
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::{c_char, c_void, CString};

    type CFStringRef = *const c_void;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(
            allocator: *const c_void,
            text: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
        fn CFRelease(value: *const c_void);
    }

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPMAssertionCreateWithName(
            kind: CFStringRef,
            level: u32,
            name: CFStringRef,
            id: *mut u32,
        ) -> i32;
        fn IOPMAssertionRelease(id: u32) -> i32;
    }

    const UTF8: u32 = 0x0800_0100;
    const ASSERTION_LEVEL_ON: u32 = 255;

    fn cf_string(text: &str) -> CFStringRef {
        let text = CString::new(text).expect("no NUL in a constant");
        // SAFETY: a valid C string; the caller releases the result.
        unsafe { CFStringCreateWithCString(std::ptr::null(), text.as_ptr(), UTF8) }
    }

    /// An IOKit power assertion, the same mechanism `caffeinate -i` uses.
    pub struct Hold(u32);

    impl Hold {
        pub fn acquire() -> Result<Self, String> {
            let kind = cf_string("PreventUserIdleSystemSleep");
            let name = cf_string("MonoBOT has agents working");
            if kind.is_null() || name.is_null() {
                for value in [kind, name] {
                    if !value.is_null() {
                        // SAFETY: created above and not yet released.
                        unsafe { CFRelease(value) };
                    }
                }
                return Err("Could not ask macOS to stay awake".into());
            }
            let mut id = 0;
            // SAFETY: both strings are valid; `id` outlives the call.
            let status =
                unsafe { IOPMAssertionCreateWithName(kind, ASSERTION_LEVEL_ON, name, &mut id) };
            // SAFETY: created above; IOKit keeps its own references.
            unsafe {
                CFRelease(kind);
                CFRelease(name);
            }
            if status == 0 {
                Ok(Self(id))
            } else {
                Err(format!("macOS didn't let MonoBOT stay awake ({status})"))
            }
        }
    }

    impl Drop for Hold {
        fn drop(&mut self) {
            // SAFETY: the id came from IOPMAssertionCreateWithName.
            unsafe { IOPMAssertionRelease(self.0) };
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    pub struct Hold;

    impl Hold {
        pub fn acquire() -> Result<Self, String> {
            Err("Keeping the computer awake isn't available on this system yet".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work(patch: impl FnOnce(&mut Work)) -> Work {
        let mut work = Work {
            cwd: "/tmp/project".into(),
            ..Default::default()
        };
        patch(&mut work);
        work
    }

    #[test]
    fn is_idle_with_nothing_running_or_waiting() {
        assert!(!has_work_underway(&[]));
        assert!(!has_work_underway(&[work(|_| {})]));
    }

    #[test]
    fn counts_an_agent_at_work() {
        assert!(has_work_underway(&[work(|_| {}), work(|w| w.busy = true)]));
    }

    #[test]
    fn counts_work_waiting_for_its_time() {
        assert!(has_work_underway(&[work(|w| w.retry_scheduled = true)]));
        assert!(has_work_underway(&[work(|w| w.resume_at_reset = true)]));
        assert!(has_work_underway(&[work(|w| w.scheduled_message = true)]));
    }

    #[test]
    fn leaves_remote_sessions_to_their_host() {
        assert!(!has_work_underway(&[work(|w| {
            w.cwd = "remote://box/repo".into();
            w.busy = true;
        })]));
    }

    #[test]
    fn the_preference_defaults_on_and_round_trips() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        assert_eq!(load_preference(&store, None), AVAILABLE && DEFAULT);
        save_preference(&store, false).unwrap();
        assert!(!load_preference(&store, None));
        assert_eq!(store.setting(SETTING_KEY).unwrap().as_deref(), Some("0"));
        save_preference(&store, true).unwrap();
        assert_eq!(load_preference(&store, None), AVAILABLE);
    }

    // Holds and releases the real request, and repeating either is harmless.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn holds_and_releases_idempotently() {
        set(true).unwrap();
        set(true).unwrap();
        assert!(is_held());
        set(false).unwrap();
        set(false).unwrap();
        assert!(!is_held());
        assert!(sync(true, &[work(|w| w.busy = true)]).unwrap());
        assert!(!sync(true, &[]).unwrap());
        assert!(!sync(false, &[work(|w| w.busy = true)]).unwrap());
    }
}
