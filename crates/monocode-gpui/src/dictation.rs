//! Dictation.tsx and useDictation.ts: the composer's mic, which starts
//! listening and stops to put the text in at the caret, and the bar under its
//! toolbar with the waveform and the words heard so far, the model download,
//! or an error. Escape drops a dictation in progress.
mod engine;
mod permission;

use crate::{input::Input, right_panel::RED_400, theme::*};
use gpui::{
    App, Context, Entity, Global, IntoElement, KeyDownEvent, MouseButton, Render, Role,
    SharedString, Task, Toggled, WeakEntity, Window, div, prelude::*, px, rgb, svg,
};
use monobot_dictation::Event;
use monocode_core::dictation::{self as model, BARS, Phase};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub(crate) use engine::Engine;

/// The engine the composers dictate with; tests put a stand-in here.
#[derive(Clone)]
pub(crate) struct Engines(pub Arc<dyn Engine>);
impl Global for Engines {}

fn engine(cx: &App) -> Arc<dyn Engine> {
    cx.try_global::<Engines>()
        .map_or_else(engine::local, |engines| engines.0.clone())
}

/// The app downloads the speech model on its own the first time it runs, in
/// the background, so dictation works when the user first reaches for the
/// mic. Once it is there it stays on disk until the first dictation loads it.
pub(crate) fn init(cx: &mut App) {
    let engine = engine(cx);
    cx.background_executor()
        .spawn(async move { engine.prepare() })
        .detach();
}

enum Install {
    Progress(u64, u64),
    Done(Result<(), String>),
}

/// One composer's dictation.
pub(crate) struct Dictation {
    input: WeakEntity<Input>,
    engine: Arc<dyn Engine>,
    phase: Phase,
    /// Recent loudness, oldest first, and the bars easing toward it.
    levels: [f32; BARS],
    shown: [f32; BARS],
    eased_at: Option<Instant>,
    /// What has been heard so far, before it lands in the draft.
    partial: String,
    /// The user is talking right now, by the voice detector.
    speaking: bool,
    /// Download progress, 0 to 1, while the model installs.
    progress: f32,
    model_bytes: u64,
    error: Option<String>,
    /// The engine's dictation this composer owns.
    ticket: Option<u64>,
    /// Bumped whenever a dictation ends here, so a start that lands after
    /// a cancel closes the microphone instead of listening.
    generation: u64,
    task: Option<Task<()>>,
    events: Option<Task<()>>,
}

impl Dictation {
    pub(crate) fn new(input: &Entity<Input>, cx: &mut Context<Self>) -> Self {
        Self {
            input: input.downgrade(),
            engine: engine(cx),
            phase: Phase::Idle,
            levels: [0.; BARS],
            shown: [0.; BARS],
            eased_at: None,
            partial: String::new(),
            speaking: false,
            progress: 0.,
            model_bytes: 0,
            error: None,
            ticket: None,
            generation: 0,
            task: None,
            events: None,
        }
    }

    pub(crate) fn phase(&self) -> Phase {
        self.phase
    }

    pub(crate) fn partial(&self) -> &str {
        &self.partial
    }

