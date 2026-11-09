//! Where the notch sits on each system, and the native window settings GPUI
//! does not offer. On macOS it reads the camera housing from `NSScreen`
//! (`safeAreaInsets` and the menu bar's auxiliary areas), drops the panel's
//! title bar, raises it over the menu bar on every Space and places it in
//! AppKit's coordinates. On Windows it is a topmost tool window on the
//! primary display, moved with `SetWindowPos`. Elsewhere, and in tests, GPUI
//! only resizes it.
use gpui::{App, DisplayId, Pixels, Point, Window, point, px};
use monocode_core::notch::{Frame, Screen};

/// The screen the notch is on, and where its top left is in GPUI's
/// coordinates (for opening the window).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Display {
    pub screen: Screen,
    pub origin: Point<Pixels>,
    pub display: Option<DisplayId>,
    /// AppKit's frame of the screen, bottom-left origin.
    #[cfg(all(target_os = "macos", not(test)))]
    pub native: Option<(f64, f64, f64, f64)>,
}

/// GPUI's view of the primary display, for the systems without a notch API.
fn primary(cx: &App) -> Display {
    let display = cx.primary_display();
    let bounds = display.as_ref().map(|display| display.bounds());
    Display {
        screen: Screen {
            width: bounds.map_or(1440., |bounds| f32::from(bounds.size.width)),
            ..Screen::default()
        },
        origin: bounds.map_or(point(px(0.), px(0.)), |bounds| bounds.origin),
        display: display.map(|display| display.id()),
        #[cfg(all(target_os = "macos", not(test)))]
        native: None,
    }
}

#[cfg(all(target_os = "macos", not(test)))]
mod mac {
    use super::*;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSScreen, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask, NSWorkspace,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    /// Over the menu bar and its menus (NSMainMenuWindowLevel + 3), as the
    /// hardware notch is.
    const LEVEL: isize = 24 + 3;

