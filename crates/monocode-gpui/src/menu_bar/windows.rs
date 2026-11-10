//! monoBOT's icon in the Windows tray. With no live item it is monoBOT's
//! own logo (the executable's icon); with one it is a ring with the number
//! inside (a plan's fullest window, the agents at work) or the item's mark,
//! and the rows are the tooltip. The item follows the taskbar's light or
//! dark theme; one that needs the user wears the accent. A left or right
//! click shows monoBOT's menu, as the menu bar icon does on macOS.
//!
//! Windows keeps a new tray icon in the overflow until the user drags it to
//! the taskbar or turns it on in Settings › Personalization › Taskbar; no
//! app can do that for them.
use super::Entry;
use super::mark::{self, Pixels};
use monocode_core::menu_bar::{Item, Mark};
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use windows::Win32::Foundation::{ERROR_SUCCESS, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
    DeleteObject, HGDIOBJ,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon,
    DestroyMenu, DestroyWindow, GetCursorPos, GetSystemMetrics, HICON, HMENU, ICONINFO, IMAGE_ICON,
    LR_DEFAULTCOLOR, LoadImageW, MENU_ITEM_FLAGS, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR,
    MF_STRING, PostMessageW, RegisterClassW, SM_CXSMICON, SetForegroundWindow, TPM_BOTTOMALIGN,
    TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, WM_APP, WM_CONTEXTMENU,
    WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{PCWSTR, w};

/// What the tray sends the window when an icon is used.
const CALLBACK: u32 = WM_APP + 0x4C;

thread_local! {
    static CLICKS: RefCell<Option<async_channel::Sender<()>>> = const { RefCell::new(None) };
    /// The hidden window that owns the icon and its menu.
    static OWNER: Cell<Option<HWND>> = const { Cell::new(None) };
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == CALLBACK {
        let event = (lparam.0 & 0xffff) as u32;
        if matches!(event, WM_LBUTTONUP | WM_RBUTTONUP | WM_CONTEXTMENU) {
            CLICKS.with(|clicks| {
                if let Some(clicks) = clicks.borrow().as_ref() {
                    let _ = clicks.try_send(());
                }
            });
        }
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// What the icon shows now.
#[derive(PartialEq)]
enum Shown {
    Logo,
    Item(Item),
}

pub(super) struct Bar {
    window: HWND,
    visible: bool,
    /// The live item, kept to redraw when the icon comes back.
    item: Option<Item>,
    accent: [u8; 3],
    shown: Option<(Shown, HICON)>,
}

/// The tray icon's id within the window.
const ID: u32 = 1;

impl Bar {
    pub(super) fn new(clicks: async_channel::Sender<()>) -> Option<Self> {
        CLICKS.with(|slot| *slot.borrow_mut() = Some(clicks));
        let window = unsafe {
            let instance = GetModuleHandleW(None).ok()?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: w!("MonoBOTLiveItem"),
                ..Default::default()
            };
            // A second bar finds the class already there.
            RegisterClassW(&class);
            // A real (never shown) window, not a message-only one: a tray
            // menu needs an owner that can come to the foreground, or it
            // stays open after a click elsewhere.
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                w!("MonoBOTLiveItem"),
                w!("monoBOT"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .ok()?
        };
        OWNER.with(|owner| owner.set(Some(window)));
        let mut bar = Self {
            window,
            visible: true,
            item: None,
            accent: [0, 0, 0],
            shown: None,
        };
        bar.redraw();
        Some(bar)
    }

    fn data(&self) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.window,
            uID: ID,
            ..Default::default()
        }
    }

    pub(super) fn show(&mut self, item: Option<&Item>, accent: [u8; 3]) {
        if self.item.as_ref() == item && self.accent == accent {
            return;
        }
        self.item = item.cloned();
        self.accent = accent;
        self.redraw();
    }

    /// Settings › Monos › Tray icon.
    pub(super) fn set_visible(&mut self, visible: bool) {
        if self.visible != visible {
            self.visible = visible;
            self.redraw();
        }
    }

    fn remove(&mut self) {
        if let Some((_, icon)) = self.shown.take() {
            let data = self.data();
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &data);
                let _ = DestroyIcon(icon);
            }
        }
    }

    fn redraw(&mut self) {
        if !self.visible {
            self.remove();
            return;
        }
        let wanted = match &self.item {
            Some(item) => Shown::Item(item.clone()),
            None => Shown::Logo,
        };
        if self
            .shown
            .as_ref()
            .is_some_and(|(shown, _)| *shown == wanted)
        {
            return;
        }
        let side = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
        let (icon, tip) = match &wanted {
            Shown::Logo => (logo(side), "monoBOT".to_owned()),
            Shown::Item(item) => {
                let ink = if light_taskbar() {
                    [0, 0, 0]
                } else {
                    [255, 255, 255]
                };
                let color = if item.urgent { self.accent } else { ink };
                // A logo leaves no room for the number: the tray shows the ring.
                let mark = match (item.mark, item.fill) {
                    (Mark::Logo(_), Some(fill)) => Mark::Ring(fill),
                    (mark, _) => mark,
                };
                (
                    icon(&mark::draw(mark, side as u32, item.figure, color)),
                    format!("{}\n{}", item.rows.join(" · "), item.tooltip),
                )
            }
        };
        let Some(icon) = icon else {
            return;
        };
        let mut data = self.data();
        data.uFlags = NIF_ICON | NIF_TIP | NIF_MESSAGE | NIF_SHOWTIP;
        data.uCallbackMessage = CALLBACK;
        data.hIcon = icon;
        let tip: Vec<u16> = tip.encode_utf16().take(data.szTip.len() - 1).collect();
        data.szTip[..tip.len()].copy_from_slice(&tip);
        let previous = self.shown.take();
        unsafe {
            let message = if previous.is_some() {
                NIM_MODIFY
            } else {
                NIM_ADD
            };
            let _ = Shell_NotifyIconW(message, &data);
            if let Some((_, old)) = previous {
                let _ = DestroyIcon(old);
            }
        }
        self.shown = Some((wanted, icon));
    }
}

