//! System dictation on the notch. While the global shortcut dictates into
//! another app the wings open with it: on the right the mic, the live level
//! and "Dictating" in the accent; on the left the newest words as they
//! arrive, gliding in beside the camera while the oldest leave past the far
//! edge. The ending says Pasted, Copied (with the chord to paste) or why it
//! failed, then folds, and the agents' wings come back.
//!
//! The controller (`crate::dictation::system`) owns the dictation; this only
//! paints a copy of its phase and levels. The notch never takes focus for
//! it: its window does not activate on either system (`WS_EX_NOACTIVATE` on
//! Windows, a non-activating panel on macOS), and nothing here asks for the
//! foreground or the keyboard.
use super::*;
use crate::dictation::system::SystemDictation;
use gpui::{SharedString, linear_color_stop, linear_gradient};
use monocode_core::dictation::{
    self as voice, BARS,
    hotkey::Os,
    system::{self as dictation, Phase},
};

/// The left wing before any word was heard.
pub(super) const EMPTY_WING: u32 = 28;
/// The left wing grows in these steps as words come, up to `WORDS_WING`, so
/// the island moves a few times per dictation rather than on every word.
const WING_STEP: f32 = 48.;
pub(super) const WORDS_WING: u32 = 192;
/// About how wide a character of the words is, for the wing's steps.
const CHAR_WIDTH: f32 = 6.2;
/// The right wing while listening: the mic, the level and its word.
pub(super) const LISTEN_WING: u32 = 152;
/// An ending: its mark on the left, its words on the right.
pub(super) const MARK_WING: u32 = 40;
pub(super) const PASTED_WING: u32 = 76;
const COPIED_WING: u32 = 178;
const ERROR_WING: u32 = WIDEST_WING;
/// How many characters of words the left wing keeps: more than it shows, so
/// the oldest slide out past its edge instead of vanishing.
const WORD_BUDGET: usize = 72;
const WORD_SIZE: f32 = 11.5;
const WORD_GAP: f32 = 3.5;
/// The fade on the far edge of the words.
const EDGE_FADE: f32 = 22.;
/// A new word's fade, and the row's glide toward the camera.
const WORD_IN: f32 = 0.18;
const GLIDE: f32 = 0.24;
/// The newest of the controller's level bars that the wing draws.
const SHOWN_BARS: usize = 12;
const BARS_HEIGHT: f32 = 14.;

/// A word on the left wing.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Word {
    /// Its place among every word heard in this dictation.
    index: usize,
    text: SharedString,
    /// When it came, while it still fades in; `None` once it was there.
    at: Option<Instant>,
}

/// What came and what it replaced at the end of the words, for the glide:
/// the row starts shifted by the difference and slides back.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Tails {
    new: Vec<SharedString>,
    old: Vec<SharedString>,
}

/// The notch's copy of the system dictation.
pub(crate) struct State {
    controller: Option<Entity<SystemDictation>>,
    phase: Phase,
    /// The last phase that showed, drawn while the island folds from it.
    last: Phase,
    /// The wings drew the dictation last time, so a fold keeps it.
    drew: bool,
    levels: [f32; BARS],
    shown: [f32; BARS],
    eased_at: Option<Instant>,
    words: Vec<Word>,
    /// The words as last heard, for what is new.
    heard: String,
    /// The words' wing: it only grows within a dictation.
    left: u32,
    /// Ends still to measure, then the row's shift and when it began.
    pending: Vec<Tails>,
    glide: Option<(f32, Instant)>,
    _watch: Option<Subscription>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            controller: None,
            phase: Phase::Idle,
            last: Phase::Idle,
            drew: false,
            levels: [0.; BARS],
            shown: [0.; BARS],
            eased_at: None,
            words: Vec::new(),
            heard: String::new(),
            left: EMPTY_WING,
            pending: Vec::new(),
            glide: None,
            _watch: None,
        }
    }
}

impl State {
    /// The dictation is on the wings now.
    pub(super) fn shows(&self) -> bool {
        self.phase.visible()
    }

    fn restart(&mut self) {
        self.words.clear();
        self.heard.clear();
        self.left = EMPTY_WING;
        self.pending.clear();
        self.glide = None;
        self.shown = [0.; BARS];
        self.eased_at = None;
    }

