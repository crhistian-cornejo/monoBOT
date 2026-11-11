//! "While dictating" (Settings › Voice): pause what plays, or lower the
//! output's volume, while a composer or the system dictation listens, and
//! put it back once the last one ends. The decisions are
//! `monocode_core::dictation::quiet`'s; this wires them to the notch's
//! media sessions and to the output's volume, which is read and set on a
//! thread of its own, in order, so a restore always follows its lowering.
use crate::notch::Notch;
use gpui::{App, EntityId, Global};
use monocode_core::dictation::quiet::{Ducking, Held, Quiet, Step, Want};
use std::sync::mpsc;

/// How the output's volume is read and set: the system's own output, or a
/// test's stand-in (set before the first dictation lowers it).
#[derive(Clone, Copy)]
pub(crate) struct Speaker {
    pub read: fn() -> Option<(f32, bool)>,
    pub set: fn(f32) -> bool,
}
impl Global for Speaker {}

impl Default for Speaker {
    fn default() -> Self {
        Self {
            read: crate::notch::output_volume,
            set: crate::notch::set_output_volume,
        }
    }
}

enum Volume {
    Lower,
    Restore,
}

#[derive(Default)]
struct State {
    quiet: Quiet,
    volume: Option<mpsc::Sender<Volume>>,
}
impl Global for State {}

/// Dictation `who` listens (`dictating`: opening the microphone, hearing,
/// finishing) or ended. The first one to listen quiets the sound as
/// Settings asks; the last one to end puts it back. Called on every phase
/// change: the same state twice changes nothing.
pub(crate) fn follow(who: EntityId, dictating: bool, cx: &mut App) {
    let choice = cx
        .try_global::<crate::settings::Preferences>()
        .map(|prefs| prefs.while_dictating)
        .unwrap_or_default();
    let step = cx
        .default_global::<State>()
        .quiet
        .follow(who.as_u64(), dictating, choice);
    // Later, so the notch is never updated inside one of its own handlers.
    if let Some(step) = step {
        cx.defer(move |cx| run(step, cx));
    }
}

fn run(step: Step, cx: &mut App) {
    match step {
        Step::Quiet(want) => {
            let held = match want {
                Want::PauseMedia => Held::Paused(pause(cx)),
                Want::LowerVolume => {
                    send(Volume::Lower, cx);
                    Held::Lowered
                }
            };
            // Every dictation ended before this ran: undo it now.
            if let Some(held) = cx.default_global::<State>().quiet.hold(held) {
                undo(held, cx);
            }
        }
        Step::Undo(held) => undo(held, cx),
    }
}

fn undo(held: Held, cx: &mut App) {
    match held {
        Held::Paused(apps) if !apps.is_empty() => resume(&apps, cx),
        Held::Paused(_) => {}
        Held::Lowered => send(Volume::Restore, cx),
    }
}

/// The notch windows, whose media sessions know what plays.
fn notches(cx: &App) -> Vec<gpui::WindowHandle<Notch>> {
    cx.windows()
        .into_iter()
        .filter_map(|window| window.downcast::<Notch>())
        .collect()
}

fn pause(cx: &mut App) -> Vec<String> {
    for notch in notches(cx) {
        if let Ok(Some(apps)) = notch.update(cx, |notch, _, cx| notch.pause_playing(cx)) {
            return apps;
        }
    }
    Vec::new()
}

fn resume(apps: &[String], cx: &mut App) {
    for notch in notches(cx) {
        if let Ok(true) = notch.update(cx, |notch, _, cx| notch.resume_paused(apps, cx)) {
            return;
        }
    }
}

fn send(volume: Volume, cx: &mut App) {
    let speaker = cx.try_global::<Speaker>().copied().unwrap_or_default();
    let state = cx.default_global::<State>();
    let sender = state
        .volume
        .get_or_insert_with(|| start_volume(speaker))
        .clone();
    let _ = sender.send(volume);
}

/// The thread that reads and sets the volume, one request after another.
/// It ends with the app, when its sender goes.
fn start_volume(speaker: Speaker) -> mpsc::Sender<Volume> {
    let (sender, received) = mpsc::channel::<Volume>();
    let _ = std::thread::Builder::new()
        .name("dictation-volume".into())
        .spawn(move || {
            let mut ducking = Ducking::default();
            while let Ok(request) = received.recv() {
                match request {
                    Volume::Lower => {
                        if let Some(to) = ducking.lower((speaker.read)())
                            && !(speaker.set)(to)
                        {
                            ducking.not_set();
                        }
                    }
                    Volume::Restore => {
                        if let Some(from) = ducking.restore((speaker.read)()) {
                            let _ = (speaker.set)(from);
                        }
                    }
                }
            }
        });
    sender
}

