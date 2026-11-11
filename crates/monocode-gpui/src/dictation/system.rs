//! System dictation: the global shortcut (Ctrl+Space by default, the
//! Keybindings command "App: Dictate Anywhere") dictates in any app. A press
//! with a monoBOT composer focused in front goes to that composer's own
//! dictation; anywhere else this controller listens with the same engine,
//! keeps what the notch shows (`Phase`, the level bars) up to date, and
//! pastes the text where the caret was, or copies it when nothing can take
//! it. The notch paints [`SystemDictation`]; it never takes focus.
use super::{
    Dictate, Dictation, Engine, Pressed, engine,
    keyboard::{self, Hit},
};
use gpui::{App, AppContext, ClipboardItem, Context, Entity, Global, Task, WeakEntity};
use monobot_dictation::Event;
use monocode_core::dictation::{
    self as model, BARS,
    hotkey::{Action, Os, Shortcut},
    keycodes,
    paste::{self, History, Outcome, Plan, Step},
    route::{Delivery, Router, Scene},
    system::Phase,
};
use std::{sync::Arc, time::Duration, time::Instant};

/// The Keybindings command that holds the global shortcut.
pub(crate) const COMMAND: &str = "App: Dictate Anywhere";
/// Its default, as a GPUI keystroke: Ctrl+Space on every system.
pub(crate) const DEFAULT_KEYSTROKE: &str = "ctrl-space";
/// How often a Mac without Accessibility checks again for it.
const RETRY: Duration = Duration::from_secs(3);

/// The global shortcut from Settings: the user's rebinding, the default,
/// or `None` once it was disabled (or cannot be read as a shortcut).
pub(crate) fn shortcut(prefs: &crate::settings::Preferences) -> Option<Shortcut> {
    match prefs.keybindings.get(COMMAND) {
        Some(Some(keys)) => keycodes::from_keystroke(keys),
        Some(None) => None,
        None => keycodes::from_keystroke(DEFAULT_KEYSTROKE),
    }
}

/// The app's one system dictation, for the notch to paint.
#[derive(Clone)]
pub(crate) struct SystemDictations(pub Entity<SystemDictation>);
impl Global for SystemDictations {}

pub(crate) fn get(cx: &App) -> Option<Entity<SystemDictation>> {
    cx.try_global::<SystemDictations>()
        .map(|global| global.0.clone())
}

/// Makes the controller, starts the keyboard hook (where the system allows
/// it) and follows Settings from then on.
pub(crate) fn init(cx: &mut App) {
    let (hits, heard) = async_channel::unbounded::<Hit>();
    let entity = cx.new(|cx| SystemDictation::new(hits, cx));
    cx.set_global(SystemDictations(entity.clone()));
    let weak = entity.downgrade();
    cx.spawn(async move |cx| {
        while let Ok(hit) = heard.recv().await {
            if weak.update(cx, |this, cx| this.key(hit, cx)).is_err() {
                return;
            }
        }
    })
    .detach();
    let weak = entity.downgrade();
    cx.observe_global::<crate::settings::Preferences>(move |cx| {
        let _ = weak.update(cx, |this, cx| this.configure(cx));
    })
    .detach();
    entity.update(cx, |this, cx| this.configure(cx));
}

pub(crate) struct SystemDictation {
    engine: Arc<dyn Engine>,
    phase: Phase,
    /// Recent loudness, oldest first, for the notch's bars.
    levels: [f32; BARS],
    router: Router,
    epoch: Instant,
    /// The engine's dictation this controller owns.
    ticket: Option<u64>,
    /// Bumped whenever a dictation ends here, so late results are dropped.
    generation: u64,
    /// The shortcut came up (push to talk) while the microphone opened.
    stop_once_listening: bool,
    /// Why the microphone went away, shown when nothing was heard.
    lost: Option<String>,
    events: Option<Task<()>>,
    task: Option<Task<()>>,
    /// The paste in progress, kept apart from `task` so a dictation that
    /// finishes meanwhile never drops it before it puts the clipboard back.
    pasting: Option<Task<()>>,
    linger: Option<Task<()>>,
    /// Paste last: the recent dictations, newest first.
    history: History,
    /// Settings: on, the shortcut, the vocabulary.
    enabled: bool,
    shortcut: Option<Shortcut>,
    vocabulary: Vec<monobot_dictation::Replacement>,
    hits: async_channel::Sender<Hit>,
    /// The keyboard hook runs; false on a Mac until Accessibility allows it.
    hooked: bool,
    retry: Option<Task<()>>,
}

