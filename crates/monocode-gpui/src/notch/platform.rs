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
    fn chosen(
        screens: &objc2_foundation::NSArray<NSScreen>,
        mtm: MainThreadMarker,
    ) -> Option<objc2::rc::Retained<NSScreen>> {
        screens
            .iter()
            .find(|screen| screen.safeAreaInsets().top > 0.)
            .or_else(|| NSScreen::mainScreen(mtm))
    }

    pub(super) fn screen(cx: &App) -> Display {
        let Some(mtm) = MainThreadMarker::new() else {
            return primary(cx);
        };
        let screens = NSScreen::screens(mtm);
        let Some(chosen) = chosen(&screens, mtm) else {
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
        // Nothing to bring back on relaunch: without this AppKit rewrites
        // its saved window state off the main thread each time the notch
        // moves or resizes.
        // SAFETY: `setRestorable:` takes a BOOL, as sent.
        let _: () = unsafe { objc2::msg_send![&*ns_window, setRestorable: false] };
        // Every change of the notch's shape resizes its window, and AppKit
        // draws a resized layer inside its own transaction. With GPUI's two
        // drawables both still held there, that draw waited up to a second
        // for one to come back, and the whole app with it: the hover's
        // panel opened that much late. A third drawable is always free. It
        // costs one more frame of the window's size, small while closed.
        let handle = HasWindowHandle::window_handle(window).ok();
        if let Some(RawWindowHandle::AppKit(appkit)) = handle.map(|handle| handle.as_raw()) {
            // SAFETY: GPUI's AppKit handle points at its live content view,
            // whose backing layer is the renderer's CAMetalLayer;
            // `setMaximumDrawableCount:` takes an NSUInteger.
            unsafe {
                let view = &*appkit.ns_view.as_ptr().cast::<objc2::runtime::AnyObject>();
                let layer: *mut objc2::runtime::AnyObject = objc2::msg_send![view, layer];
                if let Some(layer) = layer.as_ref() {
                    let _: () = objc2::msg_send![layer, setMaximumDrawableCount: 3usize];
                }
            }
        }
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

    /// The pointer's x on the screen in physical pixels, as the notch's
    /// place on the screen is measured, for the face's glance.
    pub(super) fn pointer_x() -> Option<f32> {
        let mtm = MainThreadMarker::new()?;
        let screen = chosen(&NSScreen::screens(mtm), mtm)?;
        let x = objc2_app_kit::NSEvent::mouseLocation().x;
        Some((x * screen.backingScaleFactor()) as f32)
    }

    /// What the user copies, for the notch's Clipboard page. The pasteboard
    /// counts its changes, so the thread reads what was copied only when the
    /// count moved, and never on the interface thread.
    pub(crate) mod clipboard {
        use super::super::Copied;
        use objc2_app_kit::{
            NSBitmapImageFileType, NSBitmapImageRep, NSPasteboard, NSPasteboardTypePNG,
            NSPasteboardTypeString, NSPasteboardTypeTIFF,
        };
        use objc2_foundation::NSDictionary;

        /// The marks apps put on a copy to keep it out of clipboard
        /// histories (nspasteboard.org): a password manager's copy, and what
        /// an app copies for itself.
        const HIDDEN: [&str; 4] = [
            "org.nspasteboard.ConcealedType",
            "org.nspasteboard.TransientType",
            "org.nspasteboard.AutoGeneratedType",
            "com.agilebits.onepassword",
        ];

        /// The copied text, or the copied picture (as PNG) where there is
        /// no text; `None` for what is neither, is too large, or is marked
        /// to be kept out of histories.
        fn read(board: &NSPasteboard, largest: usize, largest_picture: usize) -> Option<Copied> {
            let types = board.types()?;
            if types
                .iter()
                .any(|kind| HIDDEN.contains(&kind.to_string().as_str()))
            {
                return None;
            }
            // SAFETY: AppKit's constant type names.
            let (text, png, tiff) = unsafe {
                (
                    NSPasteboardTypeString,
                    NSPasteboardTypePNG,
                    NSPasteboardTypeTIFF,
                )
            };
            // A spreadsheet's cells come as text and as a picture of them:
            // the text is what was copied.
            if let Some(copied) = board.stringForType(text) {
                let copied = copied.to_string();
                return (copied.len() <= largest).then_some(Copied::Text(copied));
            }
            let (data, is_png) = match board.dataForType(png) {
                Some(data) => (data, true),
                None => (board.dataForType(tiff)?, false),
            };
            if data.length() > largest_picture {
                return None;
            }
            let picture = NSBitmapImageRep::imageRepWithData(&data)?;
            let (width, height) = (picture.pixelsWide(), picture.pixelsHigh());
            if width <= 0 || height <= 0 {
                return None;
            }
            let bytes = if is_png {
                data.to_vec()
            } else {
                // SAFETY: a PNG with no properties.
                unsafe {
                    picture.representationUsingType_properties(
                        NSBitmapImageFileType::PNG,
                        &NSDictionary::new(),
                    )
                }?
                .to_vec()
            };
            Some(Copied::Picture(bytes, width as u32, height as u32))
        }

        /// Sends each new copy until the receiver is gone.
        pub(crate) fn watch(
            copies: async_channel::Sender<Copied>,
            largest: usize,
            largest_picture: usize,
        ) -> bool {
            std::thread::Builder::new()
                .name("notch-clipboard".into())
                .spawn(move || {
                    let board = NSPasteboard::generalPasteboard();
                    let mut seen = board.changeCount();
                    while !copies.is_closed() {
                        std::thread::sleep(std::time::Duration::from_millis(400));
                        let now = board.changeCount();
                        if now == seen {
                            continue;
                        }
                        seen = now;
                        let copy =
                            objc2::rc::autoreleasepool(|_| read(&board, largest, largest_picture));
                        if let Some(copy) = copy {
                            let _ = copies.try_send(copy);
                        }
                    }
                })
                .is_ok()
        }
    }

    /// What plays, from macOS's now playing service (MediaRemote): the
    /// track Control Center shows (Spotify, Music, a browser's video).
    /// Since macOS 15.4 the service answers only Apple's own programs, so
    /// the reading runs in `osascript`, which loads the framework and prints
    /// one JSON line each time what plays changes, and ends by itself when
    /// MonoBOT does. Each command is one short `osascript` of its own. The
    /// service keeps the cover to itself, so it comes from Apple's catalog.
    pub(crate) mod media {
        use super::super::MediaCommand;
        use monocode_core::notch::media::{self, Playhead, Playing, Report, Track};
        use std::io::BufRead;
        use std::process::{Child, Command, Stdio};
        use std::sync::Arc;
        use std::sync::mpsc::{self, RecvTimeoutError};
        use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

        const FRAMEWORK: &str = "ObjC.import('Foundation');\
            $.NSBundle.bundleWithPath('/System/Library/PrivateFrameworks/MediaRemote.framework/').load;";

        /// Prints what plays as one JSON line whenever it changes (`{}` for
        /// nothing), until its parent goes away. It looks twice a second
        /// while something plays and less often at rest, where only a press
        /// of play is news.
        const WATCH: &str = r#"
ObjC.bindFunction('getppid', ['int', []]);
const R = $.NSClassFromString('MRNowPlayingRequest');
const out = $.NSFileHandle.fileHandleWithStandardOutput;
const parent = $.getppid();
const text = v => (v === undefined || v.isNil()) ? '' : ObjC.unwrap(v);
const num = v => (v === undefined || v.isNil()) ? null : v.doubleValue;
let last = null;
while (!R.isNil() && $.getppid() === parent) {
  let line = '{}';
  const playing = R.localIsPlaying;
  const item = R.localNowPlayingItem;
  if (!item.isNil() && !item.nowPlayingInfo.isNil()) {
    const info = item.nowPlayingInfo;
    const get = k => info.objectForKey('kMRMediaRemoteNowPlayingInfo' + k);
    const stamp = get('Timestamp');
    const path = R.localNowPlayingPlayerPath;
    line = JSON.stringify({
      app: path.isNil() ? '' : text(path.client.bundleIdentifier),
      title: text(get('Title')), artist: text(get('Artist')), album: text(get('Album')),
      duration: num(get('Duration')), elapsed: num(get('ElapsedTime')),
      stamp: stamp.isNil() ? null : stamp.timeIntervalSince1970,
      playing: playing,
    });
  }
  if (line !== last) {
    out.writeData($(line + '\n').dataUsingEncoding($.NSUTF8StringEncoding));
    last = line;
  }
  $.NSThread.sleepForTimeInterval(playing ? 0.5 : 1.5);
}
"#;

        const POLL: Duration = Duration::from_millis(250);
        /// A stamp older than this is a stale one, not playback.
        const STALE: f64 = 10. * 60.;
        /// How long the reader waits before starting again when it ended,
        /// and how many times in a row it tries.
        const RESTART: Duration = Duration::from_secs(5);
        const TRIES: u32 = 5;

        enum Event {
            Report(Option<Report>),
            Cover(Track, Option<Arc<[u8]>>),
            Ended,
        }

        fn osascript(script: &str) -> Command {
            let mut command = Command::new("/usr/bin/osascript");
            command
                .args(["-l", "JavaScript", "-e", script])
                .stdin(Stdio::null())
                .stderr(Stdio::null());
            command
        }

        fn start_reader(events: &mpsc::Sender<Event>) -> Option<Child> {
            let mut child = osascript(&format!("{FRAMEWORK}{WATCH}"))
                .stdout(Stdio::piped())
                .spawn()
                .ok()?;
            let stdout = child.stdout.take()?;
            let events = events.clone();
            let started = std::thread::Builder::new()
                .name("notch-media-reader".into())
                .spawn(move || {
                    for line in std::io::BufReader::new(stdout).lines() {
                        let Ok(line) = line else {
                            break;
                        };
                        if events
                            .send(Event::Report(media::parse_report(&line)))
                            .is_err()
                        {
                            return;
                        }
                    }
                    let _ = events.send(Event::Ended);
                });
            if started.is_err() {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Some(child)
        }

        fn send(command: MediaCommand) {
            const SEND: &str =
                "ObjC.bindFunction('MRMediaRemoteSendCommand', ['bool', ['int', 'id']]);";
            let call = match command {
                MediaCommand::TogglePlay => format!("{SEND}$.MRMediaRemoteSendCommand(2, $());"),
                MediaCommand::Next => format!("{SEND}$.MRMediaRemoteSendCommand(4, $());"),
                MediaCommand::Previous => format!("{SEND}$.MRMediaRemoteSendCommand(5, $());"),
                MediaCommand::Seek(to) => format!(
                    "ObjC.bindFunction('MRMediaRemoteSetElapsedTime', ['void', ['double']]);\
                     $.MRMediaRemoteSetElapsedTime({:.3});",
                    to.as_secs_f64()
                ),
                // Never sent here: this reader reports no shuffle to change.
                MediaCommand::Shuffle(_) => return,
            };
            let _ = osascript(&format!("{FRAMEWORK}{call}"))
                .stdout(Stdio::null())
                .status();
        }

        /// What is kept about the track that plays: its cover and the
        /// playhead's clock.
        #[derive(Default)]
        struct Cover {
            track: Option<Track>,
            art: Option<Arc<[u8]>>,
            playhead: Playhead,
        }

        fn playing(report: &Report, cover: &mut Cover, events: &mpsc::Sender<Event>) -> Playing {
            if cover.track.as_ref() != Some(&report.track) {
                *cover = Cover {
                    track: Some(report.track.clone()),
                    ..Cover::default()
                };
                let (track, events) = (report.track.clone(), events.clone());
                let _ = std::thread::Builder::new()
                    .name("notch-cover".into())
                    .spawn(move || {
                        // Offline or not in the catalog: the track goes without.
                        let art = media::fetch_cover(&track).ok().flatten().map(Arc::from);
                        let _ = events.send(Event::Cover(track, art));
                    });
            }
            let age = report.stamped.and_then(|stamp| {
                let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
                let age = now.as_secs_f64() - stamp;
                (age > 0. && age < STALE).then(|| Duration::from_secs_f64(age))
            });
            let now = Instant::now();
            // Where the song is: see `Playhead` for why not just the stamp.
            let position = cover
                .playhead
                .read(report.elapsed, report.playing, age, now);
            Playing {
                track: report.track.clone(),
                playing: report.playing,
                position,
                at: now,
                art: cover.art.clone(),
                // The now playing service does not say; Spotify's own
                // answer does, once signed in (`notch/queue.rs`).
                shuffle: None,
            }
        }

        pub(crate) fn watch(
            on_change: impl Fn(Option<Playing>) + Send + 'static,
        ) -> Option<mpsc::Sender<MediaCommand>> {
            let (commands, received) = mpsc::channel();
            std::thread::Builder::new()
                .name("notch-media".into())
                .spawn(move || {
                    let (events, heard) = mpsc::channel();
                    let mut reader = start_reader(&events);
                    let mut down_since = reader.is_none().then(Instant::now);
                    let mut tries = 0;
                    let mut report: Option<Report> = None;
                    let mut cover = Cover::default();
                    let mut shown: Option<Playing> = None;
                    loop {
                        match received.recv_timeout(POLL) {
                            Ok(command) => send(command),
                            Err(RecvTimeoutError::Timeout) => {}
                            Err(RecvTimeoutError::Disconnected) => break,
                        }
                        while let Ok(event) = heard.try_recv() {
                            match event {
                                Event::Report(now) => {
                                    tries = 0;
                                    report = now;
                                }
                                Event::Cover(track, art) => {
                                    if cover.track.as_ref() == Some(&track) {
                                        cover.art = art;
                                    }
                                }
                                Event::Ended => {
                                    if let Some(mut child) = reader.take() {
                                        let _ = child.wait();
                                    }
                                    down_since = Some(Instant::now());
                                    report = None;
                                }
                            }
                        }
                        if reader.is_none()
                            && tries < TRIES
                            && down_since.is_some_and(|since| since.elapsed() > RESTART)
                        {
                            tries += 1;
                            reader = start_reader(&events);
                            down_since = reader.is_none().then(Instant::now);
                        }
                        let now = report
                            .as_ref()
                            .map(|report| playing(report, &mut cover, &events));
                        if media::is_news(&shown, &now) {
                            on_change(now.clone());
                            shown = now;
                        }
                    }
                    if let Some(mut child) = reader {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                })
                .ok()?;
            Some(commands)
        }
    }

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

    /// GPUI makes a pop-up a topmost tool window that shows without taking
    /// focus. Two things more make it the notch:
    ///
    /// - `WS_EX_NOACTIVATE`: Windows never picks the notch when it passes
    ///   activation to another window of the app (minimizing the workspace,
    ///   switching with the taskbar). A click does not activate it either,
    ///   so a field asks for the keyboard itself (`take_keys`).
    /// - No Windows 11 frame: the window is a transparent room larger than
    ///   the island, and DWM would otherwise round it and draw its border and
    ///   shadow around that room.
    pub(super) fn style(window: &Window) {
        use windows::Win32::Graphics::Dwm::{
            DWMNCRP_DISABLED, DWMWA_BORDER_COLOR, DWMWA_NCRENDERING_POLICY,
            DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmSetWindowAttribute,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            GWL_EXSTYLE, GetWindowLongPtrW, SetWindowLongPtrW, WS_EX_NOACTIVATE,
        };
        let Some(hwnd) = hwnd(window) else {
            return;
        };
        // DWMWA_COLOR_NONE: no border at all.
        const COLOR_NONE: u32 = 0xFFFF_FFFE;
        // SAFETY: plain style and attribute writes on the live window, each
        // with a value of the size the attribute expects.
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_NOACTIVATE.0 as isize);
            let set = |attribute, value: &u32| {
                // Older Windows lacks some attributes; the notch still works.
                let _ = DwmSetWindowAttribute(
                    hwnd,
                    attribute,
                    std::ptr::from_ref(value).cast(),
                    std::mem::size_of::<u32>() as u32,
                );
            };
            set(
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &(DWMWCP_DONOTROUND.0 as u32),
            );
            set(DWMWA_BORDER_COLOR, &COLOR_NONE);
            set(DWMWA_NCRENDERING_POLICY, &(DWMNCRP_DISABLED.0 as u32));
        }
        stay_on_top(hwnd);
    }

    /// The window that had the keyboard when the notch took it.
    static KEYS_FROM: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

    /// `WS_EX_NOACTIVATE` keeps a click from activating the notch, so a
    /// field in it showed its caret while the keys still went to whatever
    /// app was in front (they reached the field only when that app was
    /// MonoBOT). A field asks for the keyboard outright: the notch becomes
    /// the foreground window, which Windows allows since the click that
    /// asks was its own, and remembers who had it before.
    pub(super) fn take_keys(window: &Window) {
        use std::sync::atomic::Ordering;
        use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
        use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow};
        let Some(hwnd) = hwnd(window) else {
            return;
        };
        // SAFETY: plain calls on the live window; a refusal changes nothing.
        unsafe {
            let front = GetForegroundWindow();
            if front != hwnd {
                KEYS_FROM.store(front.0 as isize, Ordering::Relaxed);
                let _ = SetForegroundWindow(hwnd);
            }
            let _ = SetFocus(Some(hwnd));
        }
    }

    /// Hands the keyboard back to the window it was taken from, when the
    /// notch still has it: the user may have clicked another one since.
    pub(super) fn give_keys_back() {
        use std::sync::atomic::Ordering;
        use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow};
        let before = KEYS_FROM.swap(0, Ordering::Relaxed);
        let notch = ON_TOP.load(Ordering::Relaxed);
        if before == 0 || notch == 0 {
            return;
        }
        // SAFETY: a stale handle just fails.
        unsafe {
            if GetForegroundWindow().0 as isize == notch {
                let _ = SetForegroundWindow(HWND(before as _));
            }
        }
    }

    /// The notch the foreground watch keeps on top.
    static ON_TOP: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

    /// Windows raises the taskbar, the Start menu and the window just
    /// clicked above every other topmost window, so the notch slipped under
    /// a taskbar docked at the top until something moved it. Each time the
    /// foreground changes it goes back on top, without taking focus.
    fn stay_on_top(hwnd: HWND) {
        use std::sync::atomic::Ordering;
        use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
        use windows::Win32::UI::WindowsAndMessaging::{
            EVENT_SYSTEM_FOREGROUND, SWP_NOMOVE, SWP_NOSIZE, WINEVENT_OUTOFCONTEXT,
        };
        unsafe extern "system" fn raise(
            _: HWINEVENTHOOK,
            _: u32,
            _: HWND,
            _: i32,
            _: i32,
            _: u32,
            _: u32,
        ) {
            let notch = ON_TOP.load(Ordering::Relaxed);
            if notch == 0 {
                return;
            }
            // SAFETY: a z-order change only; a stale handle just fails.
            let _ = unsafe {
                SetWindowPos(
                    HWND(notch as _),
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                )
            };
        }
        // The hook is made once; a new notch window only replaces the handle.
        if ON_TOP.swap(hwnd.0 as isize, Ordering::Relaxed) != 0 {
            return;
        }
        // SAFETY: out of context, the callback runs on this (the interface)
        // thread's message loop and only touches the notch's z-order.
        unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(raise),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
        }
    }

    /// The battery's charge and whether it charges; none on a desktop.
    pub(crate) fn battery() -> Option<(f32, bool)> {
        use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
        let mut status = SYSTEM_POWER_STATUS::default();
        // SAFETY: fills `status`.
        unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
        // 128: no system battery; 255: unknown.
        if status.BatteryFlag & 128 != 0 || status.BatteryLifePercent > 100 {
            return None;
        }
        Some((
            f32::from(status.BatteryLifePercent) / 100.,
            status.ACLineStatus == 1,
        ))
    }

    /// What the user copies, for the notch's Clipboard page. Windows tells
    /// a change by the clipboard's sequence number, so the thread reads the
    /// text only when something was copied, and never on the interface thread.
    pub(crate) mod clipboard {
        use windows::Win32::Foundation::HGLOBAL;
        use windows::Win32::System::DataExchange::{
            CloseClipboard, GetClipboardData, GetClipboardSequenceNumber,
            IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW,
        };
        use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
        use windows::core::w;

        use super::super::Copied;

        /// `CF_UNICODETEXT`.
        const TEXT: u32 = 13;
        /// `CF_DIB`: a picture as its header and pixels.
        const PICTURE: u32 = 8;

        /// A format's memory as its owner wrote it, copied out.
        ///
        /// SAFETY: the clipboard is open, so the handle stays alive.
        unsafe fn bytes(format: u32, largest: usize) -> Option<Vec<u8>> {
            unsafe {
                IsClipboardFormatAvailable(format).ok()?;
                let memory = HGLOBAL(GetClipboardData(format).ok()?.0);
                let size = GlobalSize(memory);
                if size == 0 || size > largest {
                    return None;
                }
                let data = GlobalLock(memory) as *const u8;
                if data.is_null() {
                    return None;
                }
                let copy = std::slice::from_raw_parts(data, size).to_vec();
                let _ = GlobalUnlock(memory);
                Some(copy)
            }
        }

        /// The copied text, or the copied picture where there is no text.
        /// `Err` while another app holds the clipboard; `None` for what is
        /// neither, is too large, or what its owner asked to keep out of
        /// histories (a password manager's copy).
        fn read(largest: usize, largest_picture: usize) -> Result<Option<Copied>, ()> {
            // SAFETY: opened and closed here; every read is bounded by the
            // memory's own size.
            unsafe {
                OpenClipboard(None).map_err(|_| ())?;
                let excluded =
                    RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
                let history = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
                let hidden = IsClipboardFormatAvailable(excluded).is_ok()
                    || bytes(history, 16).is_some_and(|value| value.iter().all(|byte| *byte == 0));
                let has_text = IsClipboardFormatAvailable(TEXT).is_ok();
                let text = if hidden {
                    None
                } else {
                    bytes(TEXT, largest * 2 + 2).map(|data| {
                        let units: Vec<u16> = data
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|pair| u16::from_le_bytes(*pair))
                            .take_while(|unit| *unit != 0)
                            .collect();
                        Copied::Text(String::from_utf16_lossy(&units))
                    })
                };
                // A spreadsheet's cells come as text and as a picture of
                // them: the text is what was copied.
                let picture = if hidden || has_text {
                    None
                } else {
                    bytes(PICTURE, largest_picture)
                };
                let _ = CloseClipboard();
                // Made into a file here, off the interface thread.
                Ok(text.or_else(|| {
                    let (bmp, width, height) =
                        monocode_core::notch::clipboard::bmp_from_dib(&picture?)?;
                    Some(Copied::Picture(bmp, width, height))
                }))
            }
        }

        /// Sends each new copy until the receiver is gone.
        pub(crate) fn watch(
            copies: async_channel::Sender<Copied>,
            largest: usize,
            largest_picture: usize,
        ) -> bool {
            std::thread::Builder::new()
                .name("notch-clipboard".into())
                .spawn(move || {
                    // SAFETY: a plain read of a counter.
                    let mut seen = unsafe { GetClipboardSequenceNumber() };
                    while !copies.is_closed() {
                        std::thread::sleep(std::time::Duration::from_millis(400));
                        // SAFETY: as above.
                        let now = unsafe { GetClipboardSequenceNumber() };
                        if now == seen {
                            continue;
                        }
                        // Busy: the next round reads it.
                        if let Ok(copy) = read(largest, largest_picture) {
                            seen = now;
                            if let Some(copy) = copy {
                                let _ = copies.try_send(copy);
                            }
                        }
                    }
                })
                .is_ok()
        }
    }

    /// What plays, from the system's media controls: the same session the
    /// Windows media flyout shows (Spotify, a browser's video, the media
    /// player). Read on a thread of its own every half second; WinRT's calls
    /// block there, never on the interface thread.
    pub(crate) mod media {
        use super::super::MediaCommand;
        use monocode_core::notch::media::{Playhead, Playing, Track};
        use std::sync::Arc;
        use std::sync::mpsc::{self, RecvTimeoutError};
        use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
        use windows::Media::Control::{
            GlobalSystemMediaTransportControlsSession as Session,
            GlobalSystemMediaTransportControlsSessionManager as Manager,
            GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
        };
        use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};

        const POLL: Duration = Duration::from_millis(500);
        /// Windows counts 100 ns ticks from 1601; Unix seconds from 1970.
        const UNIX_IN_TICKS: i64 = 116_444_736_000_000_000;
        /// A position older than this is a stale stamp, not playback.
        const STALE: i64 = 10 * 60 * 10_000_000;

        fn span(ticks: i64) -> Duration {
            Duration::from_nanos(ticks.max(0) as u64 * 100)
        }

        fn now_ticks() -> i64 {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| (since.as_nanos() / 100) as i64)
                + UNIX_IN_TICKS
        }

        /// The cover, as the app's bytes (PNG or JPEG). Spotify and browsers
        /// hand it over a moment after the title, so it is asked again until
        /// it comes.
        fn art(
            properties: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties,
        ) -> Option<Arc<[u8]>> {
            use windows::Storage::Streams::{DataReader, IInputStream};
            use windows::core::Interface;
            let stream = properties
                .Thumbnail()
                .ok()?
                .OpenReadAsync()
                .ok()?
                .join()
                .ok()?;
            let size = u32::try_from(stream.Size().ok()?).ok()?;
            // Nothing, or something no cover is: a few megabytes at most.
            if size == 0 || size > 8 << 20 {
                return None;
            }
            let reader = DataReader::CreateDataReader(&stream.cast::<IInputStream>().ok()?).ok()?;
            reader.LoadAsync(size).ok()?.join().ok()?;
            let mut bytes = vec![0; size as usize];
            reader.ReadBytes(&mut bytes).ok()?;
            Some(bytes.into())
        }

        /// What is kept about the track that plays: its cover, how often the
        /// cover was asked for, and the playhead's clock.
        #[derive(Default)]
        struct Cover {
            track: Option<Track>,
            art: Option<Arc<[u8]>>,
            tries: u32,
            playhead: Playhead,
        }

        fn read(session: &Session, cover: &mut Cover) -> windows::core::Result<Option<Playing>> {
            let properties = session.TryGetMediaPropertiesAsync()?.join()?;
            let timeline = session.GetTimelineProperties()?;
            let info = session.GetPlaybackInfo()?;
            let playing = info.PlaybackStatus()? == Status::Playing;
            // Shuffle as the app reports it, only where it also lets the
            // system change it: Spotify does in a playlist or an album.
            let shuffle = info
                .Controls()
                .and_then(|controls| controls.IsShuffleEnabled())
                .unwrap_or(false)
                .then(|| {
                    info.IsShuffleActive()
                        .and_then(|active| active.Value())
                        .ok()
                })
                .flatten();
            let start = span(timeline.StartTime()?.Duration);
            let end = span(timeline.EndTime()?.Duration);
            let raw = span(timeline.Position()?.Duration);
            let since = now_ticks() - timeline.LastUpdatedTime()?.UniversalTime;
            let track = Track {
                app: session.SourceAppUserModelId()?.to_string(),
                title: properties.Title()?.to_string(),
                artist: properties.Artist()?.to_string(),
                album: properties.AlbumTitle()?.to_string(),
                duration: (end > start).then(|| end - start),
            };
            if !track.is_known() {
                return Ok(None);
            }
            if cover.track.as_ref() != Some(&track) {
                *cover = Cover {
                    track: Some(track.clone()),
                    ..Cover::default()
                };
            }
            // A few tries, a poll apart, then the track goes without.
            if cover.art.is_none() && cover.tries < 6 {
                cover.tries += 1;
                cover.art = art(&properties);
            }
            // Where the song is: see `Playhead` for why not just the stamp.
            let now = Instant::now();
            let position = cover.playhead.read(
                raw,
                playing,
                (since > 0 && since < STALE).then(|| span(since)),
                now,
            );
            if super::super::super::tracing() {
                eprintln!(
                    "NOTCHTRACE timeline playing={playing} raw={:.3} stamp_age={:.3} playhead={:.3}",
                    raw.as_secs_f32(),
                    since as f64 / 1e7,
                    position.saturating_sub(start).as_secs_f32()
                );
            }
            Ok(Some(Playing {
                track,
                playing,
                position: position.saturating_sub(start),
                at: now,
                art: cover.art.clone(),
                shuffle,
            }))
        }

        fn send(session: &Session, command: MediaCommand) -> windows::core::Result<bool> {
            match command {
                MediaCommand::TogglePlay => session.TryTogglePlayPauseAsync()?.join(),
                MediaCommand::Next => session.TrySkipNextAsync()?.join(),
                MediaCommand::Previous => session.TrySkipPreviousAsync()?.join(),
                MediaCommand::Seek(to) => {
                    let start = session.GetTimelineProperties()?.StartTime()?.Duration;
                    let ticks = start + (to.as_nanos() / 100) as i64;
                    session.TryChangePlaybackPositionAsync(ticks)?.join()
                }
                MediaCommand::Shuffle(on) => session.TryChangeShuffleActiveAsync(on)?.join(),
            }
        }

        pub(crate) fn watch(
            on_change: impl Fn(Option<Playing>) + Send + 'static,
        ) -> Option<mpsc::Sender<MediaCommand>> {
            let (commands, received) = mpsc::channel();
            std::thread::Builder::new()
                .name("notch-media".into())
                .spawn(move || {
                    // SAFETY: this thread's own WinRT apartment.
                    let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
                    let Ok(manager) = Manager::RequestAsync().and_then(|op| op.join()) else {
                        return;
                    };
                    let mut shown: Option<Playing> = None;
                    let mut cover = Cover::default();
                    loop {
                        match received.recv_timeout(POLL) {
                            Ok(command) => {
                                if let Ok(session) = manager.GetCurrentSession() {
                                    let _ = send(&session, command);
                                }
                            }
                            Err(RecvTimeoutError::Timeout) => {}
                            Err(RecvTimeoutError::Disconnected) => return,
                        }
                        let now = manager
                            .GetCurrentSession()
                            .ok()
                            .and_then(|session| read(&session, &mut cover).ok().flatten());
                        if monocode_core::notch::media::is_news(&shown, &now) {
                            on_change(now.clone());
                            shown = now;
                        }
                    }
                })
                .ok()?;
            Some(commands)
        }
    }

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
                power: super::battery(),
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

    /// How loud the default output sounds now, for the equalizer: the
    /// endpoint's peak meter, read about thirty times a second on a thread
    /// of its own that lives only while the bars are asked for.
    pub(crate) mod meter {
        use monocode_core::notch::media::{WAVE, Wave};
        use std::sync::OnceLock;
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        use std::time::{Duration, Instant};
        use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
        use windows::Win32::Media::Audio::{
            IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender,
        };
        use windows::Win32::System::Com::{
            CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        };

        static RUNNING: AtomicBool = AtomicBool::new(false);
        /// When the bars were last asked for, in milliseconds on `clock`.
        static ASKED: AtomicU64 = AtomicU64::new(0);
        /// The waveform's bars, seven bits each; `NONE` while there are none.
        static BARS: AtomicU64 = AtomicU64::new(NONE);
        const STEPS: f32 = 127.;
        const NONE: u64 = u64::MAX;
        /// The thread ends this long after the last ask.
        const IDLE: u64 = 2_000;

        fn clock() -> u64 {
            static START: OnceLock<Instant> = OnceLock::new();
            START.get_or_init(Instant::now).elapsed().as_millis() as u64
        }

        fn open() -> Option<IAudioMeterInformation> {
            // SAFETY: COM for this thread, then the default render endpoint.
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                let devices: IMMDeviceEnumerator =
                    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
                let device = devices.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
                device
                    .Activate::<IAudioMeterInformation>(CLSCTX_ALL, None)
                    .ok()
            }
        }

        fn run() {
            let mut wave = Wave::default();
            let mut endpoint = open();
            let mut opened = Instant::now();
            while clock().saturating_sub(ASKED.load(Ordering::Relaxed)) < IDLE {
                // The output may change (headphones): look again now and then.
                if opened.elapsed() > Duration::from_secs(5) {
                    endpoint = open();
                    opened = Instant::now();
                }
                // SAFETY: a plain read on a live endpoint.
                let peak = endpoint
                    .as_ref()
                    .and_then(|endpoint| unsafe { endpoint.GetPeakValue().ok() });
                let packed = match peak {
                    Some(peak) => wave.feed(peak).iter().fold(0u64, |packed, bar| {
                        (packed << 7) | (bar.clamp(0., 1.) * STEPS) as u64
                    }),
                    None => NONE,
                };
                BARS.store(packed, Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(33));
            }
            BARS.store(NONE, Ordering::Relaxed);
            RUNNING.store(false, Ordering::Release);
        }

        /// The bars now, oldest to newest; none until the first reading, or
        /// where the output has no meter.
        pub(crate) fn bars() -> Option<[f32; WAVE]> {
            ASKED.store(clock(), Ordering::Relaxed);
            if !RUNNING.swap(true, Ordering::AcqRel)
                && std::thread::Builder::new()
                    .name("notch-meter".into())
                    .spawn(run)
                    .is_err()
            {
                RUNNING.store(false, Ordering::Release);
            }
            let packed = BARS.load(Ordering::Relaxed);
            (packed != NONE).then(|| {
                std::array::from_fn(|index| {
                    ((packed >> (7 * (WAVE - 1 - index))) & 0x7f) as f32 / STEPS
                })
            })
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

    /// Moves and sizes the window at once. It is called while rendering, so
    /// the `WM_SIZE` this sends cannot reach GPUI (the window is mid-update):
    /// tell GPUI the new bounds here, or it keeps laying out, painting and
    /// hit-testing in the old size, as `mac::place` does.
    pub(super) fn place(window: &mut Window, cx: &mut App, display: &Display, frame: Frame) {
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
        window.bounds_changed(cx);
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

/// Gives the notch the keyboard, for a field in it that was clicked or a
/// chat that was opened to write in. Only Windows needs asking: its notch
/// never activates by a click. The Mac's panel takes the keys of a click by
/// itself, without bringing the app forward.
pub(crate) fn take_keys(window: &Window) {
    #[cfg(all(target_os = "windows", not(test)))]
    win::take_keys(window);
    #[cfg(not(all(target_os = "windows", not(test))))]
    let _ = window;
}

/// Gives the keyboard back to the window `take_keys` took it from, when the
/// panel closes.
pub(crate) fn give_keys_back() {
    #[cfg(all(target_os = "windows", not(test)))]
    win::give_keys_back();
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

/// What the notch asks of the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MediaCommand {
    TogglePlay,
    Next,
    Previous,
    /// Move the playhead to this point of the track.
    Seek(std::time::Duration),
    /// Turn shuffle on or off, where the system's controls can.
    Shuffle(bool),
}

/// The battery's charge (0 to 1) and whether it charges; none where there
/// is no battery or the system does not say.
pub(crate) fn battery() -> Option<(f32, bool)> {
    #[cfg(all(target_os = "windows", not(test)))]
    return win::battery();
    #[cfg(all(target_os = "macos", not(test)))]
    return monocode_core::notch::system::battery();
    #[cfg(any(test, not(any(target_os = "macos", target_os = "windows"))))]
    None
}

/// Whether this system's clipboard can be watched for the Clipboard page.
pub(crate) const CLIPBOARD: bool = cfg!(any(target_os = "windows", target_os = "macos", test));

/// The file a copied picture is kept as: what the system's clipboard holds
/// (PNG on macOS; on Windows a BMP made from its bitmap). Tests copy BMPs.
pub(crate) const PICTURE: gpui::ImageFormat = if cfg!(all(target_os = "macos", not(test))) {
    gpui::ImageFormat::Png
} else {
    gpui::ImageFormat::Bmp
};

/// Something the user copied. Made only by a system's watcher, and tests
/// and the systems without one have none.
#[cfg_attr(
    any(test, not(any(target_os = "windows", target_os = "macos"))),
    allow(dead_code)
)]
pub(crate) enum Copied {
    Text(String),
    /// A picture as a file of the `PICTURE` kind, with its width and
    /// height in pixels.
    Picture(Vec<u8>, u32, u32),
}