impl Drop for Bar {
    fn drop(&mut self) {
        self.remove();
        unsafe {
            let _ = DestroyWindow(self.window);
        }
        OWNER.with(|owner| owner.set(None));
        CLICKS.with(|slot| *slot.borrow_mut() = None);
    }
}

/// The executable's own icon (resource 1, from `assets/icons/icon.ico`),
/// at the tray's size.
fn logo(side: i32) -> Option<HICON> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        LoadImageW(
            Some(instance.into()),
            PCWSTR(std::ptr::without_provenance(1)),
            IMAGE_ICON,
            side,
            side,
            LR_DEFAULTCOLOR,
        )
        .ok()
        .map(|handle| HICON(handle.0))
    }
}

/// Shows the menu at the pointer and returns the chosen action's id.
pub(super) fn popup(entries: &[Entry]) -> Option<String> {
    let owner = OWNER.with(Cell::get)?;
    let mut ids = Vec::new();
    unsafe {
        let menu = build(entries, &mut ids)?;
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        // Without this the menu would not close on a click elsewhere.
        let _ = SetForegroundWindow(owner);
        let chosen = TrackPopupMenuEx(
            menu,
            (TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY | TPM_BOTTOMALIGN).0,
            point.x,
            point.y,
            owner,
            None,
        );
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        let index = usize::try_from(chosen.0).ok()?.checked_sub(1)?;
        ids.get(index).cloned()
    }
}

/// A Win32 menu for the entries; each action's command is its index in
/// `ids` plus one (zero means nothing was chosen).
unsafe fn build(entries: &[Entry], ids: &mut Vec<String>) -> Option<HMENU> {
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        for entry in entries {
            match entry {
                Entry::Action { label, id, checked } => {
                    ids.push(id.clone());
                    let flags = if *checked {
                        MF_STRING | MF_CHECKED
                    } else {
                        MF_STRING
                    };
                    append(menu, flags, ids.len(), label);
                }
                Entry::Note(label) => append(menu, MF_STRING | MF_GRAYED, 0, label),
                Entry::Separator => {
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                }
                Entry::Submenu { label, entries } => {
                    if let Some(sub) = build(entries, ids) {
                        append(menu, MF_STRING | MF_POPUP, sub.0 as usize, label);
                    }
                }
            }
        }
        Some(menu)
    }
}

unsafe fn append(menu: HMENU, flags: MENU_ITEM_FLAGS, id: usize, label: &str) {
    // An ampersand would underline the next letter.
    let text: Vec<u16> = label
        .replace('&', "&&")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
        let _ = AppendMenuW(menu, flags, id, PCWSTR(text.as_ptr()));
    }
}

/// Whether the taskbar is light (Settings › Personalization › Colors).
fn light_taskbar() -> bool {
    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut c_void),
            Some(&mut size),
        )
    };
    result == ERROR_SUCCESS && value == 1
}

/// The pixels as an icon with its own alpha.
fn icon(pixels: &Pixels) -> Option<HICON> {
    let side = pixels.size as i32;
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: side,
            // Negative: rows from the top.
            biHeight: -side,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe {
        let mut bits: *mut c_void = std::ptr::null_mut();
        let color = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        if bits.is_null() {
            let _ = DeleteObject(HGDIOBJ(color.0));
            return None;
        }
        let bgra: Vec<u8> = pixels
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|px| [px[2], px[1], px[0], px[3]])
            .collect();
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, bgra.len());
        let mask = CreateBitmap(side, side, 1, 1, None);
        let icon = CreateIconIndirect(&ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        })
        .ok();
        let _ = DeleteObject(HGDIOBJ(color.0));
        let _ = DeleteObject(HGDIOBJ(mask.0));
        icon
    }
}