impl SystemDictation {
    pub(crate) fn new(hits: async_channel::Sender<Hit>, cx: &mut Context<Self>) -> Self {
        Self {
            engine: engine(cx),
            phase: Phase::Idle,
            levels: [0.; BARS],
            router: Router::default(),
            epoch: Instant::now(),
            ticket: None,
            generation: 0,
            stop_once_listening: false,
            lost: None,
            events: None,
            task: None,
            pasting: None,
            linger: None,
            history: History::default(),
            enabled: false,
            shortcut: None,
            vocabulary: Vec::new(),
            hits,
            hooked: false,
            retry: None,
        }
    }

    /// What the notch shows: listening (with the words heard so far),
    /// finishing, pasted, copied or why it failed.
    pub(crate) fn phase(&self) -> &Phase {
        &self.phase
    }

    /// The level bars, oldest first, each 0 to 1.
    pub(crate) fn levels(&self) -> &[f32; BARS] {
        &self.levels
    }

    /// Paste last: what was dictated recently, newest first.
    pub(crate) fn history(&self) -> &History {
        &self.history
    }

    /// The shortcut as this system writes it ("Ctrl+Space", "⌃Space"),
    /// while dictating in other apps is on.
    // For a hint that names the shortcut; nothing shows it yet.
    #[allow(dead_code)]
    pub(crate) fn shortcut_label(&self) -> Option<String> {
        self.enabled
            .then(|| self.shortcut.as_ref().map(|s| s.display(Os::current())))
            .flatten()
    }

    /// Dictating in other apps is on, the shortcut is set and the keyboard
    /// hook runs.
    pub(crate) fn ready(&self) -> bool {
        self.enabled && self.shortcut.is_some() && self.hooked
    }

    /// Follows Settings: whether to dictate in other apps, the shortcut and
    /// the vocabulary. The hook starts the first time it is needed.
    fn configure(&mut self, cx: &mut Context<Self>) {
        let Some(prefs) = cx.try_global::<crate::settings::Preferences>() else {
            return;
        };
        let enabled = prefs.dictate_anywhere;
        let shortcut = shortcut(prefs);
        self.vocabulary = prefs.dictation_vocabulary.clone();
        let changed = enabled != self.enabled || shortcut != self.shortcut;
        self.enabled = enabled;
        self.shortcut = shortcut;
        if !changed && (self.hooked || !enabled) {
            return;
        }
        keyboard::watch(self.shortcut.as_ref().filter(|_| enabled));
        if !enabled {
            self.retry = None;
            if self.phase.cancellable() {
                self.cancel(cx);
            }
            cx.notify();
            return;
        }
        self.hook(cx);
        cx.notify();
    }

