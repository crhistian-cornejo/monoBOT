//! Where the device's screen sits in the pane, and which device point a
//! position on it is, for every zoom and orientation. The framebuffer keeps
//! its portrait layout when the device turns, so the picture is drawn
//! turned with the device and touches are turned back.
use gpui::{Bounds, Pixels, Point, point, px, size};
use monocode_core::simulator::controls::Orientation;

/// How big the screen is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Zoom {
    /// As large as the slot allows (⌘0).
    #[default]
    Fit,
    /// One device point per pixel of the pane (⌘1).
    Actual,
}

/// The screen's size as drawn: its points, turned with the device.
fn turned(points: (f32, f32), orientation: Orientation) -> (f32, f32) {
    if orientation.landscape() {
        (points.1, points.0)
    } else {
        points
    }
}

/// The rectangle the screen is drawn in, centred in `slot`. With `Fit` it
/// fills the slot keeping its proportions; with `Actual` it is the screen's
/// size in points and may overflow the slot, which clips it.
pub(crate) fn screen_rect(
    slot: Bounds<Pixels>,
    points: (f32, f32),
    orientation: Orientation,
    zoom: Zoom,
) -> Option<Bounds<Pixels>> {
    let (width, height) = turned(points, orientation);
    let (slot_width, slot_height) = (f32::from(slot.size.width), f32::from(slot.size.height));
    if width <= 0. || height <= 0. || slot_width <= 0. || slot_height <= 0. {
        return None;
    }
    let scale = match zoom {
        Zoom::Fit => (slot_width / width).min(slot_height / height),
        Zoom::Actual => 1.,
    };
    let (drawn_width, drawn_height) = (width * scale, height * scale);
    Some(Bounds::new(
        point(
            slot.origin.x + px((slot_width - drawn_width) / 2.),
            slot.origin.y + px((slot_height - drawn_height) / 2.),
        ),
        size(px(drawn_width), px(drawn_height)),
    ))
}

/// The device point (portrait coordinates, which touches take) under
/// `position`, when the screen is drawn in `rect`. `None` off the screen.
pub(crate) fn device_point(
    position: Point<Pixels>,
    rect: Bounds<Pixels>,
    points: (f32, f32),
    orientation: Orientation,
) -> Option<(f32, f32)> {
    let (width, height) = turned(points, orientation);
    let (rect_width, rect_height) = (f32::from(rect.size.width), f32::from(rect.size.height));
    if width <= 0. || height <= 0. || rect_width <= 0. || rect_height <= 0. {
        return None;
    }
    // Where on the turned picture, in device points.
    let u = (f32::from(position.x) - f32::from(rect.origin.x)) / rect_width * width;
    let v = (f32::from(position.y) - f32::from(rect.origin.y)) / rect_height * height;
    if u < 0. || v < 0. || u > width || v > height {
        return None;
    }
    let (w, h) = points;
    let (x, y) = match orientation.quarter_turns() {
        0 => (u, v),
        // Turned clockwise: the portrait top edge is on the right.
        1 => (v, h - u),
        2 => (w - u, h - v),
        _ => (w - v, u),
    };
    Some((x.round(), y.round()))
}

/// The inverse of [`device_point`]: where a device point is drawn, for the
/// inspector's outline.
pub(crate) fn view_point(
    device: (f32, f32),
    rect: Bounds<Pixels>,
    points: (f32, f32),
    orientation: Orientation,
) -> Point<Pixels> {
    let (width, height) = turned(points, orientation);
    let (w, h) = points;
    let (x, y) = device;
    let (u, v) = match orientation.quarter_turns() {
        0 => (x, y),
        1 => (h - y, x),
        2 => (w - x, h - y),
        _ => (y, w - x),
    };
    point(
        rect.origin.x + px(u / width.max(1.) * f32::from(rect.size.width)),
        rect.origin.y + px(v / height.max(1.) * f32::from(rect.size.height)),
    )
}

