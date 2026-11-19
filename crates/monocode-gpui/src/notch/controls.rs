//! The notch's controls, in the macOS idiom the workspace already speaks
//! (`theme::icon_button`, `theme::switch`, `theme::primary_action`, AppKit
//! menus through `native_menu`), adapted to the island's black: rounded
//! push buttons that dim while pressed, borderless icon buttons with a hover
//! plate, a segmented control whose raised plate glides to the chosen
//! segment, the system's switch, and a glass card for widgets and previews.
//!
//! Radii are the ones a native control asks for; the window's corner scale
//! (Look › Corners) scales them like every other radius in the notch.
use super::*;
use gpui::SharedString;

/// A push button's radius.
pub(crate) const BUTTON_RADIUS: f32 = 7.;
/// A row's hover and selection plate.
pub(crate) const ROW_RADIUS: f32 = 6.;
/// A card: a widget, the clipboard's preview, a player column.
pub(crate) const CARD_RADIUS: f32 = 11.;
/// The segmented control's track, and its segments inside it.
const TRACK_RADIUS: f32 = 8.;
const TRACK_PAD: f32 = 2.;
const SEGMENT_GAP: f32 = 2.;
/// How long the segmented control's plate takes to reach a new segment.
const GLIDE: Duration = Duration::from_millis(180);

/// Glass on the island's black, as macOS draws its dark materials: a fill
/// that is a little lighter at the top than at the bottom, and a hairline
/// edge that catches the light. The island is black and has no desktop
/// behind it to blur, so the depth comes from the light, not from a blur.
pub(crate) fn glass<E: Styled>(element: E) -> E {
    element
        .rounded(px(CARD_RADIUS))
        .border_1()
        .border_color(ink(0.075))
        .bg(gpui::linear_gradient(
            180.,
            gpui::linear_color_stop(ink(0.095), 0.),
            gpui::linear_color_stop(ink(0.05), 1.),
        ))
}

/// A rounded push button. The primary one is the accent with the ink the
/// accent asks for (black on a light accent), as `.primary-action` is in
/// the workspace; the others are a quiet fill. Both dim while pressed.
pub(crate) fn push_button(
    id: impl Into<gpui::ElementId>,
    label: &str,
    primary: bool,
) -> gpui::Stateful<gpui::Div> {
    let id = id.into();
    let selector = format!("{id}");
    let (fill, text) = if primary {
        crate::theme::primary_action(true).unwrap_or((accent_color(), white(1.)))
    } else {
        (ink(0.11), ink(0.88))
    };
    div()
        .id(id)
        .debug_selector(move || selector.clone())
        .role(Role::Button)
        .aria_label(label.to_owned())
        .h(px(22.))
        .px(px(10.))
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .rounded(px(BUTTON_RADIUS))
        .text_size(px(11.5))
        .font_weight(gpui::FontWeight::MEDIUM)
        .cursor_pointer()
        .bg(fill)
        .text_color(text)
        .when(primary, |button| {
            button
                .hover(|style| style.opacity(0.9))
                .active(|style| style.opacity(0.72))
        })
        .when(!primary, |button| {
            button
                .hover(|style| style.bg(ink(0.17)))
                .active(|style| style.bg(ink(0.24)))
        })
        // A button that changes its word (Copy to Copied) grows the letters.
        .child(crate::text_morph::text_morph("label", label.to_owned()))
}

/// A borderless icon button, as the workspace's chrome buttons: the icon
/// brightens and a plate shows under the pointer, a deeper one while
/// pressed, and `on` keeps the plate for a button that is a state.
pub(crate) fn icon_button(
    id: &'static str,
    label: &'static str,
    path: &'static str,
    on: bool,
) -> gpui::Stateful<gpui::Div> {
    let group: gpui::SharedString = format!("{id}-group").into();
    div()
        .id(id)
        .debug_selector(move || id.into())
        .role(Role::Button)
        .aria_label(label)
        .group(group.clone())
        .size(px(26.))
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .rounded(px(BUTTON_RADIUS))
        .cursor_pointer()
        .when(on, |button| button.bg(ink(0.14)))
        .hover(|style| style.bg(ink(0.1)))
        .active(|style| style.bg(ink(0.18)))
        .child(
            icon(path, 14.)
                .text_color(ink(if on { 1. } else { 0.55 }))
                .group_hover(group, |style| style.text_color(ink(0.95))),
        )
}

