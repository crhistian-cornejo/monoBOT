//! A short label that changes without a jump. The letters both texts share
//! slide to their new places, the ones that go drift up and fade, and the new
//! ones rise in: "Copy" becomes "Copied" by growing three letters, and a
//! clock's minute rolls. It is for words the eye rests on (a state, a count,
//! a time), not for text that changes many times a second.
//!
//! The element takes its final size at once and moves only what it paints,
//! on the shared clock (`pulse`), for `motion::NORMAL`. It reads its font,
//! size and color from the text style around it, like plain text, and cuts a
//! line too long for its room with an ellipsis. Under reduced motion the new
//! text is simply there.
use crate::motion;
use gpui::{
    App, Bounds, Element, ElementId, FontId, GlobalElementId, GlyphId, Hsla, InspectorElementId,
    IntoElement, LayoutId, Pixels, Point, ShapedLine, SharedString, Style, TextRun, TruncateFrom,
    Window, point, px, relative,
};
use std::time::Instant;

/// How far a letter travels as it leaves or arrives, as a share of its size.
const DRIFT: f32 = 0.32;
/// A leaving letter is gone by this share of the change.
const GONE_BY: f32 = 0.6;
/// An arriving letter starts at this share of it, once there is room.
const ARRIVES_FROM: f32 = 0.25;
/// Longer texts than this just swap: they are sentences, not labels.
const LONGEST: usize = 64;

/// `text` as a label that morphs when it changes. `id` tells one label from
/// another under the same parent; what was shown before is kept by it.
pub(crate) fn text_morph(id: impl Into<ElementId>, text: impl Into<SharedString>) -> TextMorph {
    TextMorph {
        id: id.into(),
        text: text.into(),
        shimmer: None,
        within: None,
    }
}

/// A sweep of light: the seconds on its clock, and the ink it shades.
type Sweep = (f32, fn(f32) -> Hsla);

pub(crate) struct TextMorph {
    id: ElementId,
    text: SharedString,
    shimmer: Option<Sweep>,
    within: Option<Pixels>,
}

impl TextMorph {
    /// Never wider than `width`: a longer text is cut with an ellipsis. For a
    /// label whose parent takes its width from it, where "all the room there
    /// is" says nothing.
    pub(crate) fn within(mut self, width: Pixels) -> Self {
        self.within = Some(width);
        self
    }

    /// Sweeps the label with light, as `activity::shimmer_in` does: `elapsed`
    /// seconds on the sweep's clock, in `ink`.
    pub(crate) fn shimmer(mut self, elapsed: f32, ink: fn(f32) -> Hsla) -> Self {
        self.shimmer = Some((elapsed, ink));
        self
    }
}

/// One glyph of a laid out line.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Mark {
    letter: char,
    x: f32,
    y: f32,
    font: FontId,
    glyph: GlyphId,
    emoji: bool,
}

/// A glyph in one frame: where it is and how much ink it has.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Drawn {
    mark: Mark,
    x: f32,
    /// Below its line when positive.
    drop: f32,
    alpha: f32,
}

struct Change {
    start: Instant,
    old: Vec<Mark>,
    /// Where the old text was painted, so a letter that stays keeps its
    /// place on screen even when the label's box moved with its new width.
    origin: Point<Pixels>,
    /// For each new glyph, the old one it carries on from.
    from: Vec<Option<usize>>,
}

struct State {
    text: SharedString,
    marks: Vec<Mark>,
    origin: Point<Pixels>,
    change: Option<Change>,
}

pub(crate) struct Frame {
    drawn: Vec<Drawn>,
    baseline: Pixels,
    size: Pixels,
    width: f32,
    color: Hsla,
}

/// For each letter of `new`, the letter of `old` it carries on from: the
/// longest run of letters both share in the same order, so what stays keeps
/// its order and only slides.
fn carried(old: &[char], new: &[char]) -> Vec<Option<usize>> {
    let (n, m) = (old.len(), new.len());
    // lengths[i][j]: the longest shared run of old[i..] and new[j..].
    let mut lengths = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lengths[i][j] = if old[i] == new[j] {
                lengths[i + 1][j + 1] + 1
            } else {
                lengths[i + 1][j].max(lengths[i][j + 1])
            };
        }
    }
    let mut from = vec![None; m];
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if old[i] == new[j] {
            from[j] = Some(i);
            i += 1;
            j += 1;
        } else if lengths[i + 1][j] >= lengths[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    from
}

