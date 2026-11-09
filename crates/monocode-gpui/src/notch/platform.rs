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

/// Whether the window keeps one fixed room and the notch moves inside it.
/// On macOS resizing a transparent panel stalls its frames for up to a
/// second (AppKit briefly reports it occluded), so it never resizes there;
/// the pointer watch lets clicks through outside the notch.
pub(crate) const FIXED_ROOM: bool = cfg!(all(target_os = "macos", not(test)));

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

/// The system's reduced motion setting, where the system says it.
pub(crate) fn reduce_motion() -> Option<bool> {
    #[cfg(all(target_os = "macos", not(test)))]
    return mac::reduce_motion();
    #[cfg(not(all(target_os = "macos", not(test))))]
    None
}
