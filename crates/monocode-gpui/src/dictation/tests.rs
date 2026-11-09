use super::{engine::Status, *};
use gpui::TestAppContext;
use std::sync::Mutex;

type Hearing = Box<dyn Fn(Event) + Send>;

/// The engine without a microphone: it hears whatever the test says.
#[derive(Default)]
pub(crate) struct Fake {
    pub installed: Mutex<bool>,
    pub installing: Mutex<bool>,
    pub start_error: Mutex<Option<String>>,
    /// What stop returns.
    pub said: Mutex<String>,
    pub calls: Mutex<Vec<String>>,
    hearing: Mutex<Option<Hearing>>,
}

impl Fake {
    pub(crate) fn installed(said: &str) -> Arc<Self> {
        let fake = Self::default();
        *fake.installed.lock().unwrap() = true;
        *fake.said.lock().unwrap() = said.into();
        Arc::new(fake)
    }

    /// The microphone hears `event`.
    pub(crate) fn hear(&self, event: Event) {
        if let Some(hearing) = self.hearing.lock().unwrap().as_ref() {
            hearing(event);
        }
    }

    pub(crate) fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn call(&self, call: impl Into<String>) {
        self.calls.lock().unwrap().push(call.into());
    }
}

impl Engine for Fake {
    fn status(&self) -> Status {
        let installed = *self.installed.lock().unwrap();
        Status {
            installed,
            installing: *self.installing.lock().unwrap(),
            bytes: 671_122_626,
            done: 300,
            total: 1_200,
            on_disk: if installed { 460_000_000 } else { 0 },
            neural: installed,
        }
    }

    fn prepare(&self) {}

    fn install(&self, progress: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
        self.call("install");
        progress(1 << 21, 1 << 22);
        progress(1 << 22, 1 << 22);
        *self.installed.lock().unwrap() = true;
        Ok(())
    }

    fn start(&self, on_event: Box<dyn Fn(Event) + Send>) -> Result<u64, String> {
        self.call("start");
        if let Some(error) = self.start_error.lock().unwrap().clone() {
            return Err(error);
        }
        *self.hearing.lock().unwrap() = Some(on_event);
        Ok(7)
    }

    fn stop(&self, ticket: u64) -> String {
        self.call(format!("stop {ticket}"));
        self.hearing.lock().unwrap().take();
        self.said.lock().unwrap().clone()
    }

    fn cancel(&self, ticket: u64) {
        self.call(format!("cancel {ticket}"));
        self.hearing.lock().unwrap().take();
    }

    fn remove(&self) -> Result<(), String> {
        self.call("remove");
        *self.installed.lock().unwrap() = false;
        Ok(())
    }
}

fn dictation(
    fake: &Arc<Fake>,
    draft: &str,
    cx: &mut TestAppContext,
) -> (Entity<Input>, Entity<Dictation>) {
    cx.update(|cx| {
        cx.set_global(Engines(fake.clone()));
        let input = cx.new(Input::session_composer);
        input.update(cx, |input, cx| input.set_text(draft.into(), cx));
        let dictation = cx.new(|cx| Dictation::new(&input, cx));
        (input, dictation)
    })
}

fn phase(dictation: &Entity<Dictation>, cx: &mut TestAppContext) -> Phase {
    dictation.read_with(cx, |dictation, _| dictation.phase())
}

