//! The global dictation shortcut's system side: a keyboard hook that sees
//! the shortcut go down and come up in any app and keeps it (and Escape,
//! while a system dictation listens) from the app in front, and the paste of
//! the dictated text into that app. Windows: a low-level keyboard hook on a
//! thread of its own, `SendInput`. macOS: a session event tap, which needs
//! the Accessibility permission the notch already asks for, and
//! `CGEventPost`. The keys this app sends carry a marker, and the hooks let
//! them through. Which events are the shortcut's is decided in
//! `monocode_core::dictation::keycodes::Filter`.
use monocode_core::dictation::{
    hotkey::{KeyEvent, Shortcut},
    keycodes::{Filter, Verdict, Watch},
};
use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;

/// One of the shortcut's events, timed where the hook saw it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hit {
    pub event: KeyEvent,
    pub at: Instant,
}

/// Tags the keys this app sends ("mono"), so its own hook passes them.
#[cfg_attr(
    any(test, not(any(target_os = "macos", target_os = "windows"))),
    allow(dead_code)
)]
const MARKER: usize = 0x6D6F_6E6F;

static FILTER: Mutex<Filter> = Mutex::new(Filter::new());
/// A system dictation listens: Escape is taken to cancel it.
static LISTENING: AtomicBool = AtomicBool::new(false);
static HITS: OnceLock<async_channel::Sender<Hit>> = OnceLock::new();

/// Runs the filter on one key event and reports what it found. Called on the
/// hook's own thread (Windows' low-level hook, macOS's event tap); it only
/// locks a small mutex and sends on an unbounded channel.
#[cfg_attr(
    any(test, not(any(target_os = "macos", target_os = "windows"))),
    allow(dead_code)
)]
fn decide(code: u16, down: bool, held: monocode_core::dictation::hotkey::Modifiers) -> Verdict {
    let verdict = FILTER.lock().unwrap_or_else(|e| e.into_inner()).key(
        code,
        down,
        held,
        LISTENING.load(Ordering::Relaxed),
    );
    if let Some(event) = verdict.report
        && let Some(hits) = HITS.get()
    {
        let _ = hits.try_send(Hit {
            event,
            at: Instant::now(),
        });
    }
    verdict
}

/// Watches for `shortcut` in every app, or for nothing. False when this
/// system has no code for its key.
pub(crate) fn watch(shortcut: Option<&Shortcut>) -> bool {
    let watch = shortcut.and_then(|shortcut| {
        Some(Watch {
            key: code(&shortcut.key)?,
            modifiers: shortcut.modifiers,
            escape: ESCAPE,
        })
    });
    let known = shortcut.is_none() || watch.is_some();
    FILTER.lock().unwrap_or_else(|e| e.into_inner()).set(watch);
    known
}

/// A system dictation started or stopped listening.
pub(crate) fn set_listening(on: bool) {
    LISTENING.store(on, Ordering::Relaxed);
}

#[cfg(not(target_os = "macos"))]
const ESCAPE: u16 = monocode_core::dictation::keycodes::WINDOWS_ESCAPE;
#[cfg(target_os = "macos")]
const ESCAPE: u16 = monocode_core::dictation::keycodes::MAC_ESCAPE;

fn code(key: &str) -> Option<u16> {
    #[cfg(target_os = "macos")]
    return monocode_core::dictation::keycodes::mac_keycode(key);
    #[cfg(not(target_os = "macos"))]
    monocode_core::dictation::keycodes::windows_vk(key)
}

/// Starts the hook once, reporting the shortcut's events to `hits`. False
/// while the system does not allow it yet (macOS without Accessibility), so
/// the caller tries again later; tests and Linux never hook.
pub(crate) fn start(hits: async_channel::Sender<Hit>) -> bool {
    let _ = HITS.set(hits);
    #[cfg(all(target_os = "windows", not(test)))]
    return win::start();
    #[cfg(all(target_os = "macos", not(test)))]
    return mac::start();
    #[cfg(any(test, not(any(target_os = "macos", target_os = "windows"))))]
    false
}

/// Whether a window of this app is the one in front, where the system says
/// it itself (Windows: the foreground window's process). `None`: ask GPUI.
pub(crate) fn frontmost_is_ours() -> Option<bool> {
    #[cfg(all(target_os = "windows", not(test)))]
    return Some(win::frontmost_is_ours());
    #[cfg(not(all(target_os = "windows", not(test))))]
    None
}

