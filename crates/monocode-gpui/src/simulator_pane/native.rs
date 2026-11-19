//! The device's screen as a native layer placed over an empty, measured
//! slot in the pane: the framebuffer IOSurface is the layer's contents, and
//! every frame the device draws only marks the layer changed. No copies,
//! encoding or decoding, as Simulator.app shows it.
//!
//! The pane draws the slot (and the bezel around it) in GPUI; the canvas in
//! the slot moves the native view to the slot's bounds on every paint, and
//! the pane hides it when the slot is not painted. The view lets every
//! click through, so GPUI's handlers on the slot still tap and swipe.
//! Anything GPUI draws over the slot (a menu, a modal) sits under it.
use super::geometry::{self, Zoom};
use gpui::{AnyElement, Bounds, Pixels, canvas, prelude::*};
use monocode_core::simulator::{
    controls::Orientation,
    native::{FeedEvent, Surface},
};
use objc2::{
    MainThreadMarker, MainThreadOnly, class, define_class, msg_send,
    rc::{Allocated, Retained},
    runtime::{AnyObject, Bool, NSObjectProtocol},
    sel,
};
use objc2_app_kit::{NSView, NSWindowOrderingMode};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

define_class!(
    #[unsafe(super(NSView))]
    #[name = "MonocodeSimulatorScreen"]
    #[thread_kind = MainThreadOnly]
    struct ScreenClip;
    unsafe impl NSObjectProtocol for ScreenClip {}
    impl ScreenClip {
        /// Clicks and drags go through to GPUI, which turns them into
        /// touches.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

/// The rounding of an iPhone's screen corners, as a share of its short side.
const CORNER: f64 = 0.137;

thread_local! {
    /// Each hosted screen's layer, by id: the feed's calls arrive on the
    /// main thread with only the id, which crosses threads freely.
    static LAYERS: RefCell<HashMap<u64, Retained<AnyObject>>> = RefCell::new(HashMap::new());
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

struct Hosted {
    clip: Retained<ScreenClip>,
    layer: Retained<AnyObject>,
}

/// What the canvas needs to place the screen.
#[derive(Clone, Copy)]
pub(super) struct Placement {
    pub points: (f32, f32),
    pub orientation: Orientation,
    pub zoom: Zoom,
}

pub(super) struct NativeScreen {
    id: u64,
    host: Rc<RefCell<Option<Hosted>>>,
    surface: Rc<RefCell<Option<Surface>>>,
}

/// Runs CoreAnimation changes without the implicit quarter-second fade.
fn quietly(change: impl FnOnce()) {
    unsafe {
        let _: () = msg_send![class!(CATransaction), begin];
        let _: () = msg_send![class!(CATransaction), setDisableActions: true];
    }
    change();
    unsafe {
        let _: () = msg_send![class!(CATransaction), commit];
    }
}

fn set_contents(layer: &AnyObject, surface: &Surface) {
    let contents = surface.as_ptr().cast::<AnyObject>();
    quietly(|| unsafe {
        let _: () = msg_send![layer, setContents: contents];
    });
}

/// Shows the device's newest frame: the surface's pixels changed in place.
fn redraw(layer: &AnyObject) {
    let responds: Bool = unsafe { msg_send![layer, respondsToSelector: sel!(setContentsChanged)] };
    quietly(|| unsafe {
        if responds.as_bool() {
            let _: () = msg_send![layer, setContentsChanged];
        } else {
            let contents: *mut AnyObject = msg_send![layer, contents];
            let _: () = msg_send![layer, setContents: std::ptr::null_mut::<AnyObject>()];
            let _: () = msg_send![layer, setContents: contents];
        }
    });
}

impl NativeScreen {
    pub(super) fn new() -> Self {
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            host: Rc::default(),
            surface: Rc::default(),
        }
    }

    /// What the core feed calls, on the main thread, for this screen.
    pub(super) fn handler(&self) -> impl Fn(FeedEvent) + Send + Sync + 'static {
        let id = self.id;
        move |event| {
            if MainThreadMarker::new().is_none() {
                return;
            }
            LAYERS.with(|layers| {
                let layers = layers.borrow();
                let Some(layer) = layers.get(&id) else { return };
                match event {
                    FeedEvent::Frame => redraw(layer),
                    FeedEvent::Surface(surface) => set_contents(layer, &surface),
                }
            });
        }
    }

    /// The surface to show; the feed's swaps replace it on the layer.
    pub(super) fn set_surface(&self, surface: Option<Surface>) {
        if let (Some(host), Some(surface)) = (self.host.borrow().as_ref(), surface.as_ref()) {
            set_contents(&host.layer, surface);
        }
        *self.surface.borrow_mut() = surface;
    }

    /// Takes the view off the window until the slot is painted again.
    pub(super) fn hide(&self) {
        if let Some(host) = self.host.borrow().as_ref() {
            host.clip.setHidden(true);
        }
    }

    /// The slot: an empty canvas whose paint places the native view.
    pub(super) fn slot(&self, placement: Placement) -> AnyElement {
        let host = self.host.clone();
        let surface = self.surface.clone();
        let id = self.id;
        canvas(
            |_, _, _| {},
            move |slot, _, window, _| {
                let Some(surface) = surface.borrow().clone() else {
                    return;
                };
                let Ok(handle) = HasWindowHandle::window_handle(window) else {
                    return;
                };
                let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
                    return;
                };
                let Some(mtm) = MainThreadMarker::new() else {
                    return;
                };
                let parent = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
                place(&host, id, parent, mtm, &surface, slot, window, placement);
            },
        )
        .size_full()
        .into_any_element()
    }
}

