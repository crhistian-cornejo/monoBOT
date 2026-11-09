//! The system's own levels for the notch, as IVY shows them: the output
//! volume (and mute) and the built-in display's brightness. A thread of its
//! own reads them five times a second and says only when one changes; both
//! reads are a property query, with no audio or display work.
use std::time::Duration;

/// A level the user just changed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Level {
    /// 0 to 1, and whether the output is muted.
    Volume(f32, bool),
    /// 0 to 1.
    Brightness(f32),
}

/// What the system reports now; `None` where it cannot say (no output
/// device with a volume, an external display).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Levels {
    pub volume: Option<(f32, bool)>,
    pub brightness: Option<f32>,
}

/// How often the levels are read.
const EVERY: Duration = Duration::from_millis(200);

/// The changes between two readings; a change smaller than half a percent
/// is noise, not the user.
pub fn changes(before: &Levels, now: &Levels) -> Vec<Level> {
    let moved = |a: f32, b: f32| (a - b).abs() >= 0.005;
    let mut changes = Vec::new();
    if let (Some((was, was_muted)), Some((volume, muted))) = (before.volume, now.volume) {
        if moved(was, volume) || was_muted != muted {
            changes.push(Level::Volume(volume, muted));
        }
    }
    if let (Some(was), Some(brightness)) = (before.brightness, now.brightness) {
        if moved(was, brightness) {
            changes.push(Level::Brightness(brightness));
        }
    }
    changes
}

/// Reads the levels on a thread of its own and hands each change to
/// `on_change`. The first reading is the baseline, not a change.
pub fn watch(on_change: impl Fn(Level) + Send + 'static) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("notch-levels".into())
        .spawn(move || {
            let mut before = read();
            loop {
                std::thread::sleep(EVERY);
                let now = read();
                for change in changes(&before, &now) {
                    on_change(change);
                }
                before = now;
            }
        })
        .map(|_| ())
}

pub fn read() -> Levels {
    #[cfg(target_os = "macos")]
    return mac::read();
    #[cfg(not(target_os = "macos"))]
    Levels::default()
}

#[cfg(target_os = "macos")]
mod mac {
    use super::Levels;
    use std::ffi::c_void;
    use std::sync::OnceLock;

    #[repr(C)]
    struct Address {
        selector: u32,
        scope: u32,
        element: u32,
    }

    const fn code(tag: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*tag)
    }
    const SYSTEM_OBJECT: u32 = 1;
    const DEFAULT_OUTPUT: u32 = code(b"dOut");
    const GLOBAL: u32 = code(b"glob");
    const OUTPUT: u32 = code(b"outp");
    const MAIN_VOLUME: u32 = code(b"vmvc");
    const MUTE: u32 = code(b"mute");

    #[link(name = "CoreAudio", kind = "framework")]
    unsafe extern "C" {
        fn AudioObjectGetPropertyData(
            object: u32,
            address: *const Address,
            qualifier_size: u32,
            qualifier: *const c_void,
            size: *mut u32,
            data: *mut c_void,
        ) -> i32;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGMainDisplayID() -> u32;
    }

    fn property<T: Default>(object: u32, selector: u32, scope: u32) -> Option<T> {
        let address = Address {
            selector,
            scope,
            element: 0,
        };
        let mut value = T::default();
        let mut size = std::mem::size_of::<T>() as u32;
        // SAFETY: `value` is a plain number of `size` bytes.
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                (&mut value as *mut T).cast(),
            )
        };
        (status == 0).then_some(value)
    }

    fn volume() -> Option<(f32, bool)> {
        let device: u32 = property(SYSTEM_OBJECT, DEFAULT_OUTPUT, GLOBAL)?;
        let volume: f32 = property(device, MAIN_VOLUME, OUTPUT)?;
        let muted: u32 = property(device, MUTE, OUTPUT).unwrap_or(0);
        Some((volume.clamp(0., 1.), muted != 0))
    }

    type GetBrightness = unsafe extern "C" fn(display: u32, brightness: *mut f32) -> i32;

    /// DisplayServices' brightness reader, loaded once. It is a private
    /// framework: where it is missing the notch shows no brightness.
    fn get_brightness() -> Option<GetBrightness> {
        static FUNCTION: OnceLock<Option<usize>> = OnceLock::new();
        let loaded = FUNCTION.get_or_init(|| {
            let path =
                c"/System/Library/PrivateFrameworks/DisplayServices.framework/DisplayServices";
            // SAFETY: dlopen and dlsym with constant, NUL-terminated names.
            unsafe {
                let library = libc::dlopen(path.as_ptr(), libc::RTLD_LAZY);
                if library.is_null() {
                    return None;
                }
                let symbol = libc::dlsym(library, c"DisplayServicesGetBrightness".as_ptr());
                (!symbol.is_null()).then_some(symbol as usize)
            }
        });
        let address = (*loaded)?;
        // SAFETY: the symbol's C signature is `int (CGDirectDisplayID, float *)`.
        Some(unsafe { std::mem::transmute::<usize, GetBrightness>(address) })
    }

    fn brightness() -> Option<f32> {
        let get = get_brightness()?;
        let mut value = 0f32;
        // SAFETY: a display id and a pointer to one float.
        let status = unsafe { get(CGMainDisplayID(), &mut value) };
        (status == 0).then_some(value.clamp(0., 1.))
    }

    pub(super) fn read() -> Levels {
        Levels {
            volume: volume(),
            brightness: brightness(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_real_changes_are_told() {
        let before = Levels {
            volume: Some((0.5, false)),
            brightness: Some(0.8),
        };
        assert!(changes(&before, &before).is_empty());
        let jitter = Levels {
            volume: Some((0.502, false)),
            brightness: Some(0.801),
        };
        assert!(changes(&before, &jitter).is_empty(), "noise");
        let louder = Levels {
            volume: Some((0.6, false)),
            ..before
        };
        assert_eq!(changes(&before, &louder), [Level::Volume(0.6, false)]);
        let muted = Levels {
            volume: Some((0.5, true)),
            ..before
        };
        assert_eq!(changes(&before, &muted), [Level::Volume(0.5, true)]);
        let dimmer = Levels {
            brightness: Some(0.4),
            ..before
        };
        assert_eq!(changes(&before, &dimmer), [Level::Brightness(0.4)]);
        // Unknown before or now says nothing.
        assert!(changes(&Levels::default(), &louder).is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn this_mac_reports_its_levels_in_range() {
        let levels = read();
        eprintln!("levels on this Mac: {levels:?}");
        if let Some((volume, _)) = levels.volume {
            assert!((0. ..=1.).contains(&volume));
        }
        if let Some(brightness) = levels.brightness {
            assert!((0. ..=1.).contains(&brightness));
        }
    }
}