    fn hear(&mut self, current: &str, now: Instant) {
        if current == self.heard {
            return;
        }
        let (words, tails) = next_words(&self.words, &self.heard, current, now);
        self.words = words;
        self.pending.extend(tails);
        self.heard = current.to_owned();
        self.left = self.left.max(words_wing(current.chars().count()));
    }

    /// How far the row is still shifted toward the camera.
    fn glide_offset(&self, now: Instant) -> f32 {
        self.glide.map_or(0., |(shift, at)| {
            let t = (now.duration_since(at).as_secs_f32() / GLIDE).clamp(0., 1.);
            shift * (1. - t).powi(3)
        })
    }
}

/// The left wing for this many characters of words: in steps, up to
/// `WORDS_WING`.
pub(super) fn words_wing(chars: usize) -> u32 {
    if chars == 0 {
        return EMPTY_WING;
    }
    let wanted = chars as f32 * CHAR_WIDTH + 20.;
    let stepped = (wanted / WING_STEP).ceil() * WING_STEP;
    (stepped as u32).clamp(WING_STEP as u32, WORDS_WING)
}

/// The wings for a phase, `None` while there is no dictation.
pub(super) fn wings(phase: &Phase, words: u32) -> Option<(u32, u32)> {
    match phase {
        Phase::Idle => None,
        Phase::Starting | Phase::Listening { .. } | Phase::Finishing => Some((words, LISTEN_WING)),
        Phase::Pasted => Some((MARK_WING, PASTED_WING)),
        Phase::Copied => Some((MARK_WING, COPIED_WING)),
        Phase::Error(_) => Some((MARK_WING, ERROR_WING)),
    }
}

/// The words for `current`, after `old` showed `previous`: what stayed keeps
/// its time, what is new comes `now`, and the ends that changed (if any word
/// showed before) say how far the row moves.
pub(super) fn next_words(
    old: &[Word],
    previous: &str,
    current: &str,
    now: Instant,
) -> (Vec<Word>, Option<Tails>) {
    let wing = dictation::wing(previous, current, WORD_BUDGET);
    let total = current.split_whitespace().count();
    let start = total.saturating_sub(wing.words.len());
    let first_fresh = total.saturating_sub(wing.fresh).max(start);
    let words: Vec<Word> = wing
        .words
        .into_iter()
        .enumerate()
        .map(|(at, text)| {
            let index = start + at;
            let at = if index < first_fresh {
                old.iter()
                    .find(|word| word.index == index)
                    .and_then(|word| word.at)
            } else {
                Some(now)
            };
            Word {
                index,
                text: text.into(),
                at,
            }
        })
        .collect();
    let tail = |words: &[Word]| -> Vec<SharedString> {
        words
            .iter()
            .filter(|word| word.index >= first_fresh)
            .map(|word| word.text.clone())
            .collect()
    };
    let tails = Tails {
        new: tail(&words),
        old: tail(old),
    };
    let moved = !old.is_empty() && tails != Tails::default();
    (words, moved.then_some(tails))
}

impl Notch {
    /// Follows the app's system dictation, when there is one.
    pub(super) fn watch_dictation(&mut self, cx: &mut Context<Self>) {
        if let Some(controller) = crate::dictation::system::get(cx) {
            self.follow_dictation(controller, cx);
        }
    }

    pub(crate) fn follow_dictation(
        &mut self,
        controller: Entity<SystemDictation>,
        cx: &mut Context<Self>,
    ) {
        let watch = cx.observe(&controller, |this, controller, cx| {
            this.sync_dictation(&controller, cx)
        });
        self.dictating._watch = Some(watch);
        self.sync_dictation(&controller, cx);
        self.dictating.controller = Some(controller);
    }

    fn sync_dictation(&mut self, controller: &Entity<SystemDictation>, cx: &mut Context<Self>) {
        let (phase, levels) = {
            let controller = controller.read(cx);
            (controller.phase().clone(), *controller.levels())
        };
        self.show_dictation(phase, levels, cx);
    }

    /// What the system dictation is doing now.
    pub(crate) fn show_dictation(
        &mut self,
        phase: Phase,
        levels: [f32; BARS],
        cx: &mut Context<Self>,
    ) {
        let state = &mut self.dictating;
        if state.phase == phase && state.levels == levels {
            return;
        }
        let was = state.phase.visible();
        if phase == Phase::Starting && state.phase != Phase::Starting {
            state.restart();
        }
        if let Phase::Listening { words, .. } = &phase {
            state.hear(words, Instant::now());
        }
        state.levels = levels;
        if phase.visible() {
            state.last = phase.clone();
        }
        let changed = was != phase.visible();
        state.phase = phase;
        if changed {
            // The wings change what they hold: what comes fades in, the
            // dictation or the agents' state it covered.
            let now = Instant::now();
            self.appeared = self.appeared.map(|(shape, tab, _)| (shape, tab, now));
        }
        self.keep_responsive();
        cx.notify();
    }

