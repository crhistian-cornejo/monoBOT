//! The system's own levels for the notch, as IVY shows them: the output
//! volume (and mute), the built-in display's brightness and the battery. A
//! thread of its own reads them five times a second and says only when one
//! changes; each read is a property query, with no audio or display work.
use std::time::Duration;

/// A level the user just changed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Level {
    /// 0 to 1, and whether the output is muted.
    Volume(f32, bool),
    /// 0 to 1.
    Brightness(f32),
    /// The battery's charge, 0 to 1, and whether the charger is in: told
    /// when the charger goes in or out, when the charge fills, and when it
    /// runs low.
    Power(f32, bool),
}

/// What the system reports now; `None` where it cannot say (no output
/// device with a volume, an external display).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Levels {
    pub volume: Option<(f32, bool)>,
    pub brightness: Option<f32>,
    /// The battery's charge and whether the charger is in; `None` on a
    /// machine without a battery.
    pub power: Option<(f32, bool)>,
}

/// The charges at which a battery running down is told: a fifth, a tenth.
const LOW: [f32; 2] = [0.2, 0.1];
/// The charge that counts as full.
const FULL: f32 = 0.995;

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
    if let (Some((was, was_plugged)), Some((charge, plugged))) = (before.power, now.power) {
        let filled = plugged && was < FULL && charge >= FULL;
        let ran_low = !plugged && LOW.iter().any(|low| was > *low && charge <= *low);
        if was_plugged != plugged || filled || ran_low {
            changes.push(Level::Power(charge, plugged));
        }
    }
    changes
}

/// Reads the levels on a thread of its own and hands each change to
/// `on_change`. The first reading is the baseline, not a change.
pub fn watch(on_change: impl Fn(Level) + Send + 'static) -> std::io::Result<()> {
    watch_with(read, on_change)
}

/// [`watch`] with another reader, for a system whose levels the app reads
/// itself (Windows' audio endpoint).
pub fn watch_with(
    read: fn() -> Levels,
    on_change: impl Fn(Level) + Send + 'static,
) -> std::io::Result<()> {
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

/// A volume or brightness key the notch answers instead of the system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    VolumeUp,
    VolumeDown,
    Mute,
    BrightnessUp,
    BrightnessDown,
}

/// The system's step: sixteen to the full range, or sixty-four with Option
/// and Shift held (`fine`), as macOS steps them.
pub fn stepped(value: f32, up: bool, fine: bool) -> f32 {
    let steps = if fine { 64. } else { 16. };
    let at = (value * steps).round();
    let next = if up { at + 1. } else { at - 1. };
    (next / steps).clamp(0., 1.)
}

/// What a key makes of the levels: the new value to write, as a level.
/// Raising a muted volume unmutes it, as the system does.
pub fn after(key: Key, now: &Levels, fine: bool) -> Option<Level> {
    match key {
        Key::VolumeUp | Key::VolumeDown => {
            let (volume, muted) = now.volume?;
            let up = key == Key::VolumeUp;
            let volume = stepped(volume, up, fine);
            Some(Level::Volume(volume, muted && !up && volume > 0.))
        }
        Key::Mute => now
            .volume
            .map(|(volume, muted)| Level::Volume(volume, !muted)),
        Key::BrightnessUp | Key::BrightnessDown => now
            .brightness
            .map(|value| Level::Brightness(stepped(value, key == Key::BrightnessUp, fine))),
    }
}

/// Answers a key on this Mac: reads, writes the new level and returns it;
/// `None` when the level cannot be written here, so the key goes on to the
/// system untouched.
pub fn press(key: Key, fine: bool) -> Option<Level> {
    #[cfg(target_os = "macos")]
    {
        let level = after(key, &read(), fine)?;
        mac::write(level).then_some(level)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (key, fine);
        None
    }
}

pub fn read() -> Levels {
    #[cfg(target_os = "macos")]
    return mac::read();
    #[cfg(not(target_os = "macos"))]
    Levels::default()
}

/// The default output's volume (0 to 1) and whether it is muted, on this
/// Mac; `None` elsewhere (Windows reads its endpoint in the app).
pub fn output_volume() -> Option<(f32, bool)> {
    #[cfg(target_os = "macos")]
    return mac::volume();
    #[cfg(not(target_os = "macos"))]
    None
}