    /// Starts the hook, and on a Mac that does not allow it yet, tries again
    /// every few seconds while dictating in other apps stays on.
    fn hook(&mut self, cx: &mut Context<Self>) {
        self.hooked = keyboard::start(self.hits.clone());
        if self.hooked || !cfg!(target_os = "macos") || self.retry.is_some() {
            return;
        }
        self.retry = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(RETRY).await;
                let done = this.update(cx, |this, cx| {
                    if !this.enabled {
                        this.retry = None;
                        return true;
                    }
                    this.hooked = keyboard::start(this.hits.clone());
                    if this.hooked {
                        this.retry = None;
                        cx.notify();
                    }
                    this.hooked
                });
                if done.unwrap_or(true) {
                    return;
                }
            }
        }));
    }

    /// One of the shortcut's events from the hook.
    fn key(&mut self, hit: Hit, cx: &mut Context<Self>) {
        let now = hit.at.saturating_duration_since(self.epoch).as_millis() as u64;
        let keystroke = self
            .shortcut
            .as_ref()
            .and_then(|shortcut| gpui::Keystroke::parse(&keycodes::to_keystroke(shortcut)).ok());
        let scene = scene(keystroke.as_ref(), cx);
        match self.router.event(hit.event, now, scene) {
            Some(Delivery::ComposerDown) => composer_down(cx),
            Some(Delivery::ComposerUp) => composer_up(cx),
            Some(Delivery::System(action)) => self.act(action, cx),
            Some(Delivery::Window) => {
                if let (Some(window), Some(keystroke)) = (cx.active_window(), keystroke) {
                    let _ =
                        window.update(cx, |_, window, cx| window.dispatch_keystroke(keystroke, cx));
                }
            }
            None => {}
        }
    }

    /// Start, stop or drop, as the shortcut asked. Tests drive it directly.
    pub(crate) fn act(&mut self, action: Action, cx: &mut Context<Self>) {
        match action {
            Action::Start => self.start(cx),
            Action::Stop => self.finish(cx),
            Action::Cancel => self.cancel(cx),
        }
    }

    fn set_phase(&mut self, phase: Phase, cx: &mut Context<Self>) {
        keyboard::set_listening(phase.cancellable());
        let dictating = matches!(
            phase,
            Phase::Starting | Phase::Listening { .. } | Phase::Finishing
        );
        super::quiet::follow(cx.entity_id(), dictating, cx);
        self.linger = None;
        if let Some(linger) = phase.linger() {
            let generation = self.generation;
            self.linger = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(linger).await;
                let _ = this.update(cx, |this, cx| {
                    if this.generation == generation && this.phase.linger().is_some() {
                        this.phase = Phase::Idle;
                        cx.notify();
                    }
                });
            }));
        }
        self.phase = phase;
        cx.notify();
    }

    /// Ends in `reason`, and lets the next press start a new dictation.
    fn fail(&mut self, reason: String, cx: &mut Context<Self>) {
        self.events = None;
        self.ticket = None;
        self.router.system_ended();
        self.set_phase(Phase::Error(reason), cx);
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.phase,
            Phase::Starting | Phase::Listening { .. } | Phase::Finishing
        ) {
            // Still finishing the last one: this press starts nothing.
            self.router.system_ended();
            return;
        }
        self.generation += 1;
        let generation = self.generation;
        self.levels = [0.; BARS];
        self.lost = None;
        self.stop_once_listening = false;
        self.set_phase(Phase::Starting, cx);
        let engine = self.engine.clone();
        let options = monobot_dictation::Options {
            vocabulary: self.vocabulary.clone(),
        };
        let (heard, hearing) = async_channel::unbounded();
        self.events = Some(cx.spawn(async move |this, cx| {
            while let Ok(event) = hearing.recv().await {
                if this.update(cx, |this, cx| this.heard(event, cx)).is_err() {
                    return;
                }
            }
            // The engine let go while listening: a composer took the
            // microphone, or it went away. Keep what was heard.
            let _ = this.update(cx, |this, cx| {
                if matches!(this.phase, Phase::Listening { .. }) && this.generation == generation {
                    this.router.system_ended();
                    this.finish(cx);
                }
            });
        }));
        let started = cx.background_executor().spawn(async move {
            if !engine.status().installed {
                return Err("Download the speech model in Settings › Voice to dictate.".to_owned());
            }
            let on_event = Box::new(move |event| {
                let _ = heard.try_send(event);
            });
            engine
                .start_with(options, on_event)
                .map(|ticket| (ticket, engine))
        });
        // Detached, so a microphone that opens after a cancel is closed again.
        cx.spawn(async move |this, cx| {
            let started = started.await;
            let late = this
                .update(cx, |this, cx| match started {
                    Ok((ticket, _)) if this.generation == generation => {
                        this.ticket = Some(ticket);
                        this.set_phase(
                            Phase::Listening {
                                level: 0.,
                                words: String::new(),
                            },
                            cx,
                        );
                        if std::mem::take(&mut this.stop_once_listening) {
                            this.finish(cx);
                        }
                        None
                    }
                    Ok((ticket, engine)) => Some((ticket, engine)),
                    Err(reason) => {
                        if this.generation == generation {
                            this.fail(reason, cx);
                        }
                        None
                    }
                })
                .ok()
                .flatten();
            if let Some((ticket, engine)) = late {
                cx.background_executor()
                    .spawn(async move { engine.cancel(ticket) })
                    .detach();
            }
        })
        .detach();
    }

    fn heard(&mut self, event: Event, cx: &mut Context<Self>) {
        let listening = matches!(self.phase, Phase::Listening { .. });
        if let Event::Lost(reason) = event {
            // The microphone went away: keep what was heard and say why
            // if nothing was.
            self.lost = Some(reason);
            if listening {
                self.router.system_ended();
                self.finish(cx);
            }
            return;
        }
        let Phase::Listening { level, words } = &mut self.phase else {
            return;
        };
        match event {
            Event::Level(heard) => {
                model::push_level(&mut self.levels, heard);
                *level = heard;
                cx.notify();
            }
            Event::Partial(text) => {
                *words = text;
                cx.notify();
            }
            Event::Lost(_) | Event::Speech(_) | Event::DecidesTurns | Event::TurnEnd(_) => {}
        }
    }

    fn finish(&mut self, cx: &mut Context<Self>) {
        let Some(ticket) = self.ticket.take() else {
            if self.phase == Phase::Starting {
                self.stop_once_listening = true;
            } else if self.phase.cancellable() {
                self.set_phase(Phase::Idle, cx);
            }
            return;
        };
        self.generation += 1;
        let generation = self.generation;
        self.set_phase(Phase::Finishing, cx);
        let engine = self.engine.clone();
        let text = cx
            .background_executor()
            .spawn(async move { engine.stop(ticket) });
        self.task = Some(cx.spawn(async move |this, cx| {
            let text = text.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation && this.phase == Phase::Finishing {
                    this.events = None;
                    this.deliver(text, cx);
                }
            });
        }));
    }

    /// Escape, or the setting turned off: stops listening and drops the words.
    fn cancel(&mut self, cx: &mut Context<Self>) {
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
        self.events = None;
        self.router.system_ended();
        self.set_phase(Phase::Idle, cx);
    }

    /// The transcript: pasted into the app in front, or copied.
    fn deliver(&mut self, text: String, cx: &mut Context<Self>) {
        if text.trim().is_empty() {
            match self.lost.take() {
                Some(reason) => self.set_phase(Phase::Error(reason), cx),
                None => self.set_phase(Phase::Idle, cx),
            }
            return;
        }
        self.history.push(&text);
        self.paste(&text, cx);
    }

    /// Paste last: the newest dictation, pasted again where the caret is
    /// (the notch's header and the tray's menu). With nothing dictated yet
    /// the notch says so.
    pub(crate) fn paste_last(&mut self, cx: &mut Context<Self>) {
        // Not over a dictation, nor over a paste that has not put the
        // clipboard back yet (it would save our own text as the user's).
        if self.phase.cancellable() || self.phase == Phase::Finishing || self.pasting.is_some() {
            return;
        }
        self.generation += 1;
        match self.history.last().map(str::to_owned) {
            Some(text) => self.paste(&text, cx),
            None => self.set_phase(Phase::Error("Nothing dictated yet.".into()), cx),
        }
    }

    fn paste(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(plan) = paste::plan(text, Os::current(), keyboard::has_paste_target()) else {
            return self.set_phase(Phase::Idle, cx);
        };
        let generation = self.generation;
        self.pasting = Some(cx.spawn(async move |this, cx| {
            let outcome = run(plan, &this, cx).await;
            let _ = this.update(cx, |this, cx| {
                // Done: the task ends right after this, so dropping its
                // handle here cancels nothing left to run.
                this.pasting = None;
                if this.generation == generation {
                    this.set_phase(Phase::from_outcome(outcome), cx);
                }
            });
        }));
    }
}

