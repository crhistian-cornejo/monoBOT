//! The device's screen as it is drawn, straight from CoreSimulator: the
//! framebuffer IOSurface the simulator renders into, and a call on every
//! new frame. The live pane shows that surface as a layer's contents, with
//! no copy, encoding or decoding, the way Simulator.app and Claude's helper
//! do. The hardware keyboard switch lives here too.
//!
//! These are Apple's private frameworks, loaded at run time from the active
//! Xcode: `CoreSimulator` and `SimulatorKit`. Every selector is checked
//! against the protocol this Xcode declares before it is sent, so a newer
//! Xcode that renames something makes [`ScreenFeed::open`] fail and the pane
//! falls back to the runner's frames, instead of raising inside the app.
//! Measured with Xcode 27.2: `SimScreen` ports, `framebufferSurface` BGRA at
//! 1206 × 2622 on an iPhone 18 Pro, a frame call per screen change.
//!
//! Opening a feed and closing it talk to CoreSimulator over XPC: never on
//! the interface thread.
use std::{
    ffi::{c_char, c_void, CStr, CString},
    ptr::NonNull,
    sync::{Arc, Mutex, OnceLock},
};

use objc2::{
    msg_send,
    rc::{Allocated, Retained},
    runtime::{AnyClass, AnyObject, AnyProtocol, Bool, Sel},
    sel,
};
use objc2_foundation::NSString;

const CORE_SIMULATOR: &CStr =
    c"/Library/Developer/PrivateFrameworks/CoreSimulator.framework/CoreSimulator";
/// SimulatorKit, relative to the selected developer folder.
const SIMULATOR_KIT: &str = "../SharedFrameworks/SimulatorKit.framework/SimulatorKit";

#[link(name = "IOSurface", kind = "framework")]
unsafe extern "C" {
    fn IOSurfaceGetWidth(surface: *const c_void) -> usize;
    fn IOSurfaceGetHeight(surface: *const c_void) -> usize;
    fn IOSurfaceGetBytesPerRow(surface: *const c_void) -> usize;
    fn IOSurfaceGetPixelFormat(surface: *const c_void) -> u32;
    fn IOSurfaceGetBaseAddress(surface: *const c_void) -> *mut c_void;
    fn IOSurfaceLock(surface: *const c_void, options: u32, seed: *mut u32) -> i32;
    fn IOSurfaceUnlock(surface: *const c_void, options: u32, seed: *mut u32) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRetain(object: *const c_void) -> *const c_void;
    fn CFRelease(object: *const c_void);
}

unsafe extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_queue_create(label: *const c_char, attributes: *const c_void) -> *mut AnyObject;
    fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
}
const RTLD_NOW: i32 = 2;
const LOCK_READ_ONLY: u32 = 1;
/// `'BGRA'`, the simulator's framebuffer format.
pub const BGRA: u32 = u32::from_be_bytes(*b"BGRA");

/// A retained IOSurface. The pixels change under it as the device draws.
pub struct Surface(NonNull<c_void>);

// SAFETY: an IOSurfaceRef is a thread-safe CoreFoundation object; the
// pixels are read under IOSurfaceLock.
unsafe impl Send for Surface {}
unsafe impl Sync for Surface {}

impl Surface {
    /// Retains `surface`. `None` when it is null.
    ///
    /// # Safety
    /// `surface` must be null or a live IOSurfaceRef.
    pub unsafe fn retain(surface: *const c_void) -> Option<Self> {
        let surface = NonNull::new(surface.cast_mut())?;
        unsafe { CFRetain(surface.as_ptr()) };
        Some(Self(surface))
    }