/// Whether the app in front can take a paste: a focused window that is not
/// the desktop or the taskbar (Windows), or the permission to send keys at
/// all (macOS). Without one the text is only copied.
pub(crate) fn has_paste_target() -> bool {
    #[cfg(all(target_os = "windows", not(test)))]
    return win::has_paste_target();
    #[cfg(all(target_os = "macos", not(test)))]
    return mac::trusted();
    #[cfg(any(test, not(any(target_os = "macos", target_os = "windows"))))]
    false
}

/// Sends the paste chord (Ctrl+V, ⌘V) to the app in front, once the
/// modifiers the user held for the shortcut are up. Blocks for up to a
/// second and a half: run it off the interface thread.
pub(crate) fn send_paste() -> Result<(), String> {
    #[cfg(all(target_os = "windows", not(test)))]
    return win::send_paste();
    #[cfg(all(target_os = "macos", not(test)))]
    return mac::send_paste();
    #[cfg(any(test, not(any(target_os = "macos", target_os = "windows"))))]
    Err("Pasting is not available here.".into())
}

/// How long the paste waits for the modifiers to come up.
#[cfg_attr(
    any(test, not(any(target_os = "macos", target_os = "windows"))),
    allow(dead_code)
)]
const MODIFIERS_UP: std::time::Duration = std::time::Duration::from_millis(1_500);

/// Waits until `held` reports no modifier down, polling every 10 ms, for
/// at most [`MODIFIERS_UP`]. Whether they came up.
#[cfg_attr(
    any(test, not(any(target_os = "macos", target_os = "windows"))),
    allow(dead_code)
)]
fn wait_for_modifiers(held: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    while held() {
        if start.elapsed() > MODIFIERS_UP {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    true
}

#[cfg(all(target_os = "windows", not(test)))]
mod win {
    use super::{MARKER, decide, wait_for_modifiers};
    use monocode_core::dictation::hotkey::Modifiers;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
        KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
        VK_V,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GUITHREADINFO, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo,
        GetMessageW, GetWindowThreadProcessId, HC_ACTION, KBDLLHOOKSTRUCT, MSG, SetWindowsHookExW,
        WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    /// An unassigned virtual key: sent after taking an Alt or Windows-key
    /// chord, so that modifier coming up alone does not open the menu bar
    /// or Start.
    const MASK_KEY: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);

    fn down(key: VIRTUAL_KEY) -> bool {
        // SAFETY: a plain query.
        unsafe { GetAsyncKeyState(i32::from(key.0)) as u16 & 0x8000 != 0 }
    }

    fn held() -> Modifiers {
        Modifiers {
            ctrl: down(VK_CONTROL),
            alt: down(VK_MENU),
            shift: down(VK_SHIFT),
            meta: down(VK_LWIN) || down(VK_RWIN),
        }
    }

    fn key(code: VIRTUAL_KEY, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: code,
                    wScan: 0,
                    dwFlags: if up {
                        KEYEVENTF_KEYUP
                    } else {
                        KEYBD_EVENT_FLAGS(0)
                    },
                    time: 0,
                    dwExtraInfo: MARKER,
                },
            },
        }
    }

    fn send(inputs: &[INPUT]) -> bool {
        // SAFETY: a slice of well-formed keyboard inputs.
        let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
        sent as usize == inputs.len()
    }

    unsafe extern "system" fn on_key(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 {
            // SAFETY: for WH_KEYBOARD_LL, lparam points at a KBDLLHOOKSTRUCT.
            let event = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
            let message = wparam.0 as u32;
            let is_down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
            let is_up = message == WM_KEYUP || message == WM_SYSKEYUP;
            if event.dwExtraInfo != MARKER && (is_down || is_up) {
                let verdict = decide(event.vkCode as u16, is_down, held());
                if verdict.mask_modifier {
                    send(&[key(MASK_KEY, false), key(MASK_KEY, true)]);
                }
                if verdict.swallow {
                    return LRESULT(1);
                }
            }
        }
        // SAFETY: every other key goes on to the next hook.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    /// The hook's thread, started once; it lives as long as the app.
    pub(super) fn start() -> bool {
        static STARTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *STARTED.get_or_init(|| {
            let (ready, hooked) = std::sync::mpsc::channel();
            let spawned = std::thread::Builder::new()
                .name("dictation-keys".into())
                .spawn(move || {
                    // SAFETY: the hook runs on this thread's message loop.
                    unsafe {
                        let hooked =
                            SetWindowsHookExW(WH_KEYBOARD_LL, Some(on_key), None, 0).is_ok();
                        let _ = ready.send(hooked);
                        if !hooked {
                            return;
                        }
                        let mut message = MSG::default();
                        while GetMessageW(&mut message, None, 0, 0).as_bool() {}
                    }
                })
                .is_ok();
            spawned && hooked.recv().unwrap_or(false)
        })
    }

    fn process(window: HWND) -> (u32, u32) {
        let mut pid = 0u32;
        // SAFETY: a plain query; a stale handle gives zero.
        let thread = unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
        (thread, pid)
    }

    pub(super) fn frontmost_is_ours() -> bool {
        // SAFETY: a plain query.
        let front = unsafe { GetForegroundWindow() };
        !front.is_invalid() && process(front).1 == std::process::id()
    }

    pub(super) fn has_paste_target() -> bool {
        // SAFETY: plain queries on the window in front.
        unsafe {
            let front = GetForegroundWindow();
            if front.is_invalid() {
                return false;
            }
            let mut class = [0u16; 64];
            let length = GetClassNameW(front, &mut class).max(0) as usize;
            let class = String::from_utf16_lossy(&class[..length]);
            // The desktop and the taskbar take no text.
            if matches!(
                class.as_str(),
                "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
            ) {
                return false;
            }
            let (thread, _) = process(front);
            let mut info = GUITHREADINFO {
                cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };
            GetGUIThreadInfo(thread, &mut info).is_ok() && !info.hwndFocus.is_invalid()
        }
    }

    pub(super) fn send_paste() -> Result<(), String> {
        let any = || {
            let held = held();
            held.ctrl || held.alt || held.shift || held.meta
        };
        if !wait_for_modifiers(any) {
            return Err("A modifier key is still held.".into());
        }
        if send(&[
            key(VK_CONTROL, false),
            key(VK_V, false),
            key(VK_V, true),
            key(VK_CONTROL, true),
        ]) {
            Ok(())
        } else {
            Err("Windows did not take the paste.".into())
        }
    }
}