/// Runs a paste plan's steps. A paste the system refuses leaves the text on
/// the clipboard: the outcome is then Copied.
async fn run(plan: Plan, this: &WeakEntity<SystemDictation>, cx: &mut gpui::AsyncApp) -> Outcome {
    let mut saved: Option<ClipboardItem> = None;
    let mut ours = String::new();
    for step in plan.steps {
        match step {
            Step::SaveClipboard => {
                saved = this
                    .update(cx, |_, cx| cx.read_from_clipboard())
                    .ok()
                    .flatten();
            }
            Step::WriteText(text) => {
                ours = text.clone();
                let _ = this.update(cx, |_, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(text))
                });
            }
            Step::Wait(wait) => cx.background_executor().timer(wait).await,
            Step::SendPaste(_) => {
                let sent = cx
                    .background_executor()
                    .spawn(async move { keyboard::send_paste() })
                    .await;
                if sent.is_err() {
                    return Outcome::Copied;
                }
            }
            Step::RestoreIfStillOurs => {
                let _ = this.update(cx, |_, cx| {
                    let now = cx.read_from_clipboard().and_then(|item| item.text());
                    if paste::still_ours(now.as_deref(), &ours)
                        && let Some(saved) = saved.take()
                    {
                        cx.write_to_clipboard(saved);
                    }
                });
            }
        }
    }
    plan.outcome
}