    pub(crate) fn speaking(&self) -> bool {
        self.speaking
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn set_phase(&mut self, phase: Phase, cx: &mut Context<Self>) {
        self.phase = phase;
        cx.notify();
    }

    fn unlisten(&mut self) {
        self.events = None;
        self.speaking = false;
    }

    fn fail(&mut self, reason: String, cx: &mut Context<Self>) {
        self.unlisten();
        self.ticket = None;
        self.error = Some(reason);
        self.set_phase(Phase::Idle, cx);
    }

    /// The mic: starts listening, and stops to put the text in.
    pub(crate) fn toggle(&mut self, cx: &mut Context<Self>) {
        match self.phase {
            Phase::Listening => return self.finish(cx),
            Phase::Idle | Phase::NeedsModel => {}
            _ => return,
        }
        let engine = self.engine.clone();
        let status = cx
            .background_executor()
            .spawn(async move { engine.status() });
        self.task = Some(cx.spawn(async move |this, cx| {
            let status = status.await;
            let _ = this.update(cx, |this, cx| {
                this.model_bytes = status.bytes;
                if status.installing {
                    this.follow(cx);
                } else if !status.installed {
                    this.set_phase(Phase::NeedsModel, cx);
                } else {
                    this.start(cx);
                }
            });
        }));
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        self.partial.clear();
        self.levels = [0.; BARS];
        self.shown = [0.; BARS];
        self.eased_at = None;
        self.set_phase(Phase::Starting, cx);
        let generation = self.generation;
        let (heard, hearing) = async_channel::unbounded();
        self.events = Some(cx.spawn(async move |this, cx| {
            while let Ok(event) = hearing.recv().await {
                if this.update(cx, |this, cx| this.heard(event, cx)).is_err() {
                    return;
                }
            }
            // The engine let go while listening: another composer took the
            // microphone, or it went away. Keep what was heard.
            let _ = this.update(cx, |this, cx| {
                if this.phase == Phase::Listening && this.generation == generation {
                    this.finish(cx);
                }
            });
        }));
        let engine = self.engine.clone();
        let started = cx.background_executor().spawn({
            let engine = engine.clone();
            async move {
                engine.start(Box::new(move |event| {
                    let _ = heard.try_send(event);
                }))
            }
        });
        // Detached, so a microphone that opens after a cancel, or after the
        // composer closed, is always closed again.
        cx.spawn(async move |this, cx| {
            let started = started.await;
            let ticket = started.as_ref().ok().copied();
            let kept = this
                .update(cx, |this, cx| match started {
                    Ok(ticket) if this.generation == generation => {
                        this.ticket = Some(ticket);
                        this.set_phase(Phase::Listening, cx);
                        true
                    }
                    Ok(_) => false,
                    Err(reason) => {
                        if this.generation == generation {
                            this.fail(reason, cx);
                        }
                        true
                    }
                })
                .unwrap_or(false);
            if let Some(ticket) = ticket.filter(|_| !kept) {
                cx.background_executor()
                    .spawn(async move { engine.cancel(ticket) })
                    .detach();
            }
        })
        .detach();
    }

    fn heard(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Level(level) => {
                model::push_level(&mut self.levels, level);
                // The waveform repaints on the shared clock while listening;
                // under reduced motion it has none, so each level redraws.
                if cx.reduce_motion() {
                    cx.notify();
                }
            }
            Event::Partial(text) => {
                self.partial = text;
                cx.notify();
            }
            Event::Speech(speaking) => {
                if self.speaking != speaking {
                    self.speaking = speaking;
                    cx.notify();
                }
            }
            // The microphone went away: keep what was heard and say why, so
            // nothing waits on a microphone that will never speak again.
            Event::Lost(reason) => {
                if self.phase == Phase::Listening {
                    self.finish(cx);
                }
                self.error = Some(reason);
                cx.notify();
            }
        }
    }