    /// The IOSurfaceRef, for a layer's `contents`.
    pub fn as_ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }

    /// Width and height in pixels.
    pub fn size(&self) -> (u32, u32) {
        unsafe {
            (
                IOSurfaceGetWidth(self.as_ptr()) as u32,
                IOSurfaceGetHeight(self.as_ptr()) as u32,
            )
        }
    }

    pub fn pixel_format(&self) -> u32 {
        unsafe { IOSurfaceGetPixelFormat(self.as_ptr()) }
    }

    /// A copy of the pixels as they are now, still in BGRA order: what a
    /// `RenderImage` takes, and what a crop is cut from. `None` when the
    /// surface is not BGRA or cannot be locked.
    pub fn copy_bgra(&self) -> Option<image::RgbaImage> {
        if self.pixel_format() != BGRA {
            return None;
        }
        let (width, height) = self.size();
        unsafe {
            if IOSurfaceLock(self.as_ptr(), LOCK_READ_ONLY, std::ptr::null_mut()) != 0 {
                return None;
            }
            let base = IOSurfaceGetBaseAddress(self.as_ptr()).cast::<u8>();
            let stride = IOSurfaceGetBytesPerRow(self.as_ptr());
            let row = width as usize * 4;
            let mut pixels = Vec::with_capacity(row * height as usize);
            if !base.is_null() && stride >= row {
                for y in 0..height as usize {
                    pixels.extend_from_slice(std::slice::from_raw_parts(base.add(y * stride), row));
                }
            }
            IOSurfaceUnlock(self.as_ptr(), LOCK_READ_ONLY, std::ptr::null_mut());
            image::RgbaImage::from_raw(width, height, pixels)
        }
    }
}

impl Clone for Surface {
    fn clone(&self) -> Self {
        unsafe { CFRetain(self.as_ptr()) };
        Self(self.0)
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe { CFRelease(self.as_ptr()) };
    }
}

/// What a feed reports.
pub enum FeedEvent {
    /// The device drew a new frame into the surface.
    Frame,
    /// The device swapped to another surface (a new size or a reboot).
    Surface(Surface),
}

/// Where a feed's calls arrive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// The main thread, where AppKit layers can be touched.
    Main,
    /// A private serial queue.
    Private,
}

/// The device's main screen, followed frame by frame until dropped.
pub struct ScreenFeed {
    screen: Retained<AnyObject>,
    token: Retained<AnyObject>,
    surface: Arc<Mutex<Surface>>,
}

// SAFETY: the screen is a CoreSimulator remote proxy, which forwards every
// message over XPC and may be used from any thread.
unsafe impl Send for ScreenFeed {}
unsafe impl Sync for ScreenFeed {}

/// The frameworks, loaded once. The error says what was missing.
fn load() -> Result<(), String> {
    static LOADED: OnceLock<Result<(), String>> = OnceLock::new();
    LOADED
        .get_or_init(|| {
            let developer = super::run(
                super::XCODE_SELECT,
                &["-p"],
                std::time::Duration::from_secs(10),
            )?
            .ok("xcode-select")?;
            let kit = std::path::Path::new(developer.trim()).join(SIMULATOR_KIT);
            let kit =
                CString::new(kit.to_string_lossy().into_owned()).map_err(|e| e.to_string())?;
            for path in [CORE_SIMULATOR, kit.as_c_str()] {
                if unsafe { dlopen(path.as_ptr(), RTLD_NOW) }.is_null() {
                    return Err(format!("{} could not be loaded", path.to_string_lossy()));
                }
            }
            Ok(())
        })
        .clone()
}

/// Runs CoreSimulator calls with Objective-C exceptions caught: one raised
/// through Rust would abort the app.
fn guarded<R>(what: &str, work: impl FnOnce() -> Result<R, String>) -> Result<R, String> {
    match objc2::exception::catch(std::panic::AssertUnwindSafe(work)) {
        Ok(result) => result,
        Err(exception) => Err(format!(
            "CoreSimulator raised an exception while {what}: {}",
            exception.map_or_else(|| "unknown".to_owned(), |e| format!("{e:?}"))
        )),
    }
}