#[cfg(all(target_os = "macos", not(test)))]
mod mac {
    use super::{MARKER, decide, wait_for_modifiers};
    use monocode_core::dictation::hotkey::Modifiers;
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicUsize, Ordering};

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
        fn CGEventGetIntegerValueField(event: Ref, field: u32) -> i64;
        fn CGEventSetIntegerValueField(event: Ref, field: u32, value: i64);
        fn CGEventGetFlags(event: Ref) -> u64;
        fn CGEventSetFlags(event: Ref, flags: u64);
        fn CGEventCreateKeyboardEvent(source: Ref, key: u16, down: bool) -> Ref;
        fn CGEventPost(tap: u32, event: Ref);
        fn CGEventSourceCreate(state: i32) -> Ref;
        fn CGEventSourceFlagsState(state: i32) -> u64;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFMachPortCreateRunLoopSource(allocator: Ref, port: Ref, order: isize) -> Ref;
        fn CFRunLoopGetCurrent() -> Ref;
        fn CFRunLoopRun();
        fn CFRunLoopAddSource(run_loop: Ref, source: Ref, mode: Ref);
        fn CFRelease(object: Ref);
        static kCFRunLoopCommonModes: Ref;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }

    /// kCGSessionEventTap, kCGHIDEventTap, kCGHeadInsertEventTap,
    /// kCGEventTapOptionDefault (an active tap: it may drop events).
    const SESSION_TAP: u32 = 1;
    const HID_TAP: u32 = 0;
    const HEAD_INSERT: u32 = 0;
    const ACTIVE: u32 = 0;
    /// kCGEventKeyDown, kCGEventKeyUp.
    const KEY_DOWN: u32 = 10;
    const KEY_UP: u32 = 11;
    /// The tap's own events: switched off by a slow callback or the user.
    const DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
    const DISABLED_BY_USER: u32 = 0xFFFF_FFFF;
    /// kCGKeyboardEventKeycode, kCGEventSourceUserData.
    const KEYCODE: u32 = 9;
    const USER_DATA: u32 = 42;
    /// kCGEventFlagMask{Shift, Control, Alternate, Command}.
    const SHIFT: u64 = 0x0002_0000;
    const CONTROL: u64 = 0x0004_0000;
    const OPTION: u64 = 0x0008_0000;
    const COMMAND: u64 = 0x0010_0000;
    /// kCGEventSourceStateHIDSystemState.
    const HID_STATE: i32 = 1;

    static PORT: AtomicUsize = AtomicUsize::new(0);

    pub(super) fn trusted() -> bool {
        // SAFETY: a plain query.
        unsafe { AXIsProcessTrusted() }
    }

    fn modifiers(flags: u64) -> Modifiers {
        Modifiers {
            ctrl: flags & CONTROL != 0,
            alt: flags & OPTION != 0,
            shift: flags & SHIFT != 0,
            meta: flags & COMMAND != 0,
        }
    }

    extern "C" fn on_event(proxy: Ref, kind: u32, event: Ref, user: Ref) -> Ref {
        // A panic must not cross into CoreGraphics (it would abort the app):
        // the key then goes on to the app in front.
        std::panic::catch_unwind(|| handle(proxy, kind, event, user)).unwrap_or(event)
    }

    fn handle(_proxy: Ref, kind: u32, event: Ref, _user: Ref) -> Ref {
        if kind == DISABLED_BY_TIMEOUT || kind == DISABLED_BY_USER {
            let port = PORT.load(Ordering::Relaxed);
            if port != 0 {
                // SAFETY: the port this tap was created with.
                unsafe { CGEventTapEnable(port as Ref, true) };
            }
            return event;
        }
        if kind != KEY_DOWN && kind != KEY_UP {
            return event;
        }
        // SAFETY: reads of the tap's live event.
        let (code, flags, ours) = unsafe {
            (
                CGEventGetIntegerValueField(event, KEYCODE) as u16,
                CGEventGetFlags(event),
                CGEventGetIntegerValueField(event, USER_DATA) == MARKER as i64,
            )
        };
        if ours {
            return event;
        }
        let verdict = decide(code, kind == KEY_DOWN, modifiers(flags));
        if verdict.swallow {
            std::ptr::null_mut()
        } else {
            event
        }
    }

    /// Makes the tap once Accessibility allows it; false until then. The
    /// tap sees every key typed in the session, so it runs on a thread and
    /// run loop of its own: a busy interface thread never slows typing in
    /// other apps.
    pub(super) fn start() -> bool {
        if PORT.load(Ordering::Relaxed) != 0 {
            return true;
        }
        if !trusted() {
            return false;
        }
        let (ready, made) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("dictation-keys".into())
            .spawn(move || {
                // SAFETY: a session tap on key down and up, added to this
                // thread's run loop; the callback reads only static state.
                unsafe {
                    let port = CGEventTapCreate(
                        SESSION_TAP,
                        HEAD_INSERT,
                        ACTIVE,
                        (1 << KEY_DOWN) | (1 << KEY_UP),
                        on_event,
                        std::ptr::null_mut(),
                    );
                    if port.is_null() {
                        let _ = ready.send(false);
                        return;
                    }
                    let source = CFMachPortCreateRunLoopSource(std::ptr::null_mut(), port, 0);
                    if source.is_null() {
                        CFRelease(port);
                        let _ = ready.send(false);
                        return;
                    }
                    CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
                    CFRelease(source);
                    CGEventTapEnable(port, true);
                    PORT.store(port as usize, Ordering::Relaxed);
                    let _ = ready.send(true);
                    CFRunLoopRun();
                }
            })
            .is_ok();
        spawned && made.recv().unwrap_or(false)
    }

    pub(super) fn send_paste() -> Result<(), String> {
        if !trusted() {
            return Err("monoBOT may not send keys (Accessibility is off).".into());
        }
        // SAFETY: a plain query.
        let any = || unsafe {
            CGEventSourceFlagsState(HID_STATE) & (SHIFT | CONTROL | OPTION | COMMAND) != 0
        };
        if !wait_for_modifiers(any) {
            return Err("A modifier key is still held.".into());
        }
        // SAFETY: events made, posted and released here; a null source is
        // allowed.
        unsafe {
            let source = CGEventSourceCreate(HID_STATE);
            for down in [true, false] {
                let event = CGEventCreateKeyboardEvent(
                    source,
                    monocode_core::dictation::keycodes::MAC_V,
                    down,
                );
                if event.is_null() {
                    if !source.is_null() {
                        CFRelease(source);
                    }
                    return Err("macOS did not take the paste.".into());
                }
                CGEventSetFlags(event, COMMAND);
                CGEventSetIntegerValueField(event, USER_DATA, MARKER as i64);
                CGEventPost(HID_TAP, event);
                CFRelease(event);
            }
            if !source.is_null() {
                CFRelease(source);
            }
        }
        Ok(())
    }
}
