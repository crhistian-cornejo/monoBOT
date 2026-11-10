//! The live item as one icon in the Windows tray. The tray has no room for
//! rows, so the icon is a ring with the number inside (a plan's fullest
//! window, the agents at work) or the item's mark, and the rows are the
//! tooltip. It follows the taskbar's light or dark theme; one that needs the
//! user wears the accent. A click reaches a message-only window.
use super::mark::{self, Pixels};
use monocode_core::menu_bar::{Item, Mark};
use std::cell::RefCell;
use std::ffi::c_void;
use windows::Win32::Foundation::{ERROR_SUCCESS, HWND, LPARAM, LRESULT, WPARAM};
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
    CreateIconIndirect, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow,
    GetSystemMetrics, HICON, HWND_MESSAGE, ICONINFO, RegisterClassW, SM_CXSMICON, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WM_LBUTTONUP, WNDCLASSW,
};
use windows::core::w;

/// What the tray sends the window when an icon is used.
const CALLBACK: u32 = WM_APP + 0x4C;

thread_local! {
    static CLICKS: RefCell<Option<async_channel::Sender<()>>> = const { RefCell::new(None) };
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == CALLBACK {
        if (lparam.0 & 0xffff) as u32 == WM_LBUTTONUP {
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

pub(super) struct Bar {
    window: HWND,
    shown: Option<(Item, HICON)>,
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
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("MonoBOTLiveItem"),
                w!("monoBOT live item"),
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
            .ok()?
        };
        Some(Self {
            window,
            shown: None,
        })
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
        if self.shown.as_ref().map(|(shown, _)| shown) == item {
            return;
        }
        let previous = self.shown.take();
        let mut data = self.data();
        let Some(item) = item else {
            if let Some((_, icon)) = previous {
                unsafe {
                    let _ = Shell_NotifyIconW(NIM_DELETE, &data);
                    let _ = DestroyIcon(icon);
                }
            }
            return;
        };
        let ink = if light_taskbar() {
            [0, 0, 0]
        } else {
            [255, 255, 255]
        };
        let color = if item.urgent { accent } else { ink };
        let side = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16) as u32;
        // A logo leaves no room for the number: the tray shows the ring.
        let mark = match (item.mark, item.fill) {
            (Mark::Logo(_), Some(fill)) => Mark::Ring(fill),
            (mark, _) => mark,
        };
        let Some(icon) = icon(&mark::draw(mark, side, item.figure, color)) else {
            return;
        };
        data.uFlags = NIF_ICON | NIF_TIP | NIF_MESSAGE | NIF_SHOWTIP;
        data.uCallbackMessage = CALLBACK;
        data.hIcon = icon;
        let tip: Vec<u16> = format!("{}\n{}", item.rows.join(" · "), item.tooltip)
            .encode_utf16()
            .take(data.szTip.len() - 1)
            .collect();
        data.szTip[..tip.len()].copy_from_slice(&tip);
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
        self.shown = Some((item.clone(), icon));
    }
}

impl Drop for Bar {
    fn drop(&mut self) {
        if let Some((_, icon)) = self.shown.take() {
            let data = self.data();
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &data);
                let _ = DestroyIcon(icon);
            }
        }
        unsafe {
            let _ = DestroyWindow(self.window);
        }
        CLICKS.with(|slot| *slot.borrow_mut() = None);
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