/// Sets the default output's volume, leaving its mute as it is; false
/// where it cannot (another system, an output without a volume).
pub fn set_output_volume(volume: f32) -> bool {
    #[cfg(target_os = "macos")]
    return mac::set_volume(volume);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = volume;
        false
    }
}

/// The battery's charge and whether the charger is in, as the levels thread
/// last read it (macOS); never asks the system itself, so the interface can
/// call it while it draws.
pub fn battery() -> Option<(f32, bool)> {
    #[cfg(target_os = "macos")]
    return mac::cached_power(false);
    #[cfg(not(target_os = "macos"))]
    None
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

    pub(super) fn volume() -> Option<(f32, bool)> {
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

    #[link(name = "CoreAudio", kind = "framework")]
    unsafe extern "C" {
        fn AudioObjectSetPropertyData(
            object: u32,
            address: *const Address,
            qualifier_size: u32,
            qualifier: *const c_void,
            size: u32,
            data: *const c_void,
        ) -> i32;
    }

    fn set_property<T>(object: u32, selector: u32, scope: u32, value: T) -> bool {
        let address = Address {
            selector,
            scope,
            element: 0,
        };
        // SAFETY: `value` is a plain number of its own size.
        unsafe {
            AudioObjectSetPropertyData(
                object,
                &address,
                0,
                std::ptr::null(),
                std::mem::size_of::<T>() as u32,
                (&value as *const T).cast(),
            ) == 0
        }
    }

    type SetBrightness = unsafe extern "C" fn(display: u32, brightness: f32) -> i32;

    fn set_brightness() -> Option<SetBrightness> {
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
                let symbol = libc::dlsym(library, c"DisplayServicesSetBrightness".as_ptr());
                (!symbol.is_null()).then_some(symbol as usize)
            }
        });
        let address = (*loaded)?;
        // SAFETY: the symbol's C signature is `int (CGDirectDisplayID, float)`.
        Some(unsafe { std::mem::transmute::<usize, SetBrightness>(address) })
    }

    /// Sets the default output's main volume only.
    pub(super) fn set_volume(volume: f32) -> bool {
        let Some(device) = property::<u32>(SYSTEM_OBJECT, DEFAULT_OUTPUT, GLOBAL) else {
            return false;
        };
        set_property(device, MAIN_VOLUME, OUTPUT, volume.clamp(0., 1.))
    }

    /// Writes a level; false when this Mac does not take it.
    pub(super) fn write(level: super::Level) -> bool {
        match level {
            super::Level::Volume(volume, muted) => {
                let Some(device) = property::<u32>(SYSTEM_OBJECT, DEFAULT_OUTPUT, GLOBAL) else {
                    return false;
                };
                let wrote = set_property(device, MAIN_VOLUME, OUTPUT, volume);
                // Not every output can be muted; the volume still went.
                let _ = set_property(device, MUTE, OUTPUT, u32::from(muted));
                wrote
            }
            super::Level::Brightness(value) => set_brightness()
                // SAFETY: a display id and a level from 0 to 1.
                .is_some_and(|set| unsafe { set(CGMainDisplayID(), value) } == 0),
            super::Level::Power(..) => false,
        }
    }

    /// The internal battery's charge and whether the charger is in, from
    /// the power sources IOKit lists; `None` on a Mac without a battery.
    fn power() -> Option<(f32, bool)> {
        use crate::notch::cf::{self, Owned};
        #[link(name = "IOKit", kind = "framework")]
        unsafe extern "C" {
            fn IOPSCopyPowerSourcesInfo() -> cf::Ref;
            fn IOPSCopyPowerSourcesList(blob: cf::Ref) -> cf::Ref;
            fn IOPSGetPowerSourceDescription(blob: cf::Ref, source: cf::Ref) -> cf::Ref;
        }
        // SAFETY: the info and its list are owned here; each description is
        // borrowed from the info, which outlives the loop.
        unsafe {
            let info = Owned::new(IOPSCopyPowerSourcesInfo())?;
            let list = Owned::new(IOPSCopyPowerSourcesList(info.0))?;
            cf::items(list.0).into_iter().find_map(|source| {
                let description = IOPSGetPowerSourceDescription(info.0, source);
                if description.is_null() || !cf::says(description, c"Type", c"InternalBattery") {
                    return None;
                }
                let current = cf::number(description, c"Current Capacity")?;
                let most = cf::number(description, c"Max Capacity").filter(|most| *most > 0.)?;
                let plugged = cf::says(description, c"Power Source State", c"AC Power");
                Some(((current / most).clamp(0., 1.) as f32, plugged))
            })
        }
    }

    /// The battery as last read. The levels are read five times a second,
    /// and the power sources come from another process, so they are asked
    /// again only every two seconds.
    pub(super) fn cached_power(refresh: bool) -> Option<(f32, bool)> {
        use std::sync::Mutex;
        use std::time::{Duration, Instant};
        type Power = Option<(f32, bool)>;
        static LAST: Mutex<Option<(Instant, Power)>> = Mutex::new(None);
        let mut last = LAST.lock().unwrap_or_else(|error| error.into_inner());
        match *last {
            Some((at, power)) if !refresh || at.elapsed() < Duration::from_secs(2) => power,
            None if !refresh => None,
            _ => {
                let power = power();
                *last = Some((Instant::now(), power));
                power
            }
        }
    }

    pub(super) fn read() -> Levels {
        Levels {
            volume: volume(),
            brightness: brightness(),
            power: cached_power(true),
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
            power: None,
        };
        assert!(changes(&before, &before).is_empty());
        let jitter = Levels {
            volume: Some((0.502, false)),
            brightness: Some(0.801),
            power: None,
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

    #[test]
    fn the_battery_is_told_when_it_matters() {
        let at = |charge: f32, plugged: bool| Levels {
            power: Some((charge, plugged)),
            ..Levels::default()
        };
        // Draining or charging on its own says nothing.
        assert!(changes(&at(0.6, false), &at(0.59, false)).is_empty());
        assert!(changes(&at(0.6, true), &at(0.61, true)).is_empty());
        assert_eq!(
            changes(&at(0.6, false), &at(0.6, true)),
            [Level::Power(0.6, true)],
            "the charger went in"
        );
        assert_eq!(
            changes(&at(0.6, true), &at(0.6, false)),
            [Level::Power(0.6, false)],
            "and out"
        );
        assert_eq!(
            changes(&at(0.99, true), &at(1., true)),
            [Level::Power(1., true)],
            "full"
        );
        assert!(changes(&at(1., true), &at(1., true)).is_empty());
        assert_eq!(
            changes(&at(0.21, false), &at(0.2, false)),
            [Level::Power(0.2, false)],
            "a fifth left"
        );
        assert!(changes(&at(0.2, false), &at(0.19, false)).is_empty());
        assert_eq!(
            changes(&at(0.11, false), &at(0.1, false)),
            [Level::Power(0.1, false)]
        );
        // No battery says nothing.
        assert!(changes(&Levels::default(), &at(0.5, true)).is_empty());
    }

    #[test]
    fn keys_step_as_the_system_does() {
        assert_eq!(stepped(0.5, true, false), 9. / 16.);
        assert_eq!(stepped(0.5, false, true), 31. / 64.);
        assert_eq!(stepped(1., true, false), 1.);
        assert_eq!(stepped(0.01, false, false), 0.);
        let now = Levels {
            volume: Some((0.5, true)),
            brightness: Some(0.25),
            power: None,
        };
        assert_eq!(
            after(Key::VolumeUp, &now, false),
            Some(Level::Volume(9. / 16., false)),
            "unmutes"
        );
        assert_eq!(
            after(Key::VolumeDown, &now, false),
            Some(Level::Volume(7. / 16., true))
        );
        assert_eq!(
            after(Key::Mute, &now, false),
            Some(Level::Volume(0.5, false))
        );
        assert_eq!(
            after(Key::BrightnessDown, &now, false),
            Some(Level::Brightness(3. / 16.))
        );
        // A level the system does not report is left to the system.
        assert_eq!(after(Key::BrightnessUp, &Levels::default(), false), None);
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
        if let Some((charge, _)) = levels.power {
            assert!((0. ..=1.).contains(&charge));
        }
        assert_eq!(battery(), levels.power, "the cached reading");
    }
}