    fn ns_window(window: &Window) -> Option<objc2::rc::Retained<NSWindow>> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            return None;
        };
        // SAFETY: GPUI's AppKit handle points at its live content view.
        let view = unsafe { &*appkit.ns_view.as_ptr().cast::<objc2_app_kit::NSView>() };
        view.window()
    }

    /// The built-in screen when it has a camera housing, else the main one.
    pub(super) fn screen(cx: &App) -> Display {
        let Some(mtm) = MainThreadMarker::new() else {
            return primary(cx);
        };
        let screens = NSScreen::screens(mtm);
        let chosen = screens
            .iter()
            .find(|screen| screen.safeAreaInsets().top > 0.)
            .or_else(|| NSScreen::mainScreen(mtm));
        let Some(chosen) = chosen else {
            return primary(cx);
        };
        let frame = chosen.frame();
        let visible = chosen.visibleFrame();
        let top = frame.origin.y + frame.size.height;
        let menu_bar = (top - (visible.origin.y + visible.size.height)).max(0.);
        let (left, right) = if chosen.safeAreaInsets().top > 0. {
            (
                chosen.auxiliaryTopLeftArea().size.width,
                chosen.auxiliaryTopRightArea().size.width,
            )
        } else {
            (0., 0.)
        };
        // GPUI puts the primary screen's top left at the origin, y down.
        let primary_top = screens.iter().next().map_or(top, |first| {
            first.frame().origin.y + first.frame().size.height
        });
        let display = cx.displays().into_iter().find(|display| {
            let bounds = display.bounds();
            (f64::from(f32::from(bounds.origin.x)) - frame.origin.x).abs() < 1.
                && (f64::from(f32::from(bounds.size.width)) - frame.size.width).abs() < 1.
        });
        Display {
            screen: Screen {
                width: frame.size.width as f32,
                safe_top: chosen.safeAreaInsets().top as f32,
                left_area: left as f32,
                right_area: right as f32,
                menu_bar: menu_bar as f32,
            },
            origin: point(px(frame.origin.x as f32), px((primary_top - top) as f32)),
            display: display.map(|display| display.id()),
            native: Some((
                frame.origin.x,
                frame.origin.y,
                frame.size.width,
                frame.size.height,
            )),
        }
    }

    /// No title bar, no shadow, above the menu bar on every Space and over
    /// full-screen apps, and out of Mission Control and the window cycle.
    pub(super) fn style(window: &Window) {
        let Some(ns_window) = ns_window(window) else {
            return;
        };
        ns_window.setStyleMask(
            ns_window.styleMask()
                & !(NSWindowStyleMask::Titled
                    | NSWindowStyleMask::FullSizeContentView
                    | NSWindowStyleMask::Resizable),
        );
        ns_window.setHasShadow(false);
        ns_window.setLevel(LEVEL);
        ns_window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
    }

    pub(super) fn place(window: &mut Window, cx: &mut App, display: &Display, frame: Frame) {
        let (Some(ns_window), Some((x, y, _, height))) = (ns_window(window), display.native) else {
            window.resize(gpui::size(px(frame.width), px(frame.height)));
            return;
        };
        // AppKit's origin is the screen's bottom left.
        let rect = NSRect::new(
            NSPoint::new(
                x + f64::from(frame.x),
                y + height - f64::from(frame.y) - f64::from(frame.height),
            ),
            NSSize::new(f64::from(frame.width), f64::from(frame.height)),
        );
        // A borderless window is never pushed below the menu bar; ask for the
        // frame exactly.
        ns_window.setFrame_display(rect, true);
        window.bounds_changed(cx);
    }

    /// Watches the pointer over the whole screen, since AppKit sends no
    /// hover to a panel while another app is active: tells `hovered` each
    /// time the pointer goes in or out of the notch's window.
    pub(super) fn watch_pointer(
        window: &Window,
        hovered: async_channel::Sender<bool>,
    ) -> Option<PointerWatch> {
        use objc2_app_kit::{NSEvent, NSEventMask};
        use std::cell::Cell;
        use std::ptr::NonNull;
        let ns_window = ns_window(window)?;
        let ignore = ns_window.clone();
        // Clicks go through until the pointer is over the notch.
        ignore.setIgnoresMouseEvents(true);
        let area: std::rc::Rc<Cell<Option<[f64; 4]>>> = std::rc::Rc::new(Cell::new(None));
        let probe: std::rc::Rc<dyn Fn() -> bool> = {
            let area = area.clone();
            std::rc::Rc::new(move || {
                let pointer = NSEvent::mouseLocation();
                let frame = ns_window.frame();
                // The pointer in the window, from its top left.
                let x = pointer.x - frame.origin.x;
                let y = frame.origin.y + frame.size.height - pointer.y;
                let [left, top, width, height] =
                    area.get()
                        .unwrap_or([0., 0., frame.size.width, frame.size.height]);
                x >= left && x <= left + width && y >= top && y <= top + height
            })
        };
        let inside = std::rc::Rc::new(Cell::new(false));
        let check = {
            let probe = probe.clone();
            move || {
                let now = probe();
                if now != inside.get() {
                    inside.set(now);
                    // The window is a fixed room larger than the notch:
                    // outside the notch, clicks go to what is under it.
                    ignore.setIgnoresMouseEvents(!now);
                    let _ = hovered.try_send(now);
                }
            }
        };
        let mask = NSEventMask::MouseMoved | NSEventMask::LeftMouseDragged;
        let global_check = check.clone();
        let global = block2::RcBlock::new(move |_: NonNull<NSEvent>| global_check());
        let local = block2::RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            check();
            event.as_ptr()
        });
        let mut monitors = Vec::new();
        monitors.extend(NSEvent::addGlobalMonitorForEventsMatchingMask_handler(
            mask, &global,
        ));
        // SAFETY: the handler hands every event back unchanged.
        monitors
            .extend(unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &local) });
        Some(PointerWatch(monitors, probe, area))
    }

    pub(crate) struct PointerWatch(
        Vec<objc2::rc::Retained<objc2::runtime::AnyObject>>,
        std::rc::Rc<dyn Fn() -> bool>,
        std::rc::Rc<std::cell::Cell<Option<[f64; 4]>>>,
    );

    impl PointerWatch {
        /// Whether the pointer is over the notch right now.
        pub(crate) fn inside(&self) -> bool {
            (self.1)()
        }

        /// Where the notch is in its window, which may be larger.
        pub(crate) fn set_area(&self, left: f32, top: f32, width: f32, height: f32) {
            self.2.set(Some([
                f64::from(left),
                f64::from(top),
                f64::from(width),
                f64::from(height),
            ]));
        }
    }

    impl Drop for PointerWatch {
        fn drop(&mut self) {
            for monitor in &self.0 {
                // SAFETY: each one came from NSEvent's monitor methods.
                unsafe { objc2_app_kit::NSEvent::removeMonitor(monitor) };
            }
        }
    }

    /// Keeps the app out of App Nap while the notch is in use: an inactive
    /// app's timers and frames are otherwise held back by about a second.
    pub(crate) struct Responsive(
        objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2::runtime::NSObjectProtocol>>,
    );

    impl Drop for Responsive {
        fn drop(&mut self) {
            // SAFETY: the activity came from this process's beginActivity.
            unsafe { objc2_foundation::NSProcessInfo::processInfo().endActivity(&self.0) };
        }
    }

    pub(super) fn responsive() -> Option<Responsive> {
        use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
        let activity = NSProcessInfo::processInfo().beginActivityWithOptions_reason(
            NSActivityOptions::UserInitiatedAllowingIdleSystemSleep
                | NSActivityOptions::LatencyCritical,
            &NSString::from_str("The notch is in use"),
        );
        Some(Responsive(activity))
    }

    // The volume and brightness keys, taken from the system so the notch
    // shows them instead of macOS's indicator. An event tap sees only
    // system-defined events (the media keys), never ordinary typing.
    mod keys {
        use monocode_core::notch::system::{self, Key, Level};
        use std::ffi::c_void;
        use std::sync::{Mutex, OnceLock};

        type Ref = *mut c_void;
        type Callback = extern "C" fn(Ref, u32, Ref, Ref) -> Ref;

        #[link(name = "CoreGraphics", kind = "framework")]
        unsafe extern "C" {
            fn CGEventTapCreate(
                tap: u32,
                place: u32,
                options: u32,
                mask: u64,
                callback: Callback,
                user: Ref,
            ) -> Ref;
            fn CGEventTapEnable(tap: Ref, enable: bool);
        }

        #[link(name = "CoreFoundation", kind = "framework")]
        unsafe extern "C" {
            fn CFMachPortCreateRunLoopSource(allocator: Ref, port: Ref, order: isize) -> Ref;
            fn CFRunLoopGetMain() -> Ref;
            fn CFRunLoopAddSource(run_loop: Ref, source: Ref, mode: Ref);
            fn CFDictionaryCreate(
                allocator: Ref,
                keys: *const Ref,
                values: *const Ref,
                count: isize,
                key_callbacks: Ref,
                value_callbacks: Ref,
            ) -> Ref;
            fn CFRelease(object: Ref);
            static kCFRunLoopCommonModes: Ref;
            static kCFBooleanTrue: Ref;
            static kCFTypeDictionaryKeyCallBacks: c_void;
            static kCFTypeDictionaryValueCallBacks: c_void;
        }

        #[link(name = "ApplicationServices", kind = "framework")]
        unsafe extern "C" {
            fn AXIsProcessTrusted() -> bool;
            fn AXIsProcessTrustedWithOptions(options: Ref) -> bool;
            static kAXTrustedCheckOptionPrompt: Ref;
        }

        /// kCGEventSystemDefined (NX_SYSDEFINED).
        const SYSTEM_DEFINED: u64 = 14;
        /// The tap's own events: switched off by a slow callback or the user.
        const DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
        const DISABLED_BY_USER: u32 = 0xFFFF_FFFF;
        /// NX_SUBTYPE_AUX_CONTROL_BUTTONS: the media keys.
        const MEDIA_KEYS: i16 = 8;

        struct Tap {
            port: usize,
            levels: async_channel::Sender<Level>,
        }
        static TAP: OnceLock<Mutex<Option<Tap>>> = OnceLock::new();

        fn tap() -> &'static Mutex<Option<Tap>> {
            TAP.get_or_init(|| Mutex::new(None))
        }

        /// Whether MonoBOT may watch the keys, asking macOS once (it shows
        /// its own prompt to open Accessibility).
        pub(crate) fn allowed(ask: bool) -> bool {
            // SAFETY: plain calls; the dictionary lives for the call.
            unsafe {
                if AXIsProcessTrusted() {
                    return true;
                }
                if !ask {
                    return false;
                }
                let keys = [kAXTrustedCheckOptionPrompt];
                let values = [kCFBooleanTrue];
                let options = CFDictionaryCreate(
                    std::ptr::null_mut(),
                    keys.as_ptr(),
                    values.as_ptr(),
                    1,
                    (&raw const kCFTypeDictionaryKeyCallBacks) as Ref,
                    (&raw const kCFTypeDictionaryValueCallBacks) as Ref,
                );
                let trusted = AXIsProcessTrustedWithOptions(options);
                if !options.is_null() {
                    CFRelease(options);
                }
                trusted
            }
        }

        /// The key a media event names, whether it is pressed, and whether
        /// Option and Shift ask for a fine step.
        fn decode(event: Ref) -> Option<(Key, bool, bool)> {
            use objc2::encode::{Encode, Encoding, RefEncode};
            use objc2::runtime::AnyObject;
            use objc2::{class, msg_send};
            /// `CGEventRef`, encoded as AppKit declares it, so objc2's checked
            /// message sends accept it.
            #[repr(C)]
            struct CgEvent([u8; 0]);
            unsafe impl RefEncode for CgEvent {
                const ENCODING_REF: Encoding =
                    Encoding::Pointer(&Encoding::Struct("__CGEvent", &[]));
            }
            let _ = <*mut CgEvent as Encode>::ENCODING;
            let event = event.cast::<CgEvent>();
            // SAFETY: NSEvent wraps the tap's live CGEvent for this call.
            unsafe {
                let ns: *mut AnyObject = msg_send![class!(NSEvent), eventWithCGEvent: event];
                if ns.is_null() {
                    return None;
                }
                let subtype: i16 = msg_send![ns, subtype];
                if subtype != MEDIA_KEYS {
                    return None;
                }
                let data: isize = msg_send![ns, data1];
                let flags: usize = msg_send![ns, modifierFlags];
                let code = (data as usize & 0xFFFF_0000) >> 16;
                let state = (data as usize & 0xFF00) >> 8;
                let key = match code {
                    0 => Key::VolumeUp,
                    1 => Key::VolumeDown,
                    2 => Key::BrightnessUp,
                    3 => Key::BrightnessDown,
                    7 => Key::Mute,
                    _ => return None,
                };
                // Option (1 << 19) and Shift (1 << 17), as the system's fine step.
                let fine = flags & (1 << 19) != 0 && flags & (1 << 17) != 0;
                Some((key, state == 0x0A, fine))
            }
        }

        /// Whether the notch answers the keys now (Settings › Look).
        static TAKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

        pub(crate) fn set_taking(on: bool) {
            TAKING.store(on, std::sync::atomic::Ordering::Relaxed);
        }

        extern "C" fn on_event(proxy: Ref, kind: u32, event: Ref, user: Ref) -> Ref {
            // A panic must not cross into CoreGraphics (it would abort the
            // app): the key then goes on to the system.
            std::panic::catch_unwind(|| handle(proxy, kind, event, user)).unwrap_or(event)
        }

        fn handle(_proxy: Ref, kind: u32, event: Ref, _user: Ref) -> Ref {
            let guard = tap().lock().unwrap_or_else(|e| e.into_inner());
            let Some(tap) = guard.as_ref() else {
                return event;
            };
            if kind == DISABLED_BY_TIMEOUT || kind == DISABLED_BY_USER {
                // SAFETY: the port this tap was created with.
                unsafe { CGEventTapEnable(tap.port as Ref, true) };
                return event;
            }
            if !TAKING.load(std::sync::atomic::Ordering::Relaxed) {
                return event;
            }
            let Some((key, down, fine)) = decode(event) else {
                return event;
            };
            let now = system::read();
            let ours = match key {
                Key::BrightnessUp | Key::BrightnessDown => now.brightness.is_some(),
                _ => now.volume.is_some(),
            };
            if !ours {
                return event;
            }
            if down {
                // A property write: a millisecond or two, on the main thread.
                match system::press(key, fine) {
                    Some(level) => {
                        let _ = tap.levels.try_send(level);
                    }
                    // Not writable here: the system keeps the key.
                    None => return event,
                }
            }
            std::ptr::null_mut()
        }

        /// Starts taking the keys; false while Accessibility is off (try
        /// again later) or when the tap cannot be made.
        pub(crate) fn start(levels: async_channel::Sender<Level>) -> bool {
            let mut guard = tap().lock().unwrap_or_else(|e| e.into_inner());
            if guard.is_some() {
                return true;
            }
            if !allowed(false) {
                return false;
            }
            // SAFETY: a tap on the HID stream (or the session's), added to
            // the main run loop; the callback reads only the static state.
            unsafe {
                let port = [0u32, 1u32].into_iter().find_map(|location| {
                    let port = CGEventTapCreate(
                        location,
                        0,
                        0,
                        1 << SYSTEM_DEFINED,
                        on_event,
                        std::ptr::null_mut(),
                    );
                    (!port.is_null()).then_some(port)
                });
                let Some(port) = port else {
                    return false;
                };
                let source = CFMachPortCreateRunLoopSource(std::ptr::null_mut(), port, 0);
                if source.is_null() {
                    return false;
                }
                CFRunLoopAddSource(CFRunLoopGetMain(), source, kCFRunLoopCommonModes);
                CGEventTapEnable(port, true);
                *guard = Some(Tap {
                    port: port as usize,
                    levels,
                });
            }
            true
        }
    }
    pub(crate) use keys::{allowed as keys_allowed, set_taking, start as start_keys};

    pub(super) fn reduce_motion() -> Option<bool> {
        Some(NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion())
    }
}