    fn finish(&mut self, cx: &mut Context<Self>) {
        let Some(ticket) = self.ticket.take() else {
            return self.set_phase(Phase::Idle, cx);
        };
        self.generation += 1;
        let generation = self.generation;
        self.set_phase(Phase::Finishing, cx);
        let engine = self.engine.clone();
        let (done, result) = async_channel::bounded(1);
        cx.background_executor()
            .spawn(async move {
                let text = engine.stop(ticket);
                let _ = done.send(text).await;
            })
            .detach();
        self.task = Some(cx.spawn(async move |this, cx| {
            let Ok(text) = result.recv().await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation || this.phase != Phase::Finishing {
                    return;
                }
                this.unlisten();
                this.partial.clear();
                this.set_phase(Phase::Idle, cx);
                if !text.trim().is_empty() {
                    let _ = this
                        .input
                        .update(cx, |input, cx| input.insert_dictated(&text, cx));
                }
            });
        }));
    }

    /// The app is already setting the model up: show its progress, then start.
    fn follow(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        self.progress = 0.;
        self.set_phase(Phase::Installing, cx);
        let engine = self.engine.clone();
        self.task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                let engine = engine.clone();
                let status = cx
                    .background_executor()
                    .spawn(async move { engine.status() })
                    .await;
                let done = status.installed && !status.installing;
                let lost = !status.installing && !status.installed;
                let alive = this.update(cx, |this, cx| {
                    this.progress = model::progress(status.done, status.total);
                    if done {
                        this.start(cx);
                    } else if lost {
                        this.fail("The speech model could not be set up.".into(), cx);
                    } else {
                        cx.notify();
                    }
                });
                if alive.is_err() || done || lost {
                    return;
                }
            }
        }));
    }

    /// The bar's Download: fetches the model, then starts listening.
    pub(crate) fn install(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        self.progress = 0.;
        self.set_phase(Phase::Installing, cx);
        let engine = self.engine.clone();
        let (report, reports) = async_channel::unbounded();
        cx.background_executor()
            .spawn(async move {
                let mut last = 0;
                let result = engine.install(&mut |done, total| {
                    // About every megabyte, so the bar moves without flooding.
                    if done == total || done.saturating_sub(last) >= 1 << 20 {
                        last = done;
                        let _ = report.try_send(Install::Progress(done, total));
                    }
                });
                let _ = report.try_send(Install::Done(result));
            })
            .detach();
        self.task = Some(cx.spawn(async move |this, cx| {
            while let Ok(report) = reports.recv().await {
                let done = matches!(report, Install::Done(_));
                let alive = this.update(cx, |this, cx| match report {
                    Install::Progress(done, total) => {
                        this.progress = model::progress(done, total);
                        cx.notify();
                    }
                    Install::Done(Ok(())) => this.start(cx),
                    Install::Done(Err(reason)) => this.fail(reason, cx),
                });
                if alive.is_err() || done {
                    return;
                }
            }
        }));
    }

    /// Escape: stops listening and drops the words.
    pub(crate) fn cancel(&mut self, cx: &mut Context<Self>) {
        if !self.phase.cancellable() {
            return;
        }
        self.generation += 1;
        if let Some(ticket) = self.ticket.take() {
            let engine = self.engine.clone();
            cx.background_executor()
                .spawn(async move { engine.cancel(ticket) })
                .detach();
        }
        self.unlisten();
        self.partial.clear();
        self.set_phase(Phase::Idle, cx);
    }

    /// Discards a call turn even if final transcription is still in flight.
    pub(crate) fn cancel_pending(&mut self, cx: &mut Context<Self>) {
        if self.phase != Phase::Finishing {
            self.cancel(cx);
            return;
        }
        self.generation += 1;
        self.task = None;
        self.unlisten();
        self.partial.clear();
        self.set_phase(Phase::Idle, cx);
    }

    pub(crate) fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        if self.phase == Phase::NeedsModel {
            self.phase = Phase::Idle;
        }
        cx.notify();
    }

    /// Escape drops a dictation in progress before the composer sees the
    /// key. Returns whether it did.
    pub(crate) fn escape(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        if event.keystroke.key != "escape" || !self.phase.cancellable() {
            return false;
        }
        self.cancel(cx);
        true
    }
}

impl Drop for Dictation {
    fn drop(&mut self) {
        // A composer that closes while listening lets the microphone go.
        if let Some(ticket) = self.ticket.take() {
            let engine = self.engine.clone();
            std::thread::spawn(move || engine.cancel(ticket));
        }
    }
}

/// Escape on a composer box: drops `dictation` in progress first.
pub(crate) fn capture_escape(
    dictation: &Entity<Dictation>,
) -> impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static {
    let dictation = dictation.downgrade();
    move |event, _, cx| {
        if dictation
            .update(cx, |dictation, cx| dictation.escape(event, cx))
            .unwrap_or(false)
        {
            cx.stop_propagation();
        }
    }
}