#[gpui::test]
fn listens_then_puts_the_words_in_at_the_caret(cx: &mut TestAppContext) {
    let fake = Fake::installed(" qué tal ");
    let (input, dictation) = dictation(&fake, "Hola", cx);
    dictation.update(cx, |dictation, cx| dictation.toggle(cx));
    cx.run_until_parked();
    assert_eq!(phase(&dictation, cx), Phase::Listening);

    fake.hear(Event::Level(0.8));
    fake.hear(Event::Partial("qué tal".into()));
    fake.hear(Event::Speech(true));
    cx.run_until_parked();
    dictation.read_with(cx, |dictation, _| {
        assert_eq!(dictation.levels[BARS - 1], 0.8);
        assert_eq!(dictation.partial(), "qué tal");
        assert!(dictation.speaking());
    });
    fake.hear(Event::Speech(false));
    cx.run_until_parked();
    assert!(!dictation.read_with(cx, |dictation, _| dictation.speaking()));

    dictation.update(cx, |dictation, cx| dictation.toggle(cx));
    assert_eq!(phase(&dictation, cx), Phase::Finishing);
    cx.run_until_parked();
    assert_eq!(phase(&dictation, cx), Phase::Idle);
    input.read_with(cx, |input, _| {
        assert_eq!(input.content.as_ref(), "Hola qué tal");
    });
    assert_eq!(fake.calls(), ["start", "stop 7"]);
}

#[gpui::test]
fn an_unplugged_microphone_keeps_the_words_and_says_why(cx: &mut TestAppContext) {
    let fake = Fake::installed("hasta aquí");
    let (input, dictation) = dictation(&fake, "", cx);
    dictation.update(cx, |dictation, cx| dictation.toggle(cx));
    cx.run_until_parked();
    fake.hear(Event::Partial("hasta aquí".into()));
    fake.hear(Event::Lost("The microphone stopped: unplugged".into()));
    cx.run_until_parked();
    assert_eq!(phase(&dictation, cx), Phase::Idle);
    assert_eq!(
        dictation.read_with(cx, |d, _| d.error().map(str::to_owned)),
        Some("The microphone stopped: unplugged".into())
    );
    assert_eq!(
        input.read_with(cx, |i, _| i.content.to_string()),
        "hasta aquí"
    );
    assert_eq!(fake.calls(), ["start", "stop 7"]);
}