    /// The wings the dictation takes, while it shows.
    pub(super) fn dictation_wings(&self) -> Option<(u32, u32)> {
        wings(&self.dictating.phase, self.dictating.left)
    }

    /// The phase the wings draw: the dictation's, or, while the island
    /// folds from it, the one it showed last.
    pub(super) fn dictation_drawn(&mut self) -> Option<Phase> {
        if self.dictating.shows() {
            self.dictating.drew = true;
            return Some(self.dictating.phase.clone());
        }
        if self.leaving.is_some() && self.dictating.drew {
            return Some(self.dictating.last.clone());
        }
        self.dictating.drew = false;
        None
    }

    #[cfg(test)]
    pub(crate) fn dictation_words(&self) -> Vec<String> {
        self.dictating
            .words
            .iter()
            .map(|word| word.text.to_string())
            .collect()
    }

    /// The dictation on the wings: the words on the left and the mic on the
    /// right beside a camera, or both in one row where there is none.
    pub(super) fn dictation_view(
        &mut self,
        phase: &Phase,
        camera: Option<model::Camera>,
        wings: (f32, f32),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ending = phase.linger().is_some();
        let status = self.dictation_status(phase, window, cx);
        let (left_wing, right_wing) = wings;
        match camera {
            Some(camera) => {
                let left = if ending {
                    div()
                        .debug_selector(|| "notch-dictation-done".into())
                        .pl(px(14.))
                        .flex()
                        .items_center()
                        .child(ending_mark(phase))
                        .into_any_element()
                } else {
                    div()
                        .size_full()
                        .pl(px(12.))
                        .pr(px(8.))
                        .child(self.words_view(left_wing - 20., window, cx))
                        .into_any_element()
                };
                div()
                    .debug_selector(|| "notch-dictation".into())
                    .size_full()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .w(px(left_wing))
                            .h_full()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .child(left),
                    )
                    .child(div().w(px(camera.width)).flex_shrink_0())
                    .child(
                        div()
                            .w(px(right_wing))
                            .flex_shrink_0()
                            .min_w_0()
                            .pl(px(10.))
                            .pr(px(12.))
                            .flex()
                            .items_center()
                            .child(status),
                    )
                    .into_any_element()
            }
            None => {
                let row = div()
                    .debug_selector(|| "notch-dictation".into())
                    .size_full()
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(10.));
                if ending {
                    row.child(
                        div()
                            .debug_selector(|| "notch-dictation-done".into())
                            .flex_shrink_0()
                            .child(ending_mark(phase)),
                    )
                    .child(div().flex_1().min_w_0().flex().child(status))
                    .into_any_element()
                } else {
                    row.child(div().flex_1().min_w_0().h_full().child(self.words_view(
                        left_wing - 10.,
                        window,
                        cx,
                    )))
                    .child(div().flex_shrink_0().child(status))
                    .into_any_element()
                }
            }
        }
    }

    /// The mic, the level and what the dictation is doing; or how it ended.
    fn dictation_status(
        &mut self,
        phase: &Phase,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = phase.label(Os::current());
        if phase.linger().is_some() {
            return div()
                .min_w_0()
                .flex_1()
                .truncate()
                .text_size(px(11.5))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(ink(0.9))
                .child(crate::text_morph::text_morph(
                    "notch-dictation-label",
                    label,
                ))
                .into_any_element();
        }
        let listening = matches!(phase, Phase::Listening { .. });
        // The bars ease toward the level, and settle flat once it stops.
        let target = if listening {
            self.dictating.levels
        } else {
            [0.; BARS]
        };
        let now = Instant::now();
        let state = &mut self.dictating;
        if cx.reduce_motion() {
            state.shown = target;
        } else {
            let dt = state
                .eased_at
                .map_or(1., |at| now.duration_since(at).as_secs_f32());
            voice::ease_levels(&mut state.shown, &target, dt);
            for (shown, target) in state.shown.iter_mut().zip(&target) {
                if (*shown - target).abs() < 0.01 {
                    *shown = *target;
                }
            }
            if state.shown != target {
                window.request_animation_frame();
            }
        }
        state.eased_at = Some(now);
        let color = if listening { accent_color() } else { ink(0.55) };
        div()
            .debug_selector(|| "notch-dictation-status".into())
            .flex()
            .items_center()
            .gap(px(7.))
            .child(icon("mic-01", 12.).flex_shrink_0().text_color(color))
            .child(level_bars(
                &state.shown[BARS - SHOWN_BARS..],
                if listening { accent_color() } else { ink(0.3) },
            ))
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(px(11.5))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(color)
                    .child(crate::text_morph::text_morph(
                        "notch-dictation-label",
                        label,
                    )),
            )
            .into_any_element()
    }

    /// The words heard so far, newest beside the camera: a new word fades
    /// in as the row glides over to make room, and the oldest leave past
    /// the far edge, under its fade. Under reduced motion they just show.
    fn words_view(&mut self, room: f32, window: &mut Window, cx: &App) -> AnyElement {
        let now = Instant::now();
        let reduce = cx.reduce_motion();
        let pending = std::mem::take(&mut self.dictating.pending);
        if reduce {
            self.dictating.glide = None;
        } else {
            let shift: f32 = pending
                .iter()
                .map(|tails| tail_width(&tails.new, window) - tail_width(&tails.old, window))
                .sum();
            if shift.abs() > 0.5 {
                let left = self.dictating.glide_offset(now);
                self.dictating.glide = Some((left + shift, now));
            }
        }
        let offset = self.dictating.glide_offset(now);
        if offset.abs() < 0.25 {
            self.dictating.glide = None;
        }
        let mut moving = self.dictating.glide.is_some();
        let words: Vec<(Word, f32, f32)> = self
            .dictating
            .words
            .iter()
            .map(|word| {
                let shown = match word.at {
                    Some(at) if !reduce => {
                        (now.duration_since(at).as_secs_f32() / WORD_IN).clamp(0., 1.)
                    }
                    _ => 1.,
                };
                moving |= shown < 1.;
                (word.clone(), word_width(&word.text, window), shown)
            })
            .collect();
        // Once a word has faded in it stays put; it no longer asks for frames.
        for word in &mut self.dictating.words {
            if word
                .at
                .is_some_and(|at| reduce || now.duration_since(at).as_secs_f32() >= WORD_IN)
            {
                word.at = None;
            }
        }
        if moving {
            window.request_animation_frame();
        }
        let total = words.iter().map(|(_, width, _)| width).sum::<f32>()
            + WORD_GAP * words.len().saturating_sub(1) as f32;
        let row = div()
            .absolute()
            .top_0()
            .bottom_0()
            .right(px(-offset))
            .w(px(total))
            .flex()
            .items_center()
            .gap(px(WORD_GAP))
            .children(words.into_iter().map(|(word, width, shown)| {
                let index = word.index;
                div()
                    .debug_selector(move || format!("notch-dictation-word-{index}"))
                    .flex_shrink_0()
                    .w(px(width))
                    .whitespace_nowrap()
                    .text_size(px(WORD_SIZE))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(ink(0.88))
                    .opacity(shown)
                    .child(word.text)
            }));
        div()
            .debug_selector(|| "notch-dictation-words".into())
            .relative()
            .size_full()
            .overflow_hidden()
            .child(row)
            .when(total + offset.max(0.) > room, |words| {
                words.child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(EDGE_FADE))
                        .bg(linear_gradient(
                            90.,
                            linear_color_stop(black(1.), 0.),
                            linear_color_stop(black(0.), 1.),
                        )),
                )
            })
            .into_any_element()
    }

    /// Paste last, in the open panel's header, once something was dictated.
    pub(super) fn paste_last_glyph(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let controller = self.dictating.controller.clone()?;
        if controller.read(cx).history().is_empty() {
            return None;
        }
        Some(
            glyph("notch-paste-last", "Paste last dictation", "mic-01", false)
                .on_click(cx.listener(move |this, _, _, cx| {
                    // The panel folds first, handing back the keyboard if a
                    // field had it, so the paste lands in the app in front.
                    this.set_open(false, cx);
                    controller.update(cx, |controller, cx| controller.paste_last(cx));
                }))
                .into_any_element(),
        )
    }
}