/// Watches the clipboard off the interface thread and sends each new copy
/// (text up to `largest` bytes, or a picture up to `largest_picture` where
/// there is no text), until the receiver is dropped.
pub(crate) fn watch_clipboard(
    copies: async_channel::Sender<Copied>,
    largest: usize,
    largest_picture: usize,
) -> bool {
    #[cfg(all(target_os = "windows", not(test)))]
    return win::clipboard::watch(copies, largest, largest_picture);
    #[cfg(all(target_os = "macos", not(test)))]
    return mac::clipboard::watch(copies, largest, largest_picture);
    #[cfg(any(test, not(any(target_os = "macos", target_os = "windows"))))]
    {
        let _ = (copies, largest, largest_picture);
        false
    }
}

/// The system pointer's x on the screen, in physical pixels, for the
/// face's glance.
pub(crate) fn pointer_x() -> Option<f32> {
    #[cfg(all(target_os = "macos", not(test)))]
    return mac::pointer_x();
    #[cfg(all(target_os = "windows", not(test)))]
    {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut point = POINT::default();
        // SAFETY: writes the cursor position into `point`.
        unsafe { GetCursorPos(&mut point) }.ok()?;
        Some(point.x as f32)
    }
    #[cfg(any(test, not(any(target_os = "macos", target_os = "windows"))))]
    None
}