#[cfg(all(target_os = "windows", not(test)))]
mod win {
    use super::*;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SetWindowPos,
    };

    fn hwnd(window: &Window) -> Option<HWND> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return None;
        };
        Some(HWND(handle.hwnd.get() as _))
    }

    /// GPUI already makes a pop-up a borderless, topmost tool window that
    /// shows without taking focus.
    pub(super) fn style(_window: &Window) {}

    /// The default output's volume, through its audio endpoint.
    pub(crate) mod audio {
        use monocode_core::notch::system::{self, Key, Level, Levels};
        use std::cell::RefCell;
        use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
        use windows::Win32::Media::Audio::{
            IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender,
        };
        use windows::Win32::System::Com::{
            CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        };

        thread_local! {
            static ENDPOINT: RefCell<Option<IAudioEndpointVolume>> = const { RefCell::new(None) };
        }

        fn endpoint() -> Option<IAudioEndpointVolume> {
            // SAFETY: COM for this thread, then the default render endpoint.
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                let devices: IMMDeviceEnumerator =
                    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
                let device = devices.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
                device
                    .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
                    .ok()
            }
        }

        /// Runs `work` on this thread's endpoint, finding it again once if
        /// the output changed under it.
        fn with<T>(work: impl Fn(&IAudioEndpointVolume) -> Option<T>) -> Option<T> {
            ENDPOINT.with(|cell| {
                for _ in 0..2 {
                    if cell.borrow().is_none() {
                        *cell.borrow_mut() = endpoint();
                    }
                    let found = cell.borrow().as_ref().and_then(&work);
                    if found.is_some() {
                        return found;
                    }
                    *cell.borrow_mut() = None;
                }
                None
            })
        }

        pub(crate) fn read() -> Levels {
            // SAFETY: plain reads on a live endpoint.
            let volume = with(|endpoint| unsafe {
                let volume = endpoint.GetMasterVolumeLevelScalar().ok()?;
                let muted = endpoint.GetMute().ok()?.as_bool();
                Some((volume.clamp(0., 1.), muted))
            });
            Levels {
                volume,
                brightness: None,
            }
        }

        /// Answers a volume key: the new level, or `None` when Windows keeps
        /// the key.
        pub(crate) fn press(key: Key) -> Option<Level> {
            let level = system::after(key, &read(), false)?;
            let Level::Volume(volume, muted) = level else {
                return None;
            };
            // SAFETY: plain writes on a live endpoint.
            with(|endpoint| unsafe {
                endpoint
                    .SetMasterVolumeLevelScalar(volume, std::ptr::null())
                    .ok()?;
                endpoint.SetMute(muted, std::ptr::null()).ok()?;
                Some(())
            })?;
            Some(level)
        }
    }

    /// The volume keys, taken before Windows shows its own flyout: a
    /// low-level keyboard hook on a thread of its own, which passes every
    /// other key on untouched.
    pub(crate) mod keys {
        use super::audio;
        use monocode_core::notch::system::{Key, Level};
        use std::sync::OnceLock;
        use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            VK_VOLUME_DOWN, VK_VOLUME_MUTE, VK_VOLUME_UP,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            CallNextHookEx, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, MSG, SetWindowsHookExW,
            WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
        };

        static LEVELS: OnceLock<async_channel::Sender<Level>> = OnceLock::new();
        /// Whether the notch answers the keys now (Settings › Look).
        static TAKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

        pub(crate) fn set_taking(on: bool) {
            TAKING.store(on, std::sync::atomic::Ordering::Relaxed);
        }

        unsafe extern "system" fn on_key(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
            if code == HC_ACTION as i32 && TAKING.load(std::sync::atomic::Ordering::Relaxed) {
                // SAFETY: for WH_KEYBOARD_LL, lparam points at a KBDLLHOOKSTRUCT.
                let key = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
                let ours = match key.vkCode as u16 {
                    code if code == VK_VOLUME_UP.0 => Some(Key::VolumeUp),
                    code if code == VK_VOLUME_DOWN.0 => Some(Key::VolumeDown),
                    code if code == VK_VOLUME_MUTE.0 => Some(Key::Mute),
                    _ => None,
                };
                if let Some(ours) = ours {
                    let down = wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN;
                    if !down {
                        return LRESULT(1);
                    }
                    if let Some(level) = audio::press(ours) {
                        if let Some(levels) = LEVELS.get() {
                            let _ = levels.try_send(level);
                        }
                        return LRESULT(1);
                    }
                }
            }
            // SAFETY: every other key goes on to the next hook.
            unsafe { CallNextHookEx(None, code, wparam, lparam) }
        }

        /// Starts the hook's thread; it lives as long as the app.
        pub(crate) fn start(levels: async_channel::Sender<Level>) -> bool {
            if LEVELS.set(levels).is_err() {
                return true;
            }
            std::thread::Builder::new()
                .name("notch-keys".into())
                .spawn(|| {
                    // SAFETY: the hook runs on this thread's message loop.
                    unsafe {
                        if SetWindowsHookExW(WH_KEYBOARD_LL, Some(on_key), None, 0).is_err() {
                            return;
                        }
                        let mut message = MSG::default();
                        while GetMessageW(&mut message, None, 0, 0).as_bool() {}
                    }
                })
                .is_ok()
        }
    }

    pub(super) fn place(window: &mut Window, _cx: &mut App, display: &Display, frame: Frame) {
        let Some(hwnd) = hwnd(window) else {
            window.resize(gpui::size(px(frame.width), px(frame.height)));
            return;
        };
        // SetWindowPos works in device pixels.
        let scale = window.scale_factor();
        let device = |logical: f32| (logical * scale).round() as i32;
        let x = f32::from(display.origin.x) + frame.x;
        let y = f32::from(display.origin.y) + frame.y;
        let _ = unsafe {
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                device(x),
                device(y),
                device(frame.width),
                device(frame.height),
                SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            )
        };
    }
}