/// The rectangle a device frame (x, y, width, height in portrait points)
/// covers on the drawn screen.
pub(crate) fn view_rect(
    frame: (f32, f32, f32, f32),
    rect: Bounds<Pixels>,
    points: (f32, f32),
    orientation: Orientation,
) -> Bounds<Pixels> {
    let (x, y, width, height) = frame;
    let a = view_point((x, y), rect, points, orientation);
    let b = view_point((x + width, y + height), rect, points, orientation);
    Bounds::from_corners(
        point(a.x.min(b.x), a.y.min(b.y)),
        point(a.x.max(b.x), a.y.max(b.y)),
    )
}

/// Which coordinates a frame from the accessibility tree is in. Touches
/// always take the portrait screen's points, but an app whose interface
/// turned with the device reports its elements in its own, turned points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Space {
    /// The portrait screen's points.
    #[default]
    Portrait,
    /// The turned interface's points: the picture as drawn, upright.
    Upright,
}

impl Space {
    /// The space a tree is in: upright when the device is on its side and
    /// the app's root frame is too. The home screen does not turn.
    pub(crate) fn of(root: Option<(f32, f32, f32, f32)>, orientation: Orientation) -> Self {
        match root {
            Some((_, _, width, height)) if orientation.landscape() && width > height => {
                Self::Upright
            }
            _ => Self::Portrait,
        }
    }

    /// The point in this space under `position`.
    pub(crate) fn point(
        self,
        position: Point<Pixels>,
        rect: Bounds<Pixels>,
        points: (f32, f32),
        orientation: Orientation,
    ) -> Option<(f32, f32)> {
        match self {
            Self::Portrait => device_point(position, rect, points, orientation),
            Self::Upright => device_point(
                position,
                rect,
                turned(points, orientation),
                Orientation::Portrait,
            ),
        }
    }