#[gpui::test]
fn escape_drops_what_was_heard(cx: &mut TestAppContext) {
    let fake = Fake::installed("never lands");
    let (input, dictation) = dictation(&fake, "draft", cx);
    dictation.update(cx, |dictation, cx| dictation.toggle(cx));
    cx.run_until_parked();
    fake.hear(Event::Partial("never lands".into()));
    cx.run_until_parked();
    let escape = KeyDownEvent {
        keystroke: gpui::Keystroke::parse("escape").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    assert!(dictation.update(cx, |dictation, cx| dictation.escape(&escape, cx)));
    cx.run_until_parked();
    assert_eq!(phase(&dictation, cx), Phase::Idle);
    assert_eq!(dictation.read_with(cx, |d, _| d.partial().to_owned()), "");
    assert_eq!(input.read_with(cx, |i, _| i.content.to_string()), "draft");
    assert_eq!(fake.calls(), ["start", "cancel 7"]);
    // Idle, Escape is the composer's again.
    assert!(!dictation.update(cx, |dictation, cx| dictation.escape(&escape, cx)));
}

#[gpui::test]
fn cancelling_final_transcription_still_closes_the_microphone(cx: &mut TestAppContext) {
    let fake = Fake::installed("must not arrive");
    let (input, dictation) = dictation(&fake, "draft", cx);
    dictation.update(cx, |dictation, cx| dictation.toggle(cx));
    cx.run_until_parked();
    dictation.update(cx, |dictation, cx| {
        dictation.toggle(cx);
        assert_eq!(dictation.phase(), Phase::Finishing);
        dictation.cancel_pending(cx);
        assert_eq!(dictation.phase(), Phase::Idle);
    });
    cx.run_until_parked();
    assert_eq!(
        input.read_with(cx, |input, _| input.content.to_string()),
        "draft"
    );
    assert_eq!(fake.calls(), ["start", "stop 7"]);
}

#[gpui::test]
fn a_microphone_that_opens_after_escape_is_closed_again(cx: &mut TestAppContext) {
    let fake = Fake::installed("never lands");
    let (_, dictation) = dictation(&fake, "", cx);
    dictation.update(cx, |dictation, cx| {
        dictation.start(cx);
        assert_eq!(dictation.phase(), Phase::Starting);
        dictation.cancel(cx);
    });
    cx.run_until_parked();
    assert_eq!(phase(&dictation, cx), Phase::Idle);
    assert_eq!(fake.calls(), ["start", "cancel 7"]);

    // And when the composer closes before it opens.
    dictation.update(cx, |dictation, cx| dictation.start(cx));
    drop(dictation);
    cx.run_until_parked();
    assert_eq!(fake.calls(), ["start", "cancel 7", "start", "cancel 7"]);
}

#[gpui::test]
fn a_missing_model_is_offered_then_downloaded_and_listening_starts(cx: &mut TestAppContext) {
    let fake = Arc::new(Fake::default());
    let (_, dictation) = dictation(&fake, "", cx);
    dictation.update(cx, |dictation, cx| dictation.toggle(cx));
    cx.run_until_parked();
    assert_eq!(phase(&dictation, cx), Phase::NeedsModel);
    dictation.update(cx, |dictation, cx| dictation.dismiss(cx));
    assert_eq!(phase(&dictation, cx), Phase::Idle);

    dictation.update(cx, |dictation, cx| dictation.toggle(cx));
    cx.run_until_parked();
    dictation.update(cx, |dictation, cx| dictation.install(cx));
    assert_eq!(phase(&dictation, cx), Phase::Installing);
    cx.run_until_parked();
    assert_eq!(phase(&dictation, cx), Phase::Listening);
    assert_eq!(fake.calls(), ["install", "start"]);
}

#[gpui::test]
fn follows_the_setup_the_app_started(cx: &mut TestAppContext) {
    let fake = Arc::new(Fake::default());
    *fake.installing.lock().unwrap() = true;
    let (_, dictation) = dictation(&fake, "", cx);
    dictation.update(cx, |dictation, cx| dictation.toggle(cx));
    cx.run_until_parked();
    assert_eq!(phase(&dictation, cx), Phase::Installing);
    cx.executor().advance_clock(Duration::from_millis(600));
    cx.run_until_parked();
    assert_eq!(dictation.read_with(cx, |d, _| d.progress), 0.25);
    assert_eq!(phase(&dictation, cx), Phase::Installing);

    *fake.installing.lock().unwrap() = false;
    *fake.installed.lock().unwrap() = true;
    cx.executor().advance_clock(Duration::from_millis(600));
    cx.run_until_parked();
    assert_eq!(phase(&dictation, cx), Phase::Listening);
}

/// A Mac with the CPU build fetches the Neural one at launch; the mic does
/// not wait for it.
#[gpui::test]
fn dictates_with_the_model_on_disk_while_a_faster_one_downloads(cx: &mut TestAppContext) {
    let fake = Fake::installed("");
    *fake.installing.lock().unwrap() = true;
    let (_, dictation) = dictation(&fake, "", cx);
    dictation.update(cx, |dictation, cx| dictation.toggle(cx));
    cx.run_until_parked();
    assert_eq!(phase(&dictation, cx), Phase::Listening);
    assert_eq!(fake.calls(), ["start"]);
}

#[gpui::test]
fn a_microphone_that_will_not_open_says_why(cx: &mut TestAppContext) {
    let fake = Fake::installed("");
    *fake.start_error.lock().unwrap() = Some("Microphone: no microphone found".into());
    let (_, dictation) = dictation(&fake, "", cx);
    dictation.update(cx, |dictation, cx| dictation.toggle(cx));
    cx.run_until_parked();
    assert_eq!(phase(&dictation, cx), Phase::Idle);
    assert_eq!(
        dictation.read_with(cx, |d, _| d.error().map(str::to_owned)),
        Some("Microphone: no microphone found".into())
    );
    dictation.update(cx, |dictation, cx| dictation.dismiss(cx));
    assert!(dictation.read_with(cx, |d, _| d.error().is_none()));
}