/// Watches what plays and reports each change (another track, play or
/// pause, a seek); the sender carries the notch's commands to the player.
pub(crate) fn watch_media(
    on_change: impl Fn(Option<monocode_core::notch::media::Playing>) + Send + 'static,
) -> Option<std::sync::mpsc::Sender<MediaCommand>> {
    #[cfg(all(target_os = "windows", not(test)))]
    return win::media::watch(on_change);
    #[cfg(all(target_os = "macos", not(test)))]
    return mac::media::watch(on_change);
    #[cfg(any(test, not(any(target_os = "macos", target_os = "windows"))))]
    {
        let _ = on_change;
        None
    }
}

/// Whether the system pointer is inside `area`, a rectangle in physical
/// screen pixels, where the app asks the system rather than a watch (Windows).
/// GPUI's hover goes stale when the notch's window resizes under a still
/// pointer, so the decision to open or close asks the system instead.
pub(crate) fn pointer_in(area: Option<[f32; 4]>) -> Option<bool> {
    #[cfg(all(target_os = "windows", not(test)))]
    {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let [left, top, width, height] = area?;
        let mut point = POINT::default();
        // SAFETY: writes the cursor position into `point`.
        unsafe { GetCursorPos(&mut point) }.ok()?;
        let (x, y) = (point.x as f32, point.y as f32);
        Some(x >= left && x < left + width && y >= top && y < top + height)
    }
    #[cfg(not(all(target_os = "windows", not(test))))]
    {
        let _ = area;
        None
    }
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

/// The waveform of what the system plays now, oldest bar first, each 0 to
/// 1, where the system has a meter for its output (Windows). Asking keeps
/// the reading going; it stops by itself a moment after the last ask.
pub(crate) fn audio_wave() -> Option<[f32; monocode_core::notch::media::WAVE]> {
    #[cfg(all(target_os = "windows", not(test)))]
    return win::meter::bars();
    #[cfg(not(all(target_os = "windows", not(test))))]
    None
}

/// The system's reduced motion setting, where the system says it.
pub(crate) fn reduce_motion() -> Option<bool> {
    #[cfg(all(target_os = "macos", not(test)))]
    return mac::reduce_motion();
    #[cfg(not(all(target_os = "macos", not(test))))]
    None
}
