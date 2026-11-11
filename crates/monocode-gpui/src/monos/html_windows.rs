//! WebView2 host for generated HTML on Windows, as `html.rs` hosts WKWebView
//! on macOS. A child window clips the page to GPUI's content mask; the
//! controller inside it keeps the page at full layout size. The profile is
//! InPrivate, with no host objects, web messages or dev tools, and links open
//! in the browser.
//!
//! `NavigateToString` refuses a document over ~2 MB, and a page's pictures
//! (an email's `data:` ones) reach that at once: `host_images` lifts each out
//! of the markup into a memory map, and the page asks for them under
//! `i.monocode.invalid`, a name nothing resolves — the `WebResourceRequested`
//! handler answers it.
use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc, sync::OnceLock};

use base64::Engine;
use webview2_com::{
    CoreWebView2EnvironmentOptions, CreateCoreWebView2ControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE, CreateCoreWebView2EnvironmentWithOptions,
        ICoreWebView2, ICoreWebView2Controller, ICoreWebView2Environment,
        ICoreWebView2Environment10, ICoreWebView2EnvironmentOptions,
        ICoreWebView2NavigationStartingEventArgs, ICoreWebView2WebResourceRequestedEventArgs,
    },
    NavigationStartingEventHandler, NewWindowRequestedEventHandler,
    WebResourceRequestedEventHandler, take_pwstr,
};
use windows::{
    Win32::{
        Foundation::{E_POINTER, HWND, LPARAM, LRESULT, RECT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::Input::KeyboardAndMouse::EnableWindow,
        UI::Shell::SHCreateMemStream,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_TOP, RegisterClassW,
            SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
            SetWindowPos, WINDOW_EX_STYLE, WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
        },
    },
    core::{BOOL, HSTRING, Interface, PWSTR, w},
};

/// Physical client pixels: where the clip sits in GPUI's window, and where
/// the page sits inside the clip.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct Frame {
    pub clip: RECT,
    pub page: RECT,
    pub zoom: f64,
}

#[derive(Default)]
struct Page {
    controller: Option<ICoreWebView2Controller>,
    web: Option<ICoreWebView2>,
    /// The markup the page shows, and the one it should show.
    shown: Option<Arc<str>>,
    wanted: Option<Arc<str>>,
    failed: Option<Arc<str>>,
    frame: Option<Frame>,
    visible: bool,
}

impl Page {
    fn can_show(&self) -> bool {
        self.visible
            && self
                .shown
                .as_ref()
                .zip(self.wanted.as_ref())
                .is_some_and(|(shown, wanted)| Arc::ptr_eq(shown, wanted))
    }
}

/// Every `i.monocode.invalid/<n>` address `host_images` writes, as it is asked for.
pub(super) type HostedImages = Rc<RefCell<HashMap<String, (String, Vec<u8>)>>>;

/// The name that answers a picture in place of its `data:` bytes; it never resolves.
const IMAGES_HOST: &str = "https://i.monocode.invalid/";
const IMAGES_FILTER: &str = "https://i.monocode.invalid/*";