pub(crate) fn screen(cx: &App) -> Display {
    #[cfg(all(target_os = "macos", not(test)))]
    return mac::screen(cx);
    #[cfg(not(all(target_os = "macos", not(test))))]
    primary(cx)
}

pub(crate) fn style(window: &Window) {
    #[cfg(all(target_os = "macos", not(test)))]
    mac::style(window);
    #[cfg(all(target_os = "windows", not(test)))]
    win::style(window);
    #[cfg(any(test, not(any(target_os = "macos", target_os = "windows"))))]
    let _ = window;
}

pub(crate) fn place(window: &mut Window, cx: &mut App, display: &Display, frame: Frame) {
    #[cfg(all(target_os = "macos", not(test)))]
    return mac::place(window, cx, display, frame);
    #[cfg(all(target_os = "windows", not(test)))]
    return win::place(window, cx, display, frame);
    #[cfg(any(test, not(any(target_os = "macos", target_os = "windows"))))]
    {
        let _ = (cx, display);
        window.resize(gpui::size(px(frame.width), px(frame.height)));
    }
}

/// Keeps the pointer watch alive; dropping it stops watching.
#[cfg(all(target_os = "macos", not(test)))]
pub(crate) use mac::PointerWatch;
#[cfg(not(all(target_os = "macos", not(test))))]
pub(crate) struct PointerWatch;
#[cfg(not(all(target_os = "macos", not(test))))]
impl PointerWatch {
    pub(crate) fn inside(&self) -> bool {
        false
    }
    pub(crate) fn set_area(&self, _: f32, _: f32, _: f32, _: f32) {}
}