/// The glyphs to paint `t` (0 to 1) into a change from `old` to `new`. `shift`
/// is how far left of the new text the old one was painted.
fn frame(
    old: &[Mark],
    new: &[Mark],
    from: &[Option<usize>],
    shift: f32,
    size: f32,
    t: f32,
) -> Vec<Drawn> {
    let slide = motion::ease(t, motion::EMPHASIZED);
    let gone = motion::ease(t / GONE_BY, motion::EASE_OUT);
    let here = motion::ease((t - ARRIVES_FROM) / (1. - ARRIVES_FROM), motion::EASE_OUT);
    let mut kept = vec![false; old.len()];
    let mut drawn = Vec::with_capacity(old.len() + new.len());
    for (mark, from) in new.iter().zip(from) {
        match from.and_then(|at| old.get(at).map(|old| (at, old))) {
            Some((at, old)) => {
                kept[at] = true;
                let start = old.x - shift;
                drawn.push(Drawn {
                    mark: *mark,
                    x: start + (mark.x - start) * slide,
                    drop: 0.,
                    alpha: 1.,
                });
            }
            None => drawn.push(Drawn {
                mark: *mark,
                x: mark.x,
                drop: DRIFT * size * (1. - here),
                alpha: here,
            }),
        }
    }
    for (mark, kept) in old.iter().zip(kept) {
        if !kept && gone < 1. {
            drawn.push(Drawn {
                mark: *mark,
                x: mark.x - shift,
                drop: -DRIFT * size * gone,
                alpha: 1. - gone,
            });
        }
    }
    drawn
}

fn at_rest(marks: &[Mark]) -> Vec<Drawn> {
    marks
        .iter()
        .map(|mark| Drawn {
            mark: *mark,
            x: mark.x,
            drop: 0.,
            alpha: 1.,
        })
        .collect()
}

fn marks(line: &ShapedLine) -> Vec<Mark> {
    line.runs
        .iter()
        .flat_map(|run| {
            run.glyphs.iter().map(|glyph| Mark {
                letter: line
                    .text
                    .get(glyph.index..)
                    .and_then(|rest| rest.chars().next())
                    .unwrap_or(' '),
                x: f32::from(glyph.position.x),
                y: f32::from(glyph.position.y),
                font: run.font_id,
                glyph: glyph.id,
                emoji: glyph.is_emoji,
            })
        })
        .collect()
}

fn letters(marks: &[Mark]) -> Vec<char> {
    marks.iter().map(|mark| mark.letter).collect()
}