/// The system's switch on the island: the accent when on, a grey track
/// when off, its knob at the side it is set to. `theme::switch` follows the
/// workspace's ink, which a light theme makes dark; the island is always
/// black.
pub(crate) fn switch(
    id: impl Into<SharedString>,
    label: &str,
    on: bool,
) -> gpui::Stateful<gpui::Div> {
    let id = id.into();
    let selector = id.to_string();
    crate::theme::switch(id, label.to_owned(), on)
        .debug_selector(move || selector.clone())
        .when(!on, |track| track.bg(ink(0.22)))
}

/// One segment of a [`segmented`] control.
pub(crate) struct Segment {
    pub id: SharedString,
    pub label: SharedString,
    /// An icon instead of the label, which is then only read aloud.
    pub icon: Option<&'static str>,
    /// The width the segment takes.
    pub width: f32,
}

impl Segment {
    pub(crate) fn icon(
        id: impl Into<SharedString>,
        label: &'static str,
        path: &'static str,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: Some(path),
            width: 26.,
        }
    }
}

/// Where the plate of a segmented control was before its last change, so it
/// glides from there. One per control, kept by its owner.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Glide {
    from: Option<(usize, Instant)>,
}

impl Glide {
    /// The chosen segment moved from `from`.
    pub(crate) fn moved(&mut self, from: usize) {
        self.from = Some((from, Instant::now()));
    }
}

/// The left edge of segment `index` inside the track.
fn segment_left(segments: &[Segment], index: usize) -> f32 {
    TRACK_PAD
        + segments
            .iter()
            .take(index)
            .map(|segment| segment.width + SEGMENT_GAP)
            .sum::<f32>()
}

/// A segmented control, AppKit's "select one" style: a sunken track and a
/// raised plate under the chosen segment, which glides there from the one
/// chosen before (drawn in place under reduced motion). Each segment gets
/// `on_pick` with its index.
pub(crate) fn segmented(
    id: impl Into<SharedString>,
    segments: Vec<Segment>,
    chosen: usize,
    glide: Glide,
    reduce_motion: bool,
    on_pick: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let id = id.into();
    let on_pick = Rc::new(on_pick);
    let to = segment_left(&segments, chosen);
    let width = |index: usize| segments.get(index).map_or(26., |segment| segment.width);
    let (left, plate_width, gliding) = match glide.from {
        Some((from, at)) if !reduce_motion && from != chosen => {
            let t = (at.elapsed().as_secs_f32() / GLIDE.as_secs_f32()).clamp(0., 1.);
            // Ease out: quick to leave, gentle to land.
            let eased = 1. - (1. - t).powi(3);
            let from_left = segment_left(&segments, from);
            (
                from_left + (to - from_left) * eased,
                width(from) + (width(chosen) - width(from)) * eased,
                t < 1.,
            )
        }
        _ => (to, width(chosen), false),
    };
    let height = 22.;
    let mut track = div()
        .id(id)
        .role(Role::TabList)
        .relative()
        .h(px(height + 2. * TRACK_PAD))
        .p(px(TRACK_PAD))
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(SEGMENT_GAP))
        .rounded(px(TRACK_RADIUS))
        .bg(ink(0.07))
        .child(
            div()
                .absolute()
                .top(px(TRACK_PAD))
                .left(px(left))
                .w(px(plate_width))
                .h(px(height))
                .rounded(px(TRACK_RADIUS - TRACK_PAD))
                .bg(ink(0.2))
                .shadow(vec![
                    gpui::BoxShadow::new(px(0.), px(1.), black(0.35)).blur_radius(px(2.)),
                ]),
        );
    if gliding {
        // Repaints until the plate lands; nothing runs once it has.
        track = track.child(
            canvas(
                |_, _, _| (),
                |_, _, window, _| window.request_animation_frame(),
            )
            .absolute()
            .size_0(),
        );
    }
    for (index, segment) in segments.into_iter().enumerate() {
        let picked = index == chosen;
        let on_pick = on_pick.clone();
        let selector = segment.id.to_string();
        track = track.child(
            div()
                .id(segment.id.clone())
                .debug_selector(move || selector.clone())
                .role(Role::Tab)
                .aria_label(segment.label.clone())
                .aria_selected(picked)
                .relative()
                .w(px(segment.width))
                .h(px(height))
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .rounded(px(TRACK_RADIUS - TRACK_PAD))
                .cursor_pointer()
                .text_size(px(11.5))
                .text_color(ink(if picked { 1. } else { 0.6 }))
                .when(!picked, |segment| {
                    segment.hover(|style| style.text_color(ink(0.92)))
                })
                .map(|element| match segment.icon {
                    Some(path) => element.child(icon(path, 13.).text_color(ink(if picked {
                        1.
                    } else {
                        0.55
                    }))),
                    None => element.child(segment.label.clone()),
                })
                .on_click(move |_, window, cx| on_pick(index, window, cx)),
        );
    }
    track
}