/// The document `html.rs` asks WebView2 to show, without its `data:` pictures: each
/// becomes an `i.monocode.invalid/<n>` address the `WebResourceRequested` handler
/// serves from the returned map, so the markup never passes `NavigateToString`'s
/// ~2 MB refusal. What does not decode stays as it is (`data:` still works).
pub(super) fn host_images(document: &str) -> (String, HashMap<String, (String, Vec<u8>)>) {
    let mut images = HashMap::new();
    let mut out = String::with_capacity(document.len());
    let mut rest = document;
    while let Some(at) = rest.find("data:") {
        let tail = &rest[at..];
        let end = tail
            .find(|c: char| !(c.is_ascii_alphanumeric() || "%:+/=;,.-".contains(c)))
            .unwrap_or(tail.len());
        let data_url = &tail[..end];
        out.push_str(&rest[..at]);
        match data_picture(data_url) {
            Some((kind, bytes)) => {
                let url = format!("{IMAGES_HOST}{}", images.len());
                images.insert(url.clone(), (kind, bytes));
                out.push_str(&url);
            }
            None => out.push_str(data_url),
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    (out, images)
}

/// A `data:` address's media type and bytes, only if it is one the page may get back.
fn data_picture(url: &str) -> Option<(String, Vec<u8>)> {
    let (head, data) = url.strip_prefix("data:")?.split_once(',')?;
    let kind = head.strip_suffix(";base64")?;
    (kind.contains('/') && !data.is_empty()).then_some(())?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    Some((kind.to_string(), bytes))
}

pub(super) struct Hosted {
    clip: HWND,
    page: Rc<RefCell<Page>>,
}

impl Hosted {
    pub(super) fn new(parent: HWND, images: HostedImages) -> Option<Self> {
        let clip = clip_window(parent)?;
        let page = Rc::new(RefCell::new(Page::default()));
        start(clip, page.clone(), images);
        Some(Self { clip, page })
    }

    pub(super) fn show(&self, html: &Arc<str>, frame: Frame, interactive: bool) {
        let RECT {
            left,
            top,
            right,
            bottom,
        } = frame.clip;
        unsafe {
            let _ = SetWindowPos(
                self.clip,
                Some(HWND_TOP),
                left,
                top,
                right - left,
                bottom - top,
                SWP_NOACTIVATE,
            );
            let _ = EnableWindow(self.clip, interactive);
        }
        let mut page = self.page.borrow_mut();
        page.wanted = Some(html.clone());
        page.frame = Some(frame);
        page.visible = true;
        sync(&mut page, self.clip);
    }

    pub(super) fn hide(&self) {
        self.page.borrow_mut().visible = false;
        hide(self.clip);
    }
}

fn hide(clip: HWND) {
    unsafe {
        let _ = SetWindowPos(
            clip,
            None,
            0,
            0,
            0,
            0,
            SWP_HIDEWINDOW | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

impl Drop for Hosted {
    fn drop(&mut self) {
        if let Some(controller) = self.page.borrow_mut().controller.take() {
            let _ = unsafe { controller.Close() };
        }
        let _ = unsafe { DestroyWindow(self.clip) };
    }
}

/// Bring the controller up to the latest frame and markup, once it exists.
fn sync(page: &mut Page, clip: HWND) {
    let (Some(controller), Some(web), Some(frame)) = (&page.controller, &page.web, page.frame)
    else {
        return;
    };
    if unsafe {
        controller.SetBounds(frame.page).is_err() || controller.SetZoomFactor(frame.zoom).is_err()
    } {
        hide(clip);
        return;
    }
    if let Some(html) = page.wanted.clone()
        && page
            .shown
            .as_ref()
            .is_none_or(|shown| !Arc::ptr_eq(shown, &html))
        && page
            .failed
            .as_ref()
            .is_none_or(|failed| !Arc::ptr_eq(failed, &html))
    {
        if unsafe { web.NavigateToString(&HSTRING::from(&*html)) }.is_ok() {
            page.shown = Some(html);
            page.failed = None;
        } else {
            page.failed = Some(html);
        }
    }
    if page.can_show() {
        unsafe {
            let _ = SetWindowPos(
                clip,
                None,
                0,
                0,
                0,
                0,
                SWP_SHOWWINDOW | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    } else {
        hide(clip);
    }
}

fn clip_window(parent: HWND) -> Option<HWND> {
    static CLASS: OnceLock<bool> = OnceLock::new();
    let instance = unsafe { GetModuleHandleW(None) }.ok()?;
    let registered = *CLASS.get_or_init(|| {
        let class = WNDCLASSW {
            lpfnWndProc: Some(clip_procedure),
            hInstance: instance.into(),
            lpszClassName: w!("MonoBOTHtmlClip"),
            ..Default::default()
        };
        unsafe { RegisterClassW(&class) != 0 }
    });
    if !registered {
        return None;
    }
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("MonoBOTHtmlClip"),
            None,
            WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
            0,
            0,
            0,
            0,
            Some(parent),
            None,
            Some(instance.into()),
            None,
        )
    }
    .ok()
}

unsafe extern "system" fn clip_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// Create the environment and the controller asynchronously; both callbacks
/// arrive on this UI thread through GPUI's message loop.
fn start(clip: HWND, page: Rc<RefCell<Page>>, images: HostedImages) {
    let folder = std::env::temp_dir().join("monoBOT").join("WebView2");
    let options: ICoreWebView2EnvironmentOptions = CoreWebView2EnvironmentOptions::default().into();
    let created = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |result, environment| {
            result?;
            let environment = environment.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
            create_controller(&environment, clip, page, images)
        },
    ));
    let _ = unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            None,
            &HSTRING::from(folder.as_os_str()),
            &options,
            &created,
        )
    };
}

fn create_controller(
    environment: &ICoreWebView2Environment,
    clip: HWND,
    page: Rc<RefCell<Page>>,
    images: HostedImages,
) -> windows::core::Result<()> {
    let for_guard = environment.clone();
    let created = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
        move |result, controller| {
            result?;
            let controller = controller.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
            let web = unsafe { controller.CoreWebView2() }?;
            guard(&web, &for_guard, images)?;
            let mut page = page.borrow_mut();
            page.controller = Some(controller);
            page.web = Some(web);
            sync(&mut page, clip);
            Ok(())
        },
    ));
    // InPrivate keeps no website data, like WKWebsiteDataStore.nonPersistent.
    if let Ok(environment) = environment.cast::<ICoreWebView2Environment10>()
        && let Ok(options) = unsafe { environment.CreateCoreWebView2ControllerOptions() }
    {
        unsafe {
            options.SetIsInPrivateModeEnabled(true)?;
            return environment.CreateCoreWebView2ControllerWithOptions(clip, &options, &created);
        }
    }
    unsafe { environment.CreateCoreWebView2Controller(clip, &created) }
}