/// Whether this Xcode's `protocol` declares `selector`, so sending it to a
/// proxy that conforms cannot raise "unrecognized selector".
fn declares(protocol: &AnyProtocol, selector: Sel) -> bool {
    #[repr(C)]
    struct MethodDescription {
        name: *const c_void,
        types: *const c_char,
    }
    unsafe extern "C" {
        fn protocol_getMethodDescription(
            protocol: *const AnyProtocol,
            selector: Sel,
            required: Bool,
            instance: Bool,
        ) -> MethodDescription;
    }
    [true, false].into_iter().any(|required| {
        let description = unsafe {
            protocol_getMethodDescription(protocol, selector, Bool::new(required), Bool::YES)
        };
        !description.name.is_null()
    })
}

fn protocol(name: &CStr, selectors: &[Sel]) -> Result<&'static AnyProtocol, String> {
    let protocol = AnyProtocol::get(name)
        .ok_or_else(|| format!("This Xcode has no {}", name.to_string_lossy()))?;
    match selectors.iter().find(|s| !declares(protocol, **s)) {
        Some(missing) => Err(format!(
            "This Xcode's {} has no {missing}",
            name.to_string_lossy()
        )),
        None => Ok(protocol),
    }
}

/// Sends a message without objc2's check that the receiver's class has the
/// method: CoreSimulator's remote proxies forward every message they get,
/// so they have none, and the check would refuse them. Only selectors a
/// conformed protocol declares are sent this way ([`declares`]).
///
/// # Safety
/// `F` must be the method's C signature, with receiver and selector first.
unsafe fn forwarded<F: Copy>() -> F {
    assert_eq!(std::mem::size_of::<F>(), std::mem::size_of::<*const ()>());
    let send = objc2::ffi::objc_msgSend as *const ();
    unsafe { std::mem::transmute_copy(&send) }
}

/// A getter on a proxy that answers an object (+0), retained.
unsafe fn get(object: &AnyObject, selector: Sel) -> Option<Retained<AnyObject>> {
    let send: unsafe extern "C-unwind" fn(&AnyObject, Sel) -> *mut AnyObject =
        unsafe { forwarded() };
    unsafe { Retained::retain(send(object, selector)) }
}

/// `framebufferSurface`, retained.
unsafe fn framebuffer(screen: &AnyObject) -> Option<Surface> {
    let send: unsafe extern "C-unwind" fn(&AnyObject, Sel) -> *const c_void =
        unsafe { forwarded() };
    unsafe { Surface::retain(send(screen, sel!(framebufferSurface))) }
}

fn conforms(object: &AnyObject, protocol: &AnyProtocol) -> bool {
    let answer: Bool = unsafe { msg_send![object, conformsToProtocol: protocol] };
    answer.as_bool()
}

/// The `SimDevice` with this UDID in the default device set.
fn device(udid: &str) -> Result<Retained<AnyObject>, String> {
    load()?;
    guarded("finding the device", || find_device(udid))
}

fn find_device(udid: &str) -> Result<Retained<AnyObject>, String> {
    let developer = super::run(
        super::XCODE_SELECT,
        &["-p"],
        std::time::Duration::from_secs(10),
    )?
    .ok("xcode-select")?;
    let class = |name: &CStr| {
        AnyClass::get(name).ok_or_else(|| format!("This Xcode has no {}", name.to_string_lossy()))
    };
    let context_class = class(c"SimServiceContext")?;
    let uuid_class = class(c"NSUUID")?;
    unsafe {
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let developer = NSString::from_str(developer.trim());
        let context: Option<Retained<AnyObject>> = msg_send![
            context_class,
            sharedServiceContextForDeveloperDir: &*developer,
            error: &mut error
        ];
        let context = context.ok_or("CoreSimulator's service did not answer")?;
        let set: Option<Retained<AnyObject>> =
            msg_send![&context, defaultDeviceSetWithError: &mut error];
        let set = set.ok_or("CoreSimulator has no device set")?;
        let devices: Option<Retained<AnyObject>> = msg_send![&set, devicesByUDID];
        let devices = devices.ok_or("CoreSimulator listed no devices")?;
        let allocated: Allocated<AnyObject> = msg_send![uuid_class, alloc];
        let uuid: Option<Retained<AnyObject>> =
            msg_send![allocated, initWithUUIDString: &*NSString::from_str(udid)];
        let uuid = uuid.ok_or_else(|| format!("'{udid}' is not a device UDID"))?;
        let device: Option<Retained<AnyObject>> = msg_send![&devices, objectForKey: &*uuid];
        device.ok_or_else(|| format!("No simulator has the UDID {udid}"))
    }
}