/// What the router needs to know when the shortcut goes down: is a monoBOT
/// window in front, is its focus in a composer or in something else that
/// binds these keys (the code editor's completion), and does a composer the
/// shortcut started still listen.
fn scene(keystroke: Option<&gpui::Keystroke>, cx: &mut App) -> Scene {
    let window = cx.active_window();
    let frontmost = keyboard::frontmost_is_ours().unwrap_or(window.is_some()) && window.is_some();
    let (focused_composer, window_takes_key) = window
        .filter(|_| frontmost)
        .and_then(|window| {
            window
                .update(cx, |_, window, cx| {
                    let composer = window.is_action_available(&Dictate, cx);
                    let bound = keystroke.is_some_and(|keystroke| {
                        let (bindings, _) = cx.key_bindings().borrow().bindings_for_input(
                            std::slice::from_ref(keystroke),
                            &window.context_stack(),
                        );
                        bindings
                            .iter()
                            .any(|binding| !binding.action().partial_eq(&Dictate))
                    });
                    (composer, bound)
                })
                .ok()
        })
        .unwrap_or((false, false));
    let composer_listening = pressed(cx).is_some_and(|dictation| {
        matches!(
            dictation.read(cx).phase(),
            model::Phase::Starting | model::Phase::Listening
        )
    });
    Scene {
        frontmost,
        focused_composer,
        composer_listening,
        window_takes_key,
    }
}

fn pressed(cx: &App) -> Option<Entity<Dictation>> {
    cx.try_global::<Pressed>()
        .and_then(|pressed| pressed.0.upgrade())
}

/// The shortcut went down for a composer: a listening one gets it directly;
/// otherwise the focused composer in front does, through its own action.
fn composer_down(cx: &mut App) {
    if let Some(dictation) = pressed(cx).filter(|dictation| {
        matches!(
            dictation.read(cx).phase(),
            model::Phase::Starting | model::Phase::Listening
        )
    }) {
        dictation.update(cx, |dictation, cx| dictation.shortcut_down(cx));
        return;
    }
    if let Some(window) = cx.active_window() {
        let _ = window.update(cx, |_, window, cx| {
            window.dispatch_action(Box::new(Dictate), cx)
        });
    }
}