#[cfg(test)]
mod tests {
    use super::super::system::SystemDictation;
    use super::super::tests::Fake;
    use super::super::{Dictation, Engines};
    use super::*;
    use crate::input::Input;
    use gpui::{AppContext, Entity, TestAppContext};
    use monocode_core::dictation::{self as model, hotkey::Action, quiet::WhileDictating};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    /// The stand-in output: its volume, and every level set on it.
    static OUTPUT: Mutex<(f32, Vec<f32>)> = Mutex::new((0.5, Vec::new()));

    fn read() -> Option<(f32, bool)> {
        Some((OUTPUT.lock().unwrap().0, false))
    }

    fn set(volume: f32) -> bool {
        let mut output = OUTPUT.lock().unwrap();
        output.0 = volume;
        output.1.push(volume);
        true
    }

    /// The levels set so far, once the volume thread set `count` of them
    /// (or gave up waiting).
    fn levels_set(count: usize) -> Vec<f32> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let set = OUTPUT.lock().unwrap().1.clone();
            if set.len() >= count || Instant::now() > deadline {
                return set;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// The levels set, after long enough for a request that should not be
    /// there to have arrived.
    fn settled() -> Vec<f32> {
        std::thread::sleep(Duration::from_millis(100));
        OUTPUT.lock().unwrap().1.clone()
    }

    fn choose(choice: WhileDictating, cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(crate::settings::Preferences {
                while_dictating: choice,
                ..Default::default()
            });
            cx.set_global(Speaker { read, set });
        });
    }

    fn system_phase(
        system: &Entity<SystemDictation>,
        cx: &mut TestAppContext,
    ) -> model::system::Phase {
        system.read_with(cx, |system, _| system.phase().clone())
    }

    #[gpui::test]
    fn the_volume_is_lowered_while_any_dictation_listens(cx: &mut TestAppContext) {
        choose(WhileDictating::LowerVolume, cx);
        // An engine each: one stand-in microphone serves one dictation.
        let (system, composer, _input) = cx.update(|cx| {
            cx.set_global(Engines(Fake::installed("hola")));
            let (hits, _) = async_channel::unbounded();
            let system = cx.new(|cx| SystemDictation::new(hits, cx));
            cx.set_global(Engines(Fake::installed("qué tal")));
            let input = cx.new(Input::session_composer);
            let composer = cx.new(|cx| Dictation::new(&input, cx));
            (system, composer, input)
        });
        let lowered = 0.5 * model::quiet::LOWER_TO;

        // The system dictation lowers it; the composer beside it does not
        // lower it again, and the first to end does not raise it.
        system.update(cx, |system, cx| system.act(Action::Start, cx));
        cx.run_until_parked();
        assert!(matches!(
            system_phase(&system, cx),
            model::system::Phase::Listening { .. }
        ));
        assert_eq!(levels_set(1), [lowered]);
        composer.update(cx, |composer, cx| composer.toggle(cx));
        cx.run_until_parked();
        assert_eq!(
            composer.read_with(cx, |composer, _| composer.phase()),
            model::Phase::Listening
        );
        system.update(cx, |system, cx| system.act(Action::Stop, cx));
        cx.run_until_parked();
        assert_eq!(system_phase(&system, cx), model::system::Phase::Copied);
        assert_eq!(settled(), [lowered], "the composer still listens");

        // The last one ends: the volume comes back.
        composer.update(cx, |composer, cx| composer.toggle(cx));
        cx.run_until_parked();
        assert_eq!(
            composer.read_with(cx, |composer, _| composer.phase()),
            model::Phase::Idle
        );
        assert_eq!(levels_set(2), [lowered, 0.5]);

        // A volume the user moved while dictating is theirs: it stays.
        composer.update(cx, |composer, cx| composer.toggle(cx));
        cx.run_until_parked();
        assert_eq!(levels_set(3).len(), 3);
        OUTPUT.lock().unwrap().0 = 0.9;
        composer.update(cx, |composer, cx| composer.cancel(cx));
        cx.run_until_parked();
        assert_eq!(settled().len(), 3);
        assert_eq!(OUTPUT.lock().unwrap().0, 0.9);

        // Do nothing leaves the volume alone; so does a composer that closes
        // while it listens, once it lowered it and goes.
        choose(WhileDictating::Nothing, cx);
        system.update(cx, |system, cx| system.act(Action::Start, cx));
        cx.run_until_parked();
        system.update(cx, |system, cx| system.act(Action::Cancel, cx));
        cx.run_until_parked();
        assert_eq!(settled().len(), 3);

        choose(WhileDictating::LowerVolume, cx);
        composer.update(cx, |composer, cx| composer.toggle(cx));
        cx.run_until_parked();
        assert_eq!(levels_set(4).len(), 4);
        drop(composer);
        // The app lets go of the entity at its next update.
        cx.update(|_| {});
        cx.run_until_parked();
        let set = levels_set(5);
        assert_eq!(set.len(), 5, "closing the composer puts it back");
        assert_eq!(set[4], 0.9);
    }
}