/// Connects or disconnects the Mac's keyboard from the device. Without it,
/// iOS shows its own keyboard when a text field has the focus.
pub fn set_hardware_keyboard(udid: &str, connected: bool) -> Result<(), String> {
    let device = device(udid)?;
    guarded("switching the keyboard", || {
        switch_keyboard(&device, connected)
    })
}

fn switch_keyboard(device: &AnyObject, connected: bool) -> Result<(), String> {
    let selector = sel!(setHardwareKeyboardEnabled:keyboardType:error:);
    let responds: Bool = unsafe { msg_send![device, respondsToSelector: selector] };
    if !responds.as_bool() {
        return Err("This Xcode cannot switch the simulator's keyboard".into());
    }
    let mut error: *mut AnyObject = std::ptr::null_mut();
    let done: Bool = unsafe {
        msg_send![
            device,
            setHardwareKeyboardEnabled: Bool::new(connected),
            keyboardType: 0u8,
            error: &mut error
        ]
    };
    if done.as_bool() {
        Ok(())
    } else {
        Err("CoreSimulator did not switch the keyboard".into())
    }
}

impl ScreenFeed {
    /// Starts following the main screen of the booted device `udid`.
    /// `on_event` runs for every frame and surface swap, on `delivery`.
    pub fn open(
        udid: &str,
        delivery: Delivery,
        on_event: impl Fn(FeedEvent) + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let device = device(udid)?;
        let screen_protocol = protocol(
            c"SimScreen",
            &[
                sel!(registerScreenCallbacksWithUUID:callbackQueue:frameCallback:surfacesChangedCallback:propertiesChangedCallback:),
                sel!(unregisterScreenCallbacksWithUUID:),
            ],
        )?;
        let surface_protocol = protocol(
            c"SimDisplayIOSurfaceRenderable",
            &[sel!(framebufferSurface)],
        )?;
        guarded("opening the screen", move || unsafe {
            let io: Option<Retained<AnyObject>> = msg_send![&device, io];
            let io = io.ok_or("The device has no IO client; is it booted?")?;
            let ports: Option<Retained<AnyObject>> = msg_send![&io, ioPorts];
            let ports = ports.ok_or("The device lists no IO ports")?;
            let count: usize = msg_send![&ports, count];
            // Several ports are screens (external displays, CarPlay); the
            // main one is the screen with a framebuffer.
            let mut found = None;
            for index in 0..count {
                let port: Retained<AnyObject> = msg_send![&ports, objectAtIndex: index];
                let descriptor = get(&port, sel!(descriptor));
                let Some(descriptor) = descriptor else {
                    continue;
                };
                if !conforms(&descriptor, screen_protocol)
                    || !conforms(&descriptor, surface_protocol)
                {
                    continue;
                }
                if let Some(surface) = framebuffer(&descriptor) {
                    found = Some((descriptor, surface));
                    break;
                }
            }
            let (screen, surface) = found.ok_or("The device has no screen to show yet")?;
            let surface = Arc::new(Mutex::new(surface));
            let token: Retained<AnyObject> =
                msg_send![AnyClass::get(c"NSUUID").ok_or("no NSUUID")?, new];
            let queue: *mut AnyObject = match delivery {
                Delivery::Main => (&raw const _dispatch_main_q).cast_mut().cast(),
                Delivery::Private => {
                    dispatch_queue_create(c"monobot.simulator.screen".as_ptr(), std::ptr::null())
                }
            };
            let on_event = Arc::new(on_event);
            let frame = {
                let on_event = on_event.clone();
                callback(move || on_event(FeedEvent::Frame))
            };
            let swapped = {
                let screen = SendScreen(screen.clone());
                let surface = surface.clone();
                callback(move || {
                    let new = guarded("reading the new surface", || Ok(framebuffer(&screen.0)));
                    if let Ok(Some(new)) = new {
                        if let Ok(mut current) = surface.lock() {
                            *current = new.clone();
                        }
                        on_event(FeedEvent::Surface(new));
                    }
                })
            };
            let properties = callback(|| {});
            type Callback = block2::Block<dyn Fn()>;
            let register: unsafe extern "C-unwind" fn(
                &AnyObject,
                Sel,
                &AnyObject,
                *mut AnyObject,
                &Callback,
                &Callback,
                &Callback,
            ) = forwarded();
            register(
                &screen,
                sel!(registerScreenCallbacksWithUUID:callbackQueue:frameCallback:surfacesChangedCallback:propertiesChangedCallback:),
                &token,
                queue,
                &frame,
                &swapped,
                &properties,
            );
            Ok(Self {
                screen,
                token,
                surface,
            })
        })
    }

