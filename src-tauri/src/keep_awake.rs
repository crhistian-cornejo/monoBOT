//! Keep the computer from going to sleep while MonoBOT has work under way.
//!
//! Only idle sleep is held off: the display can still turn off, and closing
//! the lid or choosing Sleep still sleeps. The hold ends with the process.

use std::sync::Mutex;

static HOLD: Mutex<Option<platform::Hold>> = Mutex::new(None);

/// Hold or release. Calling it again with the same value does nothing.
#[tauri::command]
pub fn keep_awake_set(enabled: bool) -> Result<(), String> {
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

#[cfg(windows)]
mod platform {
    use std::sync::mpsc;
    use std::thread;
    use windows_sys::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
    };

    /// Windows ties the request to the thread that made it, and command
    /// threads come and go, so one thread of our own makes it and holds it.
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
    extern "C" {
        fn CFStringCreateWithCString(
            allocator: *const c_void,
            text: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
        fn CFRelease(value: *const c_void);
    }

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
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

    // Holds and releases the real request, and repeating either is harmless.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn holds_and_releases_idempotently() {
        keep_awake_set(true).unwrap();
        keep_awake_set(true).unwrap();
        assert!(HOLD.lock().unwrap().is_some());
        keep_awake_set(false).unwrap();
        keep_awake_set(false).unwrap();
        assert!(HOLD.lock().unwrap().is_none());
    }
}