/// DictationButton: the composer's `size-6.5` mic, solid accent with a stop
/// square while listening.
pub(crate) fn mic(dictation: &Entity<Dictation>, cx: &App) -> impl IntoElement + use<> {
    let phase = dictation.read(cx).phase();
    let listening = phase == Phase::Listening;
    let busy = phase.busy();
    let entity = dictation.downgrade();
    div()
        .id("dictation-mic")
        .debug_selector(|| "dictation-mic".into())
        .role(Role::Button)
        .aria_label(phase.button_label())
        .tooltip(chrome_tip(phase.button_label(), None))
        .aria_toggled(if listening {
            Toggled::True
        } else {
            Toggled::False
        })
        .group("dictation-mic")
        .flex_shrink_0()
        .size(px(26.))
        .flex()
        .items_center()
        .justify_center()
        .when(listening, |d| d.bg(accent_color()))
        .when(!listening && !busy, |d| d.hover(|s| s.bg(content(0.08))))
        .when(busy, |d| d.opacity(0.5))
        .when(!busy, |d| d.cursor_pointer())
        .child(if listening {
            svg()
                .path("square-fill")
                .size(px(10.))
                .flex_shrink_0()
                .text_color(white(1.))
        } else {
            icon("mic-01", 14.)
                .text_color(content(0.6))
                .when(!busy, |s| {
                    s.group_hover("dictation-mic", |s| s.text_color(content(1.)))
                })
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, _, cx| {
            let _ = entity.update(cx, |dictation, cx| dictation.toggle(cx));
        })
}

/// The bar's quiet text buttons (`text-content/55 hover:text-content`).
fn text_button(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .role(Role::Button)
        .aria_label(label)
        .flex_shrink_0()
        .cursor_pointer()
        .text_color(content(0.55))
        .hover(|s| s.text_color(content(1.)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(label)
}

/// `border-t border-stroke px-3 py-1.5 text-xs`.
fn row() -> gpui::Div {
    div()
        .flex()
        .items_center()
        .border_t_1()
        .border_color(content(0.07))
        .px_3()
        .py(px(6.))
        .text_size(px(12.))
        .line_height(px(16.))
}

/// DictationBar: what the composer shows under its toolbar while dictation
/// is active.
impl Render for Dictation {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(error) = self.error.clone() {
            return row()
                .debug_selector(|| "dictation-error".into())
                .gap_2()
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .text_color(rgb(RED_400))
                        .child(error),
                )
                .child(
                    text_button("dictation-dismiss", "Dismiss")
                        .on_click(cx.listener(|this, _, _, cx| this.dismiss(cx))),
                )
                .into_any_element();
        }
        let offer = model::model_offer(self.model_bytes);
        match self.phase {
            Phase::NeedsModel => row()
                .debug_selector(|| "dictation-offer".into())
                .gap_2()
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .text_color(content(0.7))
                        .child(offer.text),
                )
                .child(
                    text_button("dictation-not-now", "Not now")
                        .on_click(cx.listener(|this, _, _, cx| this.dismiss(cx))),
                )
                .child(
                    div()
                        .id("dictation-download")
                        .debug_selector(|| "dictation-download".into())
                        .role(Role::Button)
                        .aria_label(offer.action)
                        .flex_shrink_0()
                        .cursor_pointer()
                        .px_2()
                        .py(px(2.))
                        .bg(accent_color())
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(white(1.))
                        .child(offer.action)
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(|this, _, _, cx| this.install(cx))),
                )
                .into_any_element(),
            Phase::Installing => row()
                .debug_selector(|| "dictation-install".into())
                .gap_3()
                .text_color(content(0.7))
                .child(div().flex_shrink_0().child(offer.busy))
                .child(
                    div()
                        .relative()
                        .h(px(4.))
                        .min_w_0()
                        .flex_1()
                        .bg(content(0.1))
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .left_0()
                                .w(gpui::relative(self.progress))
                                .bg(accent_color()),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .font_features(crate::right_panel::tabular())
                        .child(model::percent(self.progress)),
                )
                .into_any_element(),
            Phase::Starting | Phase::Listening | Phase::Finishing => {
                let now = Instant::now();
                if cx.reduce_motion() {
                    self.shown = self.levels;
                } else {
                    let dt = self
                        .eased_at
                        .map_or(1., |at| now.duration_since(at).as_secs_f32());
                    model::ease_levels(&mut self.shown, &self.levels, dt);
                }
                self.eased_at = Some(now);
                if self.phase == Phase::Listening || self.shown != self.levels {
                    crate::pulse::request(crate::pulse::Rate::Smooth, window, cx);
                }
                let caption: SharedString = self
                    .phase
                    .caption(&self.partial)
                    .unwrap_or_default()
                    .to_owned()
                    .into();
                row()
                    .debug_selector(|| "dictation-bar".into())
                    .gap_3()
                    .child(wave(&self.shown))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_color(content(0.5))
                            .child(caption),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .font_family(crate::fonts::mono())
                            .text_size(px(11.))
                            .text_color(content(0.4))
                            .child("Esc to cancel".to_uppercase()),
                    )
                    .into_any_element()
            }
            Phase::Idle => div().into_any_element(),
        }
    }
}

/// DictationWave: live bars of how loud the microphone is, newest on the
/// right.
fn wave(levels: &[f32; BARS]) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .h(px(20.))
        .flex()
        .items_center()
        .gap(px(2.))
        .children(levels.iter().map(|level| {
            div()
                .w(px(2.))
                .h(px(model::bar_height(*level)))
                .bg(accent_color())
        }))
}

#[cfg(test)]
pub(crate) mod tests;