    /// The surface the device draws into now.
    pub fn surface(&self) -> Surface {
        self.surface
            .lock()
            .map(|surface| surface.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
    }
}

/// `void (^)(void)`. CoreSimulator's proxies send blocks over XPC and need
/// each block's signature to do it; a block without one raises.
struct VoidBlock;
// SAFETY: the encoding matches `dyn Fn()`, as clang writes it.
unsafe impl block2::ManualBlockEncoding for VoidBlock {
    type Arguments = ();
    type Return = ();
    const ENCODING_CSTR: &'static CStr = c"v8@?0";
}

fn callback(closure: impl Fn() + 'static) -> block2::RcBlock<dyn Fn()> {
    block2::RcBlock::with_encoding::<_, _, _, VoidBlock>(closure)
}

/// The screen proxy, moved into the swap callback.
struct SendScreen(Retained<AnyObject>);
// SAFETY: see `ScreenFeed`.
unsafe impl Send for SendScreen {}
unsafe impl Sync for SendScreen {}

impl Drop for ScreenFeed {
    fn drop(&mut self) {
        let _ = guarded("closing the screen", || unsafe {
            let unregister: unsafe extern "C-unwind" fn(&AnyObject, Sel, &AnyObject) = forwarded();
            unregister(
                &self.screen,
                sel!(unregisterScreenCallbacksWithUUID:),
                &self.token,
            );
            Ok(())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn only_selectors_this_xcode_declares_pass() {
        if super::super::xcode_problem().is_some() || load().is_err() {
            return;
        }
        assert!(protocol(
            c"SimDisplayIOSurfaceRenderable",
            &[sel!(framebufferSurface)]
        )
        .is_ok());
        assert!(protocol(c"SimDisplayIOSurfaceRenderable", &[sel!(noSuchSelector)]).is_err());
        assert!(protocol(c"NoSuchProtocol", &[]).is_err());
    }

    /// Needs a booted simulator; passes without one.
    #[test]
    fn the_booted_device_feeds_its_framebuffer() {
        let Ok(Some(device)) = super::super::booted_device() else {
            return;
        };
        let frames = Arc::new(AtomicUsize::new(0));
        let counted = frames.clone();
        let feed = ScreenFeed::open(&device.udid, Delivery::Private, move |event| {
            if matches!(event, FeedEvent::Frame) {
                counted.fetch_add(1, Ordering::Relaxed);
            }
        })
        .expect("a feed from the booted device");
        let surface = feed.surface();
        let (width, height) = surface.size();
        assert!(width > 0 && height > width, "{width}×{height}");
        assert_eq!(surface.pixel_format(), BGRA);
        let pixels = surface.copy_bgra().expect("the pixels");
        assert_eq!(pixels.dimensions(), (width, height));
        // A change on screen arrives as frames.
        let appearance = super::super::controls::toggle_appearance(&device.udid).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let _ = super::super::appearance(
            Some(&device.udid),
            super::super::controls::other_appearance(appearance),
        );
        assert!(frames.load(Ordering::Relaxed) > 0, "no frames arrived");
        drop(feed);
    }
}