/// Only the document itself loads; a clicked web link opens in the browser. The
/// page's `i.monocode.invalid` pictures are answered from memory, never the web.
fn guard(
    web: &ICoreWebView2,
    environment: &ICoreWebView2Environment,
    images: HostedImages,
) -> windows::core::Result<()> {
    unsafe {
        let settings = web.Settings()?;
        settings.SetAreDevToolsEnabled(false)?;
        settings.SetAreHostObjectsAllowed(false)?;
        settings.SetIsWebMessageEnabled(false)?;
        settings.SetIsStatusBarEnabled(false)?;
        web.AddWebResourceRequestedFilter(
            &HSTRING::from(IMAGES_FILTER),
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE,
        )?;
        let environment = environment.clone();
        let mut token = 0;
        web.add_WebResourceRequested(
            &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    picture(&args, &environment, &images)?;
                }
                Ok(())
            })),
            &mut token,
        )?;
        web.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(|_, args| {
                if let Some(args) = args {
                    decide(&args)?;
                }
                Ok(())
            })),
            &mut token,
        )?;
        web.add_FrameNavigationStarting(
            // Frames reuse the navigation handler interface.
            &NavigationStartingEventHandler::create(Box::new(|_, args| {
                if let Some(args) = args {
                    decide(&args)?;
                }
                Ok(())
            })),
            &mut token,
        )?;
        web.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(|_, args| {
                if let Some(args) = args {
                    let mut uri = PWSTR::null();
                    args.Uri(&mut uri)?;
                    let uri = take_pwstr(uri);
                    if is_web(&uri) {
                        open(&uri);
                    }
                    args.SetHandled(true)?;
                }
                Ok(())
            })),
            &mut token,
        )?;
    }
    Ok(())
}

/// A picture the page asks for under `i.monocode.invalid`: its bytes from memory,
/// never a request — anything the map does not name is not answered.
fn picture(
    args: &ICoreWebView2WebResourceRequestedEventArgs,
    environment: &ICoreWebView2Environment,
    images: &HostedImages,
) -> windows::core::Result<()> {
    let request = unsafe { args.Request()? };
    let mut uri = PWSTR::null();
    unsafe { request.Uri(&mut uri)? };
    let uri = take_pwstr(uri);
    let Some((kind, bytes)) = images.borrow().get(&uri).cloned() else {
        return Ok(());
    };
    let Some(stream) = (unsafe { SHCreateMemStream(Some(bytes.as_slice())) }) else {
        return Ok(());
    };
    let headers = format!("Content-Type: {kind}\r\nCache-Control: no-store\r\n");
    let response = unsafe {
        environment.CreateWebResourceResponse(
            &stream,
            200,
            w!("OK"),
            &HSTRING::from(headers.as_str()),
        )?
    };
    unsafe { args.SetResponse(&response) }
}