/// How an ending reads at a glance: a check, the clipboard, or a warning.
fn ending_mark(phase: &Phase) -> AnyElement {
    match phase {
        Phase::Pasted => icon("check", 13.)
            .text_color(accent_color())
            .into_any_element(),
        Phase::Copied => icon("response-copy", 13.)
            .text_color(ink(0.85))
            .into_any_element(),
        Phase::Error(_) => icon("alert-circle", 13.)
            .text_color(ink(0.85))
            .into_any_element(),
        _ => div().into_any_element(),
    }
}

/// The microphone's recent loudness, newest on the right.
fn level_bars(levels: &[f32], color: gpui::Hsla) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .h(px(BARS_HEIGHT))
        .flex()
        .items_center()
        .gap(px(2.))
        .children(levels.iter().map(move |level| {
            div()
                .w(px(2.))
                .h(px((voice::bar_height(*level) * BARS_HEIGHT / 20.).max(2.)))
                .bg(color)
        }))
}

/// A word's width as the wing draws it.
fn word_width(text: &str, window: &mut Window) -> f32 {
    let mut font = window.text_style().font();
    font.family = crate::fonts::UI.into();
    font.weight = gpui::FontWeight::MEDIUM;
    let run = gpui::TextRun {
        len: text.len(),
        font,
        color: gpui::Hsla::default(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(text.to_owned().into(), px(WORD_SIZE), &[run], None)
        .width
        .as_f32()
}

/// The room these words take at the end of the row, each with its gap.
fn tail_width(words: &[SharedString], window: &mut Window) -> f32 {
    words
        .iter()
        .map(|word| word_width(word, window) + WORD_GAP)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(words: &[Word]) -> Vec<&str> {
        words.iter().map(|word| word.text.as_ref()).collect()
    }

    #[test]
    fn new_words_come_now_and_the_rest_keep_their_time() {
        let first = Instant::now();
        let (words, tails) = next_words(&[], "", "hola", first);
        assert_eq!(texts(&words), ["hola"]);
        assert_eq!(words[0].at, Some(first));
        // Nothing showed before: the first word only fades in.
        assert_eq!(tails, None);

        let later = first + Duration::from_millis(300);
        let (words, tails) = next_words(&words, "hola", "hola mundo", later);
        assert_eq!(texts(&words), ["hola", "mundo"]);
        assert_eq!(words[0].at, Some(first));
        assert_eq!(words[1].at, Some(later));
        assert_eq!(
            tails,
            Some(Tails {
                new: vec!["mundo".into()],
                old: vec![],
            })
        );

        // A partial that revises its last word trades it.
        let (words, tails) = next_words(&words, "hola mundo", "hola mundos", later);
        assert_eq!(texts(&words), ["hola", "mundos"]);
        assert_eq!(
            tails,
            Some(Tails {
                new: vec!["mundos".into()],
                old: vec!["mundo".into()],
            })
        );
    }

    #[test]
    fn the_oldest_words_leave_once_past_the_budget() {
        let now = Instant::now();
        let long = "palabra ".repeat(20);
        let (words, _) = next_words(&[], "", &long, now);
        let kept: usize = words.iter().map(|word| word.text.len() + 1).sum();
        assert!(kept <= WORD_BUDGET + 1, "{kept}");
        // Their places count from the first word heard.
        assert_eq!(words.last().unwrap().index, 19);
        assert_eq!(words[0].index, 20 - words.len());
    }

    #[test]
    fn the_wings_grow_in_steps_and_endings_take_their_own() {
        assert_eq!(words_wing(0), EMPTY_WING);
        assert_eq!(words_wing(3), 48);
        assert_eq!(words_wing(10), 96);
        assert_eq!(words_wing(400), WORDS_WING);
        assert_eq!(wings(&Phase::Idle, 96), None);
        assert_eq!(wings(&Phase::Starting, 96), Some((96, LISTEN_WING)));
        assert_eq!(wings(&Phase::Finishing, 96), Some((96, LISTEN_WING)));
        assert_eq!(wings(&Phase::Pasted, 96), Some((MARK_WING, PASTED_WING)));
        let (_, copied) = wings(&Phase::Copied, 96).unwrap();
        let (_, error) = wings(&Phase::Error("x".into()), 96).unwrap();
        assert!(copied > PASTED_WING && error >= copied);
        assert!(error <= WIDEST_WING && WORDS_WING <= WIDEST_WING);
    }
}