/// Where GPUI cannot see hover on its own (an inactive app's panel on
/// macOS), watches the pointer for the notch. Elsewhere GPUI's hover does.
pub(crate) fn watch_pointer(
    window: &Window,
    hovered: async_channel::Sender<bool>,
) -> Option<PointerWatch> {
    #[cfg(all(target_os = "macos", not(test)))]
    return mac::watch_pointer(window, hovered);
    #[cfg(not(all(target_os = "macos", not(test))))]
    {
        let _ = (window, hovered);
        None
    }
}

#[cfg(all(target_os = "macos", not(test)))]
pub(crate) use mac::Responsive;
#[cfg(not(all(target_os = "macos", not(test))))]
pub(crate) struct Responsive;

/// Holds the app at full pace while the notch is in use; dropping it lets
/// the system throttle it again.
pub(crate) fn responsive() -> Option<Responsive> {
    #[cfg(all(target_os = "macos", not(test)))]
    return mac::responsive();
    #[cfg(not(all(target_os = "macos", not(test))))]
    None
}

/// Takes the volume and brightness keys from the system, so the notch shows
/// them instead of its indicator. False while the system does not allow it
/// yet (macOS's Accessibility), so the caller tries again later.
pub(crate) fn take_system_keys(
    levels: async_channel::Sender<monocode_core::notch::system::Level>,
) -> bool {
    #[cfg(all(target_os = "macos", not(test)))]
    return mac::start_keys(levels);
    #[cfg(all(target_os = "windows", not(test)))]
    return win::keys::start(levels);
    #[cfg(any(test, not(any(target_os = "macos", target_os = "windows"))))]
    {
        let _ = levels;
        false
    }
}