fn decide(args: &ICoreWebView2NavigationStartingEventArgs) -> windows::core::Result<()> {
    let mut uri = PWSTR::null();
    let mut user = BOOL::default();
    unsafe {
        args.Uri(&mut uri)?;
        args.IsUserInitiated(&mut user)?;
    }
    let uri = take_pwstr(uri);
    if user.as_bool() && is_web(&uri) {
        open(&uri);
    }
    if !allowed(&uri) {
        unsafe { args.SetCancel(true)? };
    }
    Ok(())
}

/// NavigateToString loads as a data URL; the sandboxed iframe as about:srcdoc.
fn allowed(uri: &str) -> bool {
    uri.is_empty()
        || uri.starts_with("about:blank")
        || uri.starts_with("about:srcdoc")
        || uri.starts_with("data:text/html")
}

fn is_web(uri: &str) -> bool {
    uri.starts_with("https://") || uri.starts_with("http://")
}

/// Off the GPUI update path: WebView2 raises these from inside the message loop.
fn open(uri: &str) {
    let _ = std::process::Command::new("explorer.exe").arg(uri).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_document_itself_may_load() {
        assert!(allowed("about:blank"));
        assert!(allowed("about:srcdoc"));
        assert!(allowed("data:text/html;charset=utf-8,<p>hi</p>"));
        assert!(!allowed("https://example.com"));
        assert!(!allowed("file:///C:/Windows/win.ini"));
        assert!(is_web("http://example.com") && !is_web("javascript:alert(1)"));
    }

    #[test]
    fn data_pictures_leave_the_document_and_are_served_by_address() {
        let (document, images) = host_images(
            r##"<head><meta content="img-src data: https://i.monocode.invalid"></head><body><img src="data:image/png;base64,AQID"><p>data: no es una imagen</p><img src="https://web.test/x.png"><div style="background:url('data:image/gif;base64,BAUG')"></div></body>"##,
        );
        assert!(
            !document.contains("base64") && document.contains("img-src data:"),
            "{document}"
        );
        assert!(document.contains("src=\"https://i.monocode.invalid/0\""));
        assert!(document.contains("https://web.test/x.png"));
        assert!(document.contains("https://i.monocode.invalid/1"));
        assert_eq!(images.len(), 2);
        assert_eq!(
            images["https://i.monocode.invalid/0"],
            ("image/png".to_string(), vec![1, 2, 3])
        );
        assert_eq!(
            images["https://i.monocode.invalid/1"],
            ("image/gif".to_string(), vec![4, 5, 6])
        );
    }

    #[test]
    fn what_does_not_decode_stays_in_the_document() {
        let (document, images) = host_images(
            r##"<p>data:image/png,AAA data:sincoma data:image/png;base64,$$$ data:image/png;base64,</p>"##,
        );
        assert!(images.is_empty(), "{images:?}");
        assert!(
            document.contains("data:image/png,AAA") && document.contains("data:image/png;base64,"),
            "{document}"
        );
    }

    #[test]
    fn the_clip_stays_hidden_until_the_current_page_can_be_shown() {
        let first: Arc<str> = "<p>one</p>".into();
        let next: Arc<str> = "<p>two</p>".into();
        let mut page = Page {
            wanted: Some(first.clone()),
            visible: true,
            ..Default::default()
        };
        assert!(!page.can_show());
        page.shown = Some(first);
        assert!(page.can_show());
        page.wanted = Some(next);
        assert!(!page.can_show());
        page.visible = false;
        page.shown = page.wanted.clone();
        assert!(!page.can_show());
    }
}