/// The key came up after a composer took its press.
fn composer_up(cx: &mut App) {
    if let Some(dictation) = pressed(cx) {
        dictation.update(cx, |dictation, cx| dictation.shortcut_up(cx));
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::Fake;
    use super::*;
    use gpui::TestAppContext;

    fn controller(fake: &Arc<Fake>, cx: &mut TestAppContext) -> Entity<SystemDictation> {
        cx.update(|cx| {
            cx.set_global(super::super::Engines(fake.clone()));
            let (hits, _) = async_channel::unbounded();
            cx.new(|cx| SystemDictation::new(hits, cx))
        })
    }

    fn phase(entity: &Entity<SystemDictation>, cx: &mut TestAppContext) -> Phase {
        entity.read_with(cx, |this, _| this.phase().clone())
    }

    #[gpui::test]
    fn listens_shows_the_words_then_copies_with_nothing_in_front(cx: &mut TestAppContext) {
        let fake = Fake::installed(" hola mundo ");
        let entity = controller(&fake, cx);
        entity.update(cx, |this, cx| this.act(Action::Start, cx));
        assert_eq!(phase(&entity, cx), Phase::Starting);
        cx.run_until_parked();
        assert!(matches!(phase(&entity, cx), Phase::Listening { .. }));

        fake.hear(Event::Level(0.6));
        fake.hear(Event::Partial("hola".into()));
        cx.run_until_parked();
        entity.read_with(cx, |this, _| {
            assert_eq!(this.phase.words(), "hola");
            assert_eq!(this.phase.level(), 0.6);
            assert_eq!(this.levels()[BARS - 1], 0.6);
        });

        entity.update(cx, |this, cx| this.act(Action::Stop, cx));
        assert_eq!(phase(&entity, cx), Phase::Finishing);
        cx.run_until_parked();
        // The test platform has no app in front to paste into: copied.
        assert_eq!(phase(&entity, cx), Phase::Copied);
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("hola mundo".into())
        );
        entity.read_with(cx, |this, _| {
            assert_eq!(this.history().last(), Some("hola mundo"));
        });
        assert_eq!(fake.calls(), ["start", "stop 7"]);
        // "Copied" stays a moment, then the notch lets it go.
        cx.executor().advance_clock(Duration::from_secs(4));
        cx.run_until_parked();
        assert_eq!(phase(&entity, cx), Phase::Idle);
    }

    #[gpui::test]
    fn escape_drops_the_words_and_nothing_is_pasted(cx: &mut TestAppContext) {
        let fake = Fake::installed("no");
        let entity = controller(&fake, cx);
        entity.update(cx, |this, cx| this.act(Action::Start, cx));
        cx.run_until_parked();
        entity.update(cx, |this, cx| this.act(Action::Cancel, cx));
        cx.run_until_parked();
        assert_eq!(phase(&entity, cx), Phase::Idle);
        assert_eq!(fake.calls(), ["start", "cancel 7"]);
        assert!(entity.read_with(cx, |this, _| this.history().is_empty()));
    }

    #[gpui::test]
    fn a_release_while_the_microphone_opens_stops_once_it_is_open(cx: &mut TestAppContext) {
        let fake = Fake::installed("rápido");
        let entity = controller(&fake, cx);
        entity.update(cx, |this, cx| {
            this.act(Action::Start, cx);
            this.act(Action::Stop, cx);
        });
        cx.run_until_parked();
        assert_eq!(phase(&entity, cx), Phase::Copied);
        assert_eq!(fake.calls(), ["start", "stop 7"]);
    }

    #[gpui::test]
    fn a_missing_model_says_where_to_get_it(cx: &mut TestAppContext) {
        let fake = Arc::new(Fake::default());
        let entity = controller(&fake, cx);
        entity.update(cx, |this, cx| this.act(Action::Start, cx));
        cx.run_until_parked();
        assert!(
            matches!(phase(&entity, cx), Phase::Error(reason) if reason.contains("Settings › Voice"))
        );
        assert!(fake.calls().is_empty());
    }

    #[gpui::test]
    fn nothing_heard_pastes_nothing(cx: &mut TestAppContext) {
        let fake = Fake::installed("   ");
        let entity = controller(&fake, cx);
        cx.write_to_clipboard(ClipboardItem::new_string("mine".into()));
        entity.update(cx, |this, cx| this.act(Action::Start, cx));
        cx.run_until_parked();
        entity.update(cx, |this, cx| this.act(Action::Stop, cx));
        cx.run_until_parked();
        assert_eq!(phase(&entity, cx), Phase::Idle);
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("mine".into())
        );
    }

    #[gpui::test]
    fn the_shortcut_and_vocabulary_follow_settings(cx: &mut TestAppContext) {
        let fake = Fake::installed("x");
        let entity = controller(&fake, cx);
        let mut prefs = crate::settings::Preferences::default();
        assert_eq!(shortcut(&prefs), Some(Shortcut::default()));
        prefs
            .keybindings
            .insert(COMMAND.into(), Some("ctrl-alt-d".into()));
        prefs.dictation_vocabulary = vec![monobot_dictation::Replacement {
            heard: "mono bot".into(),
            write: "monoBOT".into(),
        }];
        cx.update(|cx| cx.set_global(prefs.clone()));
        entity.update(cx, |this, cx| this.configure(cx));
        entity.read_with(cx, |this, _| {
            assert_eq!(
                this.shortcut.as_ref().map(Shortcut::stored).as_deref(),
                Some("Ctrl+Alt+D")
            );
            assert_eq!(this.vocabulary.len(), 1);
            assert!(this.shortcut_label().is_some());
            // The test platform has no hook.
            assert!(!this.ready());
        });
        prefs.keybindings.insert(COMMAND.into(), None);
        prefs.dictate_anywhere = false;
        cx.update(|cx| cx.set_global(prefs));
        entity.update(cx, |this, cx| this.configure(cx));
        entity.read_with(cx, |this, _| {
            assert_eq!(this.shortcut, None);
            assert_eq!(this.shortcut_label(), None);
        });
    }

    /// A composer box as the chats and the quick composer build it: the
    /// "Composer" key context and the shortcut's action on its dictation.
    struct ComposerBox {
        input: Entity<crate::input::Input>,
        dictation: Entity<Dictation>,
        composer: bool,
    }

    impl gpui::Render for ComposerBox {
        fn render(
            &mut self,
            _: &mut gpui::Window,
            _: &mut Context<Self>,
        ) -> impl gpui::IntoElement {
            use gpui::{InteractiveElement, ParentElement};
            let boxed = gpui::div().child(self.input.clone());
            if self.composer {
                boxed
                    .key_context("Composer")
                    .on_action(super::super::on_shortcut(&self.dictation))
            } else {
                // Like the code editor, which completes on Ctrl+Space.
                boxed
                    .key_context("Editorish")
                    .on_action(|_: &Complete, _, _| {
                        COMPLETIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    })
            }
        }
    }

    gpui::actions!(dictation_tests, [Complete]);
    static COMPLETIONS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn window<'a>(
        fake: &Arc<Fake>,
        composer: bool,
        cx: &'a mut TestAppContext,
    ) -> (Entity<ComposerBox>, &'a mut gpui::VisualTestContext) {
        cx.update(|cx| cx.set_global(super::super::Engines(fake.clone())));
        let (view, cx) = cx.add_window_view(|window, cx| {
            let input = cx.new(crate::input::Input::session_composer);
            let dictation = cx.new(|cx| Dictation::new(&input, cx));
            window.focus(&gpui::Focusable::focus_handle(input.read(cx), cx), cx);
            ComposerBox {
                input,
                dictation,
                composer,
            }
        });
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        (view, cx)
    }

    fn hit(event: KeyEvent) -> Hit {
        Hit {
            event,
            at: Instant::now(),
        }
    }

    use monocode_core::dictation::hotkey::KeyEvent;

    #[gpui::test]
    fn the_shortcut_in_a_focused_composer_dictates_into_it(cx: &mut TestAppContext) {
        let fake = Fake::installed("ship it");
        let (view, cx) = window(&fake, true, cx);
        let system = cx.update(|_, cx| {
            let (hits, _) = async_channel::unbounded();
            cx.new(|cx| SystemDictation::new(hits, cx))
        });
        let dictation = view.read_with(cx, |view, _| view.dictation.clone());
        // A tap: the composer listens, the system dictation stays idle.
        system.update(cx, |this, cx| this.key(hit(KeyEvent::Press), cx));
        cx.run_until_parked();
        system.update(cx, |this, cx| this.key(hit(KeyEvent::Release), cx));
        cx.run_until_parked();
        assert_eq!(
            dictation.read_with(cx, |d, _| d.phase()),
            model::Phase::Listening
        );
        assert_eq!(
            system.read_with(cx, |this, _| this.phase().clone()),
            Phase::Idle
        );
        // The next tap ends it and the words land in the draft.
        system.update(cx, |this, cx| this.key(hit(KeyEvent::Press), cx));
        cx.run_until_parked();
        system.update(cx, |this, cx| this.key(hit(KeyEvent::Release), cx));
        cx.run_until_parked();
        assert_eq!(
            dictation.read_with(cx, |d, _| d.phase()),
            model::Phase::Idle
        );
        let draft = view.read_with(cx, |view, cx| view.input.read(cx).content.to_string());
        assert_eq!(draft, "ship it");
        assert_eq!(fake.calls(), ["start", "stop 7"]);
    }

    #[gpui::test]
    fn the_shortcut_with_no_composer_focused_dictates_into_the_system(cx: &mut TestAppContext) {
        let fake = Fake::installed("hola");
        let (view, cx) = window(&fake, false, cx);
        let system = cx.update(|_, cx| {
            let (hits, _) = async_channel::unbounded();
            cx.new(|cx| SystemDictation::new(hits, cx))
        });
        system.update(cx, |this, cx| this.key(hit(KeyEvent::Press), cx));
        cx.run_until_parked();
        assert!(matches!(
            system.read_with(cx, |this, _| this.phase().clone()),
            Phase::Listening { .. }
        ));
        let dictation = view.read_with(cx, |view, _| view.dictation.clone());
        assert_eq!(
            dictation.read_with(cx, |d, _| d.phase()),
            model::Phase::Idle
        );
        // Escape while it listens drops it.
        system.update(cx, |this, cx| this.key(hit(KeyEvent::Release), cx));
        system.update(cx, |this, cx| this.key(hit(KeyEvent::Escape), cx));
        cx.run_until_parked();
        assert_eq!(
            system.read_with(cx, |this, _| this.phase().clone()),
            Phase::Idle
        );
        assert_eq!(fake.calls(), ["start", "cancel 7"]);
    }

    #[gpui::test]
    fn keys_the_focused_element_binds_go_to_it_instead(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.bind_keys([gpui::KeyBinding::new(
                DEFAULT_KEYSTROKE,
                Complete,
                Some("Editorish"),
            )])
        });
        let fake = Fake::installed("hola");
        let (_view, cx) = window(&fake, false, cx);
        let system = cx.update(|_, cx| {
            let (hits, _) = async_channel::unbounded();
            cx.new(|cx| {
                let mut this = SystemDictation::new(hits, cx);
                this.shortcut = Some(Shortcut::default());
                this
            })
        });
        let before = COMPLETIONS.load(std::sync::atomic::Ordering::Relaxed);
        system.update(cx, |this, cx| this.key(hit(KeyEvent::Press), cx));
        system.update(cx, |this, cx| this.key(hit(KeyEvent::Release), cx));
        cx.run_until_parked();
        assert_eq!(
            COMPLETIONS.load(std::sync::atomic::Ordering::Relaxed),
            before + 1
        );
        assert_eq!(
            system.read_with(cx, |this, _| this.phase().clone()),
            Phase::Idle
        );
        assert!(fake.calls().is_empty());
    }

    #[gpui::test]
    fn rebinding_the_shortcut_rebinds_it_in_composers_too(cx: &mut TestAppContext) {
        let keys = |cx: &mut TestAppContext| {
            cx.update(|cx| {
                let mut keys: Vec<String> = cx
                    .key_bindings()
                    .borrow()
                    .bindings_for_action(&Dictate)
                    .map(|binding| binding.keystrokes()[0].unparse())
                    .collect();
                keys.sort();
                keys
            })
        };
        fn sorted(keys: [&str; 2]) -> Vec<String> {
            let mut keys = keys.map(str::to_owned).to_vec();
            keys.sort();
            keys
        }
        cx.update(super::super::bind);
        let mut prefs = crate::settings::Preferences::default();
        cx.update(|cx| crate::settings::apply_keybindings(&prefs, cx));
        let default = if cfg!(target_os = "macos") {
            "cmd-shift-m"
        } else {
            "ctrl-shift-m"
        };
        assert_eq!(keys(cx), sorted([DEFAULT_KEYSTROKE, default]));
        prefs
            .keybindings
            .insert(COMMAND.into(), Some("ctrl-alt-d".into()));
        cx.update(|cx| crate::settings::apply_keybindings(&prefs, cx));
        assert_eq!(keys(cx), sorted(["ctrl-alt-d", default]));
        // Disabled: only the composer's own shortcut is left.
        prefs.keybindings.insert(COMMAND.into(), None);
        cx.update(|cx| crate::settings::apply_keybindings(&prefs, cx));
        assert_eq!(keys(cx), [default]);
    }
}