impl TextMorph {
    fn shape(&self, text: SharedString, window: &Window) -> (ShapedLine, TextRun, Pixels) {
        let style = window.text_style();
        let size = style.font_size.to_pixels(window.rem_size());
        let run = TextRun {
            len: text.len(),
            font: style.font(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = window
            .text_system()
            .shape_line(text, size, std::slice::from_ref(&run), None);
        (line, run, size)
    }
}

impl IntoElement for TextMorph {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for TextMorph {
    type RequestLayoutState = ();
    type PrepaintState = Frame;
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let (line, ..) = self.shape(self.text.clone(), window);
        let style = window.text_style();
        let mut layout = Style::default();
        // The new text's own width at once; narrower only when the row has
        // no room for it.
        layout.size.width = line.width.into();
        // The height plain text gives its line, snapped the way it snaps.
        let size = style.font_size.to_pixels(window.rem_size());
        layout.size.height = window
            .pixel_snap(style.line_height.to_pixels(size.into(), window.rem_size()))
            .into();
        layout.min_size.width = px(0.).into();
        layout.max_size.width = match self.within {
            Some(width) => width.into(),
            None => relative(1.).into(),
        };
        layout.flex_shrink = 1.;
        (window.request_layout(layout, [], cx), ())
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Frame {
        let (mut line, run, size) = self.shape(self.text.clone(), window);
        if line.width > bounds.size.width + px(0.5) {
            let mut wrapper = window.text_system().line_wrapper(run.font.clone(), size);
            let (cut, _) = wrapper.truncate_line(
                self.text.clone(),
                bounds.size.width,
                "…",
                std::slice::from_ref(&run),
                TruncateFrom::End,
            );
            drop(wrapper);
            line = self.shape(cut, window).0;
        }
        let line_height = bounds.size.height;
        let baseline = (line_height - line.ascent - line.descent) / 2. + line.ascent;
        let marks = marks(&line);
        let animate = !cx.reduce_motion();
        let now = Instant::now();
        let (drawn, moving) = window.with_element_state(
            id.expect("a text morph has an id"),
            |state: Option<State>, _| {
                let mut state = state.unwrap_or_else(|| State {
                    text: self.text.clone(),
                    marks: marks.clone(),
                    origin: bounds.origin,
                    change: None,
                });
                if state.text != self.text {
                    let old = std::mem::take(&mut state.marks);
                    let fits = old.len() <= LONGEST && marks.len() <= LONGEST;
                    state.change = (animate && fits).then(|| Change {
                        start: now,
                        from: carried(&letters(&old), &letters(&marks)),
                        old,
                        origin: state.origin,
                    });
                    state.text = self.text.clone();
                }
                let t = state.change.as_ref().map(|change| {
                    now.duration_since(change.start).as_secs_f32() / motion::NORMAL.as_secs_f32()
                });
                if t.is_some_and(|t| t >= 1.) || !animate {
                    state.change = None;
                }
                let drawn = match (&state.change, t) {
                    (Some(change), Some(t)) => {
                        // A label that moved to another row starts from
                        // where it is now, not from across the panel.
                        let same_row = (change.origin.y - bounds.origin.y).abs() < px(1.);
                        let shift = if same_row {
                            f32::from(bounds.origin.x - change.origin.x)
                        } else {
                            0.
                        };
                        frame(&change.old, &marks, &change.from, shift, f32::from(size), t)
                    }
                    _ => at_rest(&marks),
                };
                let moving = state.change.is_some();
                state.marks = marks;
                state.origin = bounds.origin;
                ((drawn, moving), state)
            },
        );
        // The change, and the sweep of light, move on the shared clock.
        if moving || (self.shimmer.is_some() && animate) {
            crate::pulse::request(crate::pulse::Rate::Smooth, window, cx);
        }
        Frame {
            drawn,
            baseline,
            size,
            width: f32::from(line.width).max(1.),
            color: window.text_style().color,
        }
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        frame: &mut Frame,
        window: &mut Window,
        _: &mut App,
    ) {
        let size = f32::from(frame.size);
        window.paint_layer(bounds, |window| {
            for drawn in &frame.drawn {
                let origin = bounds.origin
                    + point(px(drawn.x), frame.baseline + px(drawn.mark.y + drawn.drop));
                if drawn.mark.emoji {
                    let _ =
                        window.paint_emoji(origin, drawn.mark.font, drawn.mark.glyph, frame.size);
                    continue;
                }
                let ink = match self.shimmer {
                    Some((elapsed, ink)) => ink(0.4
                        + 0.6
                            * crate::activity::sweep(
                                (drawn.x + size * 0.25) / frame.width,
                                elapsed,
                            )),
                    None => frame.color,
                };
                let _ = window.paint_glyph(
                    origin,
                    drawn.mark.font,
                    drawn.mark.glyph,
                    frame.size,
                    ink.opacity(drawn.alpha),
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext, div, prelude::*};

    fn chars(text: &str) -> Vec<char> {
        text.chars().collect()
    }

    fn mark(letter: char, x: f32) -> Mark {
        Mark {
            letter,
            x,
            y: 0.,
            font: FontId(0),
            glyph: GlyphId(letter as u32),
            emoji: false,
        }
    }

    fn line(text: &str) -> Vec<Mark> {
        text.chars()
            .enumerate()
            .map(|(at, letter)| mark(letter, at as f32 * 10.))
            .collect()
    }

    #[test]
    fn letters_both_texts_share_carry_on_in_order() {
        // "Copy" grows into "Copied": the first three letters stay.
        assert_eq!(
            carried(&chars("Copy"), &chars("Copied")),
            vec![Some(0), Some(1), Some(2), None, None, None]
        );
        // A clock's minute: only the last digit is new.
        assert_eq!(
            carried(&chars("12:04"), &chars("12:05")),
            vec![Some(0), Some(1), Some(2), Some(3), None]
        );
        // Nothing shared, nothing carried; empty texts are fine.
        assert_eq!(carried(&chars("abc"), &chars("xyz")), vec![None; 3]);
        assert!(carried(&chars("abc"), &[]).is_empty());
        assert_eq!(carried(&[], &chars("ab")), vec![None, None]);
        // What stays never crosses: each carried letter comes from further
        // along the old text than the one before it.
        let from = carried(&chars("Thinking"), &chars("Editing"));
        let kept: Vec<usize> = from.into_iter().flatten().collect();
        assert!(kept.len() >= 3 && kept.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn a_change_slides_what_stays_and_trades_the_rest() {
        let (old, new) = (line("ab"), line("xb"));
        let from = carried(&letters(&old), &letters(&new));
        assert_eq!(from, vec![None, Some(1)]);
        // At the start the old text is whole and the new letter unseen.
        let start = frame(&old, &new, &from, 0., 10., 0.);
        let seen = |drawn: &[Drawn], letter| {
            drawn
                .iter()
                .find(|drawn| drawn.mark.letter == letter)
                .copied()
        };
        assert_eq!(seen(&start, 'a').map(|a| a.alpha), Some(1.));
        assert_eq!(seen(&start, 'x').map(|x| x.alpha), Some(0.));
        assert!(seen(&start, 'x').unwrap().drop > 0., "it rises from below");
        // Halfway the leaving letter is on its way up and nearly gone.
        let middle = frame(&old, &new, &from, 0., 10., 0.5);
        let leaving = seen(&middle, 'a').unwrap();
        assert!(leaving.alpha < 0.2 && leaving.drop < 0.);
        // At the end only the new text is left, where plain text would be.
        let end = frame(&old, &new, &from, 0., 10., 1.);
        assert_eq!(end, at_rest(&new));
    }

    #[test]
    fn a_letter_that_stays_keeps_its_place_on_screen_when_the_box_moves() {
        // "ab" at x 100 becomes "b" at x 110: the box moved right by 10, and
        // "b" was at 110 on screen all along.
        let (old, new) = (line("ab"), line("b"));
        let from = carried(&letters(&old), &letters(&new));
        let start = frame(&old, &new, &from, 10., 10., 0.);
        let b = start.iter().find(|drawn| drawn.mark.letter == 'b').unwrap();
        assert_eq!(b.x, 0., "10 in the old box less the box's move");
        let end = frame(&old, &new, &from, 10., 10., 1.);
        assert_eq!(end[0].x, 0.);
    }

    struct Label(SharedString);
    impl Render for Label {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .flex()
                .items_start()
                .text_size(px(12.))
                .child(
                    div()
                        .debug_selector(|| "plain".into())
                        .child(self.0.clone()),
                )
                .child(
                    div()
                        .debug_selector(|| "morph".into())
                        .child(text_morph("label", self.0.clone())),
                )
                .child(
                    div()
                        .debug_selector(|| "narrow".into())
                        .w(px(30.))
                        .overflow_hidden()
                        .child(text_morph("narrow-label", self.0.clone())),
                )
        }
    }

    #[gpui::test]
    fn it_takes_the_room_plain_text_takes_and_the_new_width_at_once(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| cx.new(|_| Label("Copy".into())))
                .unwrap()
        });
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let size = |visual: &mut VisualTestContext, name: &'static str| {
            visual.debug_bounds(name).unwrap().size
        };
        let same = |visual: &mut VisualTestContext| {
            let (morph, plain) = (size(visual, "morph"), size(visual, "plain"));
            assert_eq!(morph.height, plain.height);
            assert!(
                (morph.width - plain.width).abs() < px(1.),
                "{morph:?} against plain text's {plain:?}"
            );
        };
        same(visual);
        window
            .update(visual, |label, _, cx| {
                label.0 = "Copied to the clipboard".into();
                cx.notify();
            })
            .unwrap();
        visual.run_until_parked();
        // Mid-change it already has the new text's size: nothing reflows
        // while the letters move. The narrow one was cut to its room and
        // drew without trouble.
        same(visual);
        assert_eq!(size(visual, "narrow").width, px(30.));
    }
}