    /// Where a frame in this space is drawn.
    pub(crate) fn rect(
        self,
        frame: (f32, f32, f32, f32),
        rect: Bounds<Pixels>,
        points: (f32, f32),
        orientation: Orientation,
    ) -> Bounds<Pixels> {
        match self {
            Self::Portrait => view_rect(frame, rect, points, orientation),
            Self::Upright => view_rect(
                frame,
                rect,
                turned(points, orientation),
                Orientation::Portrait,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHONE: (f32, f32) = (402., 874.);

    fn slot() -> Bounds<Pixels> {
        Bounds::new(point(px(10.), px(20.)), size(px(201.), px(874.)))
    }

    #[test]
    fn fit_fills_the_slot_and_actual_size_is_one_point_per_pixel() {
        // Half size: 201 × 437, centred with 218.5 px above.
        let fit = screen_rect(slot(), PHONE, Orientation::Portrait, Zoom::Fit).unwrap();
        assert_eq!(fit.origin, point(px(10.), px(20. + 218.5)));
        assert_eq!(fit.size, size(px(201.), px(437.)));
        let actual = screen_rect(slot(), PHONE, Orientation::Portrait, Zoom::Actual).unwrap();
        assert_eq!(actual.size, size(px(402.), px(874.)));
        // Wider than the slot, centred: it overflows on both sides.
        assert_eq!(actual.origin.x, px(10. - 100.5));
        // Landscape fits by width.
        let wide = screen_rect(slot(), PHONE, Orientation::LandscapeLeft, Zoom::Fit).unwrap();
        assert_eq!(f32::from(wide.size.width), 201.);
        assert!((f32::from(wide.size.height) - 201. * 402. / 874.).abs() < 0.01);
        assert!(screen_rect(slot(), (0., 0.), Orientation::Portrait, Zoom::Fit).is_none());
    }

    #[test]
    fn a_click_lands_on_the_device_point_under_it() {
        let rect = screen_rect(slot(), PHONE, Orientation::Portrait, Zoom::Fit).unwrap();
        assert_eq!(
            device_point(rect.origin, rect, PHONE, Orientation::Portrait),
            Some((0., 0.))
        );
        assert_eq!(
            device_point(
                point(px(110.5), px(20. + 437.)),
                rect,
                PHONE,
                Orientation::Portrait
            ),
            Some((201., 437.))
        );
        // Above the picture, in the letterbox.
        assert_eq!(
            device_point(point(px(50.), px(30.)), rect, PHONE, Orientation::Portrait),
            None
        );
    }

    #[test]
    fn turned_screens_map_back_to_portrait_points_and_round_trip() {
        let corner = |o: Orientation| {
            let rect = screen_rect(slot(), PHONE, o, Zoom::Fit).unwrap();
            // The drawn picture's top-left corner.
            device_point(rect.origin, rect, PHONE, o).unwrap()
        };
        // Turned clockwise, the portrait bottom-left corner is on top.
        assert_eq!(corner(Orientation::LandscapeRight), (0., 874.));
        assert_eq!(corner(Orientation::PortraitUpsideDown), (402., 874.));
        assert_eq!(corner(Orientation::LandscapeLeft), (402., 0.));
        for o in [
            Orientation::Portrait,
            Orientation::LandscapeRight,
            Orientation::PortraitUpsideDown,
            Orientation::LandscapeLeft,
        ] {
            let rect = screen_rect(slot(), PHONE, o, Zoom::Actual).unwrap();
            for device in [(10., 20.), (300., 800.), (201., 437.)] {
                let shown = view_point(device, rect, PHONE, o);
                assert_eq!(device_point(shown, rect, PHONE, o), Some(device), "{o:?}");
            }
        }
    }

    #[test]
    fn a_turned_interface_reports_frames_in_its_own_points() {
        // Safari on its side: a root frame wider than tall.
        let landscape = Orientation::LandscapeRight;
        assert_eq!(
            Space::of(Some((0., 0., 874., 402.)), landscape),
            Space::Upright
        );
        // The home screen stays portrait on a turned device.
        assert_eq!(
            Space::of(Some((0., 0., 402., 874.)), landscape),
            Space::Portrait
        );
        assert_eq!(
            Space::of(Some((0., 0., 874., 402.)), Orientation::Portrait),
            Space::Portrait
        );
        let rect = screen_rect(slot(), PHONE, landscape, Zoom::Actual).unwrap();
        // Safari's tab button: (780, 32) in its points, drawn where it is.
        let button = view_point((780., 32.), rect, (874., 402.), Orientation::Portrait);
        assert_eq!(
            Space::Upright.point(button, rect, PHONE, landscape),
            Some((780., 32.))
        );
        // The same spot is (32, 94) on the portrait screen, which taps take:
        // measured on an iPhone 18 Pro simulator, this tap opens the tabs.
        assert_eq!(
            Space::Portrait.point(button, rect, PHONE, landscape),
            Some((32., 94.))
        );
        let outline = Space::Upright.rect((758., 10., 44., 44.), rect, PHONE, landscape);
        assert_eq!(outline.size, size(px(44.), px(44.)));
        assert!(outline.contains(&button));
    }

    #[test]
    fn an_element_frame_becomes_its_outline_on_screen() {
        let rect = screen_rect(slot(), PHONE, Orientation::Portrait, Zoom::Actual).unwrap();
        let outline = view_rect((10., 20., 100., 40.), rect, PHONE, Orientation::Portrait);
        assert_eq!(
            outline.origin,
            point(rect.origin.x + px(10.), rect.origin.y + px(20.))
        );
        assert_eq!(outline.size, size(px(100.), px(40.)));
        let wide = screen_rect(slot(), PHONE, Orientation::LandscapeRight, Zoom::Actual).unwrap();
        let turned = view_rect(
            (10., 20., 100., 40.),
            wide,
            PHONE,
            Orientation::LandscapeRight,
        );
        assert_eq!(turned.size, size(px(40.), px(100.)));
    }
}
