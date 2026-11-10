//! The quick composer's global shortcut on Windows: `RegisterHotKey` on a
//! message-only window, which Windows answers with `WM_HOTKEY` from any app.
//! Same interface as the macOS `hotkey.rs`.
use std::cell::RefCell;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
    UnregisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_MESSAGE, RegisterClassW, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_HOTKEY, WNDCLASSW,
};
use windows::core::w;

/// The shortcut's id within its window.
const ID: i32 = 1;

thread_local! {
    static PRESSED: RefCell<Option<async_channel::Sender<()>>> = const { RefCell::new(None) };
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_HOTKEY && wparam.0 == ID as usize {
        PRESSED.with(|pressed| {
            if let Some(pressed) = pressed.borrow().as_ref() {
                let _ = pressed.try_send(());
            }
        });
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

pub(super) struct Registration {
    window: HWND,
}

impl Registration {
    pub fn new(shortcut: &str) -> Result<(Self, async_channel::Receiver<()>), String> {
        let (modifiers, key) = parse(shortcut)?;
        let failed = || "Could not register Quick composer.".to_owned();
        // GPUI's main thread pumps this window's messages.
        let window = unsafe {
            let instance = GetModuleHandleW(None).map_err(|_| failed())?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: w!("MonoBOTQuickComposer"),
                ..Default::default()
            };
            RegisterClassW(&class);
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("MonoBOTQuickComposer"),
                w!("monoBOT quick composer"),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance.into()),
                None,
            )
            .map_err(|_| failed())?
        };
        if unsafe { RegisterHotKey(Some(window), ID, modifiers | MOD_NOREPEAT, key) }.is_err() {
            unsafe {
                let _ = DestroyWindow(window);
            }
            return Err("This shortcut is already in use. Choose another in Keybindings.".into());
        }
        let (tx, rx) = async_channel::bounded(1);
        PRESSED.with(|slot| *slot.borrow_mut() = Some(tx));
        Ok((Self { window }, rx))
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        unsafe {
            let _ = UnregisterHotKey(Some(self.window), ID);
            let _ = DestroyWindow(self.window);
        }
        PRESSED.with(|slot| *slot.borrow_mut() = None);
    }
}

/// A shortcut such as `ctrl-shift-space` as Windows modifiers and a
/// virtual-key code. `cmd` is Ctrl here, as GPUI reads it on Windows.
fn parse(shortcut: &str) -> Result<(HOT_KEY_MODIFIERS, u32), String> {
    let mut modifiers = HOT_KEY_MODIFIERS(0);
    let mut key = None;
    for part in shortcut.split('-') {
        match part {
            "cmd" | "ctrl" => modifiers |= MOD_CONTROL,
            "shift" => modifiers |= MOD_SHIFT,
            "alt" => modifiers |= MOD_ALT,
            "win" | "super" => modifiers |= MOD_WIN,
            v => key = Some(v),
        }
    }
    let key = key.ok_or("Choose a key for Quick composer.")?;
    let code = match key {
        letter if letter.len() == 1 && letter.as_bytes()[0].is_ascii_alphanumeric() => {
            u32::from(letter.as_bytes()[0].to_ascii_uppercase())
        }
        "space" => 0x20,
        "tab" => 0x09,
        "backspace" => 0x08,
        "escape" => 0x1B,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        "equal" => 0xBB,
        "minus" => 0xBD,
        _ => return Err("This Quick composer shortcut is not supported.".into()),
    };
    if modifiers.0 == 0 {
        return Err("Quick composer needs a modifier key.".into());
    }
    Ok((modifiers, code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_become_windows_modifiers_and_keys() {
        assert_eq!(
            parse("cmd-shift-space").unwrap(),
            (MOD_CONTROL | MOD_SHIFT, 0x20)
        );
        assert_eq!(parse("alt-k").unwrap(), (MOD_ALT, u32::from(b'K')));
        assert_eq!(parse("win-1").unwrap(), (MOD_WIN, u32::from(b'1')));
        assert!(parse("space").is_err());
        assert!(parse("ctrl-f13").is_err());
    }
}