/// Gives the keys back to the system, or takes them again.
pub(crate) fn set_system_keys(on: bool) {
    #[cfg(all(target_os = "macos", not(test)))]
    mac::set_taking(on);
    #[cfg(all(target_os = "windows", not(test)))]
    win::keys::set_taking(on);
    #[cfg(any(test, not(any(target_os = "macos", target_os = "windows"))))]
    let _ = on;
}

/// Asks the system once to let MonoBOT take those keys (macOS shows its
/// Accessibility prompt); true when it already may.
pub(crate) fn ask_for_system_keys() -> bool {
    #[cfg(all(target_os = "macos", not(test)))]
    return mac::keys_allowed(true);
    #[cfg(not(all(target_os = "macos", not(test))))]
    true
}

/// Watches the levels where the app reads them itself (Windows' endpoint);
/// elsewhere the core reads them.
pub(crate) fn watch_levels(
    on_change: impl Fn(monocode_core::notch::system::Level) + Send + 'static,
) -> std::io::Result<()> {
    #[cfg(all(target_os = "windows", not(test)))]
    return monocode_core::notch::system::watch_with(win::audio::read, on_change);
    #[cfg(not(all(target_os = "windows", not(test))))]
    monocode_core::notch::system::watch(on_change)
}

/// The system's reduced motion setting, where the system says it.
pub(crate) fn reduce_motion() -> Option<bool> {
    #[cfg(all(target_os = "macos", not(test)))]
    return mac::reduce_motion();
    #[cfg(not(all(target_os = "macos", not(test))))]
    None
}