#[allow(clippy::too_many_arguments)]
fn place(
    host: &Rc<RefCell<Option<Hosted>>>,
    id: u64,
    parent: &NSView,
    mtm: MainThreadMarker,
    surface: &Surface,
    slot: Bounds<Pixels>,
    window: &gpui::Window,
    placement: Placement,
) {
    let mut host = host.borrow_mut();
    let Some(screen) = geometry::screen_rect(
        slot,
        placement.points,
        placement.orientation,
        placement.zoom,
    ) else {
        if let Some(host) = host.as_ref() {
            host.clip.setHidden(true);
        }
        return;
    };
    // AppKit frames are in window points; GPUI's are divided by the zoom.
    let zoom = window.content_zoom();
    let visible = slot
        .intersect(&window.content_mask().bounds)
        .intersect(&screen)
        .map(|v| v * zoom);
    let screen = screen.map(|v| v * zoom);
    if visible.size.width <= gpui::px(0.) || visible.size.height <= gpui::px(0.) {
        if let Some(host) = host.as_ref() {
            host.clip.setHidden(true);
        }
        return;
    }
    let y = if parent.isFlipped() {
        f64::from(visible.top())
    } else {
        parent.bounds().size.height - f64::from(visible.bottom())
    };
    let clip_rect = NSRect::new(
        NSPoint::new(f64::from(visible.left()), y),
        NSSize::new(
            f64::from(visible.size.width),
            f64::from(visible.size.height),
        ),
    );
    if host.is_none() {
        let allocated: Allocated<ScreenClip> = ScreenClip::alloc(mtm);
        let clip: Retained<ScreenClip> = unsafe { msg_send![allocated, initWithFrame: clip_rect] };
        unsafe {
            // A layer-hosting view: the root layer is ours, y up.
            let root: Retained<AnyObject> = msg_send![class!(CALayer), new];
            let layer: Retained<AnyObject> = msg_send![class!(CALayer), new];
            let _: () = msg_send![&layer, setContentsGravity: &*NSString::from_str("resize")];
            let _: () = msg_send![&layer, setMinificationFilter: &*NSString::from_str("trilinear")];
            let _: () = msg_send![&layer, setMasksToBounds: true];
            let _: () = msg_send![&root, addSublayer: &*layer];
            let _: () = msg_send![&clip, setLayer: &*root];
            clip.setWantsLayer(true);
            set_contents(&layer, surface);
            LAYERS.with(|layers| layers.borrow_mut().insert(id, layer.clone()));
            *host = Some(Hosted { clip, layer });
        }
    }
    let Some(host) = host.as_ref() else { return };
    if unsafe { host.clip.superview() }
        .as_deref()
        .is_none_or(|view| !std::ptr::eq(view, parent))
    {
        parent.addSubview_positioned_relativeTo(&host.clip, NSWindowOrderingMode::Above, None);
    }
    if host.clip.frame() != clip_rect {
        host.clip.setFrame(clip_rect);
    }
    if host.clip.isHidden() {
        host.clip.setHidden(false);
    }
    // The screen inside the clip, in the clip's y-up coordinates. The
    // picture is portrait; it turns about its centre with the device.
    let (drawn_w, drawn_h) = (f64::from(screen.size.width), f64::from(screen.size.height));
    let (portrait_w, portrait_h) = if placement.orientation.landscape() {
        (drawn_h, drawn_w)
    } else {
        (drawn_w, drawn_h)
    };
    let center_x = f64::from(screen.left() - visible.left()) + drawn_w / 2.;
    let center_y = f64::from(visible.bottom() - screen.bottom()) + drawn_h / 2.;
    let turns = f64::from(placement.orientation.quarter_turns());
    let layer = &host.layer;
    quietly(|| unsafe {
        let bounds = NSRect::new(NSPoint::new(0., 0.), NSSize::new(portrait_w, portrait_h));
        let _: () = msg_send![layer, setBounds: bounds];
        let _: () = msg_send![layer, setPosition: NSPoint::new(center_x, center_y)];
        let _: () = msg_send![layer, setCornerRadius: CORNER * portrait_w.min(portrait_h)];
        // Clockwise on screen is a negative angle with y up.
        let angle: Retained<AnyObject> = msg_send![
            class!(NSNumber),
            numberWithDouble: -turns * std::f64::consts::FRAC_PI_2
        ];
        let _: () = msg_send![
            layer,
            setValue: &*angle,
            forKeyPath: &*NSString::from_str("transform.rotation.z")
        ];
    });
}

impl Drop for NativeScreen {
    fn drop(&mut self) {
        LAYERS.with(|layers| layers.borrow_mut().remove(&self.id));
        if let Some(host) = self.host.borrow_mut().take() {
            host.clip.removeFromSuperview();
        }
    }
}
