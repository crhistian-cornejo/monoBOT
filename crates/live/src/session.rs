//! One live conversation on its own thread: it connects, sends the setup,
//! streams the microphone, hands what comes back to the host, runs tools on
//! worker threads, and reconnects when the far end asks to move or drops.
//!
//! A tool that answers within `patience` (8 s) answers directly. A slower one
//! gets the protocol's "still running" meanwhile and its result later, as
//! IVY's `CONSULT_PATIENCE` does; at most `most_tools` (3) run at once.
use crate::transport::{Closed, Connect, Transport};
use crate::{Event, Protocol, ToolCall};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// What the app does with a session.
pub trait Host: Send + Sync + 'static {
    /// Everything the far end says but tool calls: audio, transcripts,
    /// interruptions, errors.
    fn event(&self, event: &Event);
    /// Where the call stands.
    fn state(&self, state: State);
    /// Runs a tool; it may take minutes. `cancelled` turns true when the
    /// model no longer wants it or the call ended.
    fn tool(&self, call: &ToolCall, cancelled: &AtomicBool) -> Value;
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Connecting,
    Live,
    Reconnecting,
    /// The call is over, and why when it did not end on the user's word.
    Ended(Option<String>),
}

#[derive(Clone, Debug)]
pub struct Options {
    /// How long a tool may take before the model hears it is still running.
    pub patience: Duration,
    /// How often to say so again, for a protocol that waits (Phonic).
    pub remind_every: Option<Duration>,
    pub most_tools: usize,
    /// How long the far end has to accept the setup.
    pub setup_timeout: Duration,
    /// Reconnects allowed in a minute before the call gives up.
    pub reconnects: usize,
    /// The wait before the first reconnect after a drop; each one after it
    /// waits twice as long, up to four seconds, so a network that is away
    /// for a moment has time to come back.
    pub backoff: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            patience: Duration::from_secs(8),
            remind_every: None,
            most_tools: 3,
            setup_timeout: Duration::from_secs(10),
            reconnects: 5,
            backoff: Duration::from_millis(500),
        }
    }
}

/// Lets a protocol change itself after a refusal (Gemini steps down its
/// model ladder) or resume a conversation (Gemini's handles).
pub trait Recover {
    /// The far end closed before accepting the setup: whether trying again
    /// may work.
    fn fall_back(&mut self, _reason: &str) -> bool {
        false
    }
    /// Whether a dropped connection can be resumed.
    fn resumes(&self) -> bool {
        false
    }
}

impl Recover for crate::gemini::Gemini {
    fn fall_back(&mut self, reason: &str) -> bool {
        if !crate::gemini::model_refused(reason) {
            return false;
        }
        match Self::next_model(&self.model) {
            Some(next) => {
                self.model = next.to_owned();
                true
            }
            None => false,
        }
    }

    fn resumes(&self) -> bool {
        true
    }
}

impl Recover for crate::phonic::Phonic {}

enum Out {
    Audio(Vec<f32>),
    /// What the user typed.
    Text(String),
}

/// Where the microphone sends what it hears. The capture thread keeps one
/// of its own, so the call's owner can hold, move and end the [`Session`]
/// without the microphone losing its way to it.
#[derive(Clone)]
pub struct Microphone(Sender<Out>);

impl Microphone {
    /// Microphone audio, mono at `INPUT_RATE`. After the call ends it goes
    /// nowhere.
    pub fn send(&self, samples: Vec<f32>) {
        let _ = self.0.send(Out::Audio(samples));
    }
}

pub struct Session {
    outgoing: Sender<Out>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Session {
    pub fn start<P: Protocol + Recover>(
        protocol: P,
        connect: impl Connect,
        host: Arc<dyn Host>,
        options: Options,
    ) -> Self {
        let (outgoing, incoming) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("live-call".into())
                .spawn(move || {
                    let mut run = Run {
                        protocol,
                        connect,
                        host,
                        options,
                        stop,
                        incoming,
                        handle: None,
                        tools: HashMap::new(),
                        done: mpsc::channel(),
                        reconnects: Vec::new(),
                        was_live: false,
                    };
                    let why = run.go();
                    run.cancel_tools();
                    run.host.state(State::Ended(why));
                })
                .ok()
        };
        Self {
            outgoing,
            stop,
            thread,
        }
    }

    /// Microphone audio, mono at `INPUT_RATE`.
    pub fn send_audio(&self, samples: Vec<f32>) {
        let _ = self.outgoing.send(Out::Audio(samples));
    }

    /// What the user typed instead of saying it, as a turn of theirs. A
    /// protocol that takes no text drops it.
    pub fn send_text(&self, text: String) {
        let _ = self.outgoing.send(Out::Text(text));
    }

    /// The way in for a microphone that runs on a thread of its own.
    pub fn microphone(&self) -> Microphone {
        Microphone(self.outgoing.clone())
    }

    pub fn live(&self) -> bool {
        self.thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
    }

    /// Ends the call and waits for its thread.
    pub fn hang_up(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

struct Running {
    call: ToolCall,
    cancelled: Arc<AtomicBool>,
    started: Instant,
    /// When "still running" was last said, if it was.
    reminded: Option<Instant>,
}

/// A finished tool: its call id and what it answered.
type Finished = (String, Value);

struct Run<P, C> {
    protocol: P,
    connect: C,
    host: Arc<dyn Host>,
    options: Options,
    stop: Arc<AtomicBool>,
    incoming: Receiver<Out>,
    handle: Option<String>,
    tools: HashMap<String, Running>,
    done: (Sender<Finished>, Receiver<Finished>),
    reconnects: Vec<Instant>,
    /// The call was live at least once: a connection that fails now is a
    /// drop to recover from, not a call that never began.
    was_live: bool,
}

/// Why a connection ended.
enum End {
    /// The user hung up.
    Stopped,
    /// The far end asked to move, or dropped; worth reconnecting.
    Moved,
    /// The far end refused this model before the call began; the next one
    /// down the ladder is tried at once.
    Stepped,
    /// It failed for good, in the user's words.
    Failed(String),
}

impl<P: Protocol + Recover, C: Connect> Run<P, C> {
    fn go(&mut self) -> Option<String> {
        self.host.state(State::Connecting);
        loop {
            match self.connection() {
                End::Stopped => return None,
                End::Failed(why) => return Some(why),
                end @ (End::Moved | End::Stepped) => {
                    let now = Instant::now();
                    self.reconnects
                        .retain(|at| now.duration_since(*at) < Duration::from_secs(60));
                    if self.reconnects.len() >= self.options.reconnects {
                        return Some("The live call kept dropping. Try again in a moment.".into());
                    }
                    self.reconnects.push(now);
                    self.host.state(State::Reconnecting);
                    // A drop waits a little longer each time; hanging up
                    // meanwhile ends the wait.
                    if matches!(end, End::Moved) {
                        let doubled = 1u32 << (self.reconnects.len() - 1).min(3);
                        let wait = (self.options.backoff * doubled).min(Duration::from_secs(4));
                        let until = now + wait;
                        while Instant::now() < until {
                            if self.stop.load(Ordering::Acquire) {
                                return None;
                            }
                            std::thread::sleep(Duration::from_millis(20).min(wait));
                        }
                    }
                }
            }
        }
    }

    fn connection(&mut self) -> End {
        let mut socket = match self
            .connect
            .connect(self.protocol.url(), &self.protocol.headers())
        {
            Ok(socket) => socket,
            // The network went away mid-call: try again, within the same
            // allowance, rather than ending a call that was going well.
            Err(_) if self.was_live && self.protocol.resumes() => return End::Moved,
            Err(why) => return End::Failed(why),
        };
        let resume = if self.protocol.resumes() {
            self.handle.as_deref()
        } else {
            None
        };
        if let Err(closed) = socket.send(&self.protocol.setup(resume)) {
            return self.refused(closed);
        }
        match self.until_ready(socket.as_mut()) {
            Ok(()) => {}
            Err(end) => return end,
        }
        // What the microphone heard while this connected was said to nobody:
        // sent now it would arrive late and all at once.
        while self.incoming.try_recv().is_ok() {}
        self.was_live = true;
        self.host.state(State::Live);
        let mut leaving = false;
        let mut last_audio = Instant::now();
        loop {
            if self.stop.load(Ordering::Acquire) {
                socket.close();
                return End::Stopped;
            }
            while let Ok(out) = self.incoming.try_recv() {
                let messages = match out {
                    Out::Audio(samples) => self.protocol.audio(&samples),
                    Out::Text(text) => self.protocol.text(&text).into_iter().collect(),
                };
                for message in messages {
                    if socket.send(&message).is_err() {
                        return End::Moved;
                    }
                }
            }
            match socket.recv(Duration::from_millis(20)) {
                Ok(Some(message)) => {
                    for event in self.protocol.parse(&message) {
                        match event {
                            Event::Tool(call) => {
                                if let Some(answer) = self.begin_tool(call) {
                                    let _ = socket.send(&answer);
                                }
                            }
                            Event::Cancelled(ids) => {
                                for id in ids {
                                    if let Some(running) = self.tools.remove(&id) {
                                        running.cancelled.store(true, Ordering::Release);
                                    }
                                }
                            }
                            Event::Resumable(handle) => self.handle = Some(handle),
                            Event::GoAway => leaving = true,
                            Event::Audio(_) => {
                                last_audio = Instant::now();
                                self.host.event(&event);
                            }
                            Event::Error(why) => {
                                self.host.event(&Event::Error(why.clone()));
                                return End::Failed(why);
                            }
                            other => self.host.event(&other),
                        }
                    }
                }
                Ok(None) => {}
                Err(closed) => {
                    return if self.protocol.resumes() && closed.code != 1008 {
                        End::Moved
                    } else {
                        End::Failed(describe(&closed))
                    };
                }
            }
            for message in self.tool_progress() {
                if socket.send(&message).is_err() {
                    return End::Moved;
                }
            }
            // Asked to move: at the next quiet moment, with the handle.
            if leaving && last_audio.elapsed() > Duration::from_millis(600) {
                socket.close();
                return End::Moved;
            }
        }
    }

    fn until_ready(&mut self, socket: &mut dyn Transport) -> Result<(), End> {
        let deadline = Instant::now() + self.options.setup_timeout;
        loop {
            if self.stop.load(Ordering::Acquire) {
                socket.close();
                return Err(End::Stopped);
            }
            if Instant::now() > deadline {
                return Err(End::Failed(
                    "The live voice service did not answer in time.".into(),
                ));
            }
            match socket.recv(Duration::from_millis(50)) {
                Ok(Some(message)) => {
                    for event in self.protocol.parse(&message) {
                        match event {
                            Event::Ready => return Ok(()),
                            Event::Error(why) => return Err(End::Failed(why)),
                            _ => {}
                        }
                    }
                }
                Ok(None) => {}
                Err(closed) => return Err(self.refused(closed)),
            }
        }
    }

    /// The far end closed before the call began.
    fn refused(&mut self, closed: Closed) -> End {
        if self.protocol.fall_back(&closed.reason) {
            End::Stepped
        } else {
            End::Failed(describe(&closed))
        }
    }

    /// Starts a tool on its own thread; returns an immediate answer when it
    /// cannot run.
    fn begin_tool(&mut self, call: ToolCall) -> Option<String> {
        if call.id.is_empty() {
            return None;
        }
        if self.tools.len() >= self.options.most_tools {
            return Some(self.protocol.tool_result(
                &call,
                &json!({"error": "Too many requests are still running. Tell the user to wait for one to finish."}),
            ));
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let (host, done, worker_call, flag) = (
            self.host.clone(),
            self.done.0.clone(),
            call.clone(),
            cancelled.clone(),
        );
        let spawned = std::thread::Builder::new()
            .name("live-tool".into())
            .spawn(move || {
                let result = host.tool(&worker_call, &flag);
                let _ = done.send((worker_call.id, result));
            });
        if spawned.is_err() {
            return Some(
                self.protocol
                    .tool_result(&call, &json!({"error": "Could not run it."})),
            );
        }
        self.tools.insert(
            call.id.clone(),
            Running {
                call,
                cancelled,
                started: Instant::now(),
                reminded: None,
            },
        );
        None
    }

    /// Results that arrived, and "still running" for those past patience.
    fn tool_progress(&mut self) -> Vec<String> {
        let mut messages = Vec::new();
        while let Ok((id, result)) = self.done.1.try_recv() {
            if let Some(running) = self.tools.remove(&id) {
                messages.push(if running.reminded.is_some() {
                    self.protocol.late_result(&running.call, &result)
                } else {
                    self.protocol.tool_result(&running.call, &result)
                });
            }
        }
        for running in self.tools.values_mut() {
            let due = match running.reminded {
                None => running.started.elapsed() >= self.options.patience,
                Some(at) => self
                    .options
                    .remind_every
                    .is_some_and(|every| at.elapsed() >= every),
            };
            if due && let Some(message) = self.protocol.still_running(&running.call) {
                messages.push(message);
                running.reminded = Some(Instant::now());
            } else if due {
                running.reminded = Some(Instant::now());
            }
        }
        messages
    }

    fn cancel_tools(&mut self) {
        for (_, running) in self.tools.drain() {
            running.cancelled.store(true, Ordering::Release);
        }
    }
}

fn describe(closed: &Closed) -> String {
    crate::gemini::close_reason(closed.code, &closed.reason)
}

/// A host that records what happens, for tests here and in the app.
#[derive(Default)]
pub struct Recorder {
    pub events: Mutex<Vec<Event>>,
    pub states: Mutex<Vec<State>>,
    pub tools: Mutex<Vec<ToolCall>>,
    /// How long each tool takes, and what it answers.
    pub delay: Mutex<Duration>,
    pub answer: Mutex<Value>,
}

impl Host for Recorder {
    fn event(&self, event: &Event) {
        self.events.lock().unwrap().push(event.clone());
    }

    fn state(&self, state: State) {
        self.states.lock().unwrap().push(state);
    }

    fn tool(&self, call: &ToolCall, cancelled: &AtomicBool) -> Value {
        self.tools.lock().unwrap().push(call.clone());
        let until = Instant::now() + *self.delay.lock().unwrap();
        while Instant::now() < until && !cancelled.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(2));
        }
        self.answer.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gemini::{Gemini, Tool};
    use crate::phonic::Phonic;

    /// The far end, played by the test: what the session sent, and what it
    /// will receive, one connection after another.
    #[derive(Clone, Default)]
    struct Far {
        sent: Arc<Mutex<Vec<Vec<String>>>>,
        /// Per connection: messages to deliver, or a close.
        scripts: Arc<Mutex<Vec<Vec<Step>>>>,
        live: Arc<Mutex<Option<Sender<Step>>>>,
        /// How many of the next connections fail, as with the network away.
        offline: Arc<Mutex<usize>>,
    }

    #[derive(Clone)]
    enum Step {
        Say(String),
        Close(Closed),
    }

    struct Line {
        sent: Arc<Mutex<Vec<Vec<String>>>>,
        index: usize,
        steps: Receiver<Step>,
    }

    impl Transport for Line {
        fn send(&mut self, message: &str) -> Result<(), Closed> {
            self.sent.lock().unwrap()[self.index].push(message.to_owned());
            Ok(())
        }
        fn recv(&mut self, wait: Duration) -> Result<Option<String>, Closed> {
            match self.steps.recv_timeout(wait) {
                Ok(Step::Say(message)) => Ok(Some(message)),
                Ok(Step::Close(closed)) => Err(closed),
                Err(_) => Ok(None),
            }
        }
    }

    impl Connect for Far {
        fn connect(
            &self,
            _: &str,
            _: &[(&'static str, String)],
        ) -> Result<Box<dyn Transport>, String> {
            {
                let mut offline = self.offline.lock().unwrap();
                if *offline > 0 {
                    *offline -= 1;
                    return Err("The network is unreachable.".into());
                }
            }
            let mut sent = self.sent.lock().unwrap();
            let index = sent.len();
            sent.push(Vec::new());
            let (tx, steps) = mpsc::channel();
            let script = {
                let mut scripts = self.scripts.lock().unwrap();
                if scripts.is_empty() {
                    Vec::new()
                } else {
                    scripts.remove(0)
                }
            };
            for step in script {
                tx.send(step).unwrap();
            }
            *self.live.lock().unwrap() = Some(tx);
            Ok(Box::new(Line {
                sent: self.sent.clone(),
                index,
                steps,
            }))
        }
    }

    impl Far {
        fn script(self, connections: Vec<Vec<Step>>) -> Self {
            *self.scripts.lock().unwrap() = connections;
            self
        }
        fn say(&self, message: &str) {
            self.live
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .send(Step::Say(message.into()))
                .unwrap();
        }
        fn sent(&self, connection: usize) -> Vec<String> {
            self.sent
                .lock()
                .unwrap()
                .get(connection)
                .cloned()
                .unwrap_or_default()
        }
        fn connections(&self) -> usize {
            self.sent.lock().unwrap().len()
        }
    }

    fn say(message: &str) -> Step {
        Step::Say(message.into())
    }

    fn gemini() -> Gemini {
        Gemini::new(
            "AIza-test",
            "On a call.",
            vec![Tool {
                name: "ask_agent".into(),
                description: "Ask.".into(),
                parameters: json!({"type": "object"}),
            }],
        )
    }

    fn quick() -> Options {
        Options {
            patience: Duration::from_millis(80),
            setup_timeout: Duration::from_millis(500),
            backoff: Duration::from_millis(5),
            ..Options::default()
        }
    }

    /// Waits until `check` holds, or fails after two seconds.
    fn eventually(what: &str, check: impl Fn() -> bool) {
        let until = Instant::now() + Duration::from_secs(2);
        while !check() {
            assert!(Instant::now() < until, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    const READY: &str = r#"{"setupComplete":{}}"#;

    #[test]
    fn setup_goes_first_then_audio_flows_both_ways() {
        let far = Far::default().script(vec![vec![say(READY)]]);
        let host = Arc::new(Recorder::default());
        let session = Session::start(gemini(), far.clone(), host.clone(), quick());
        eventually("live", || {
            host.states.lock().unwrap().contains(&State::Live)
        });
        assert!(far.sent(0)[0].contains("\"setup\""));
        session.send_audio(vec![0.1; 640]);
        eventually("two frames", || far.sent(0).len() == 3);
        assert!(far.sent(0)[1].contains("realtimeInput"));
        let audio = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            crate::pcm16(&[0.5; 4]),
        );
        far.say(&json!({"serverContent": {"modelTurn": {"parts": [{"inlineData": {"data": audio}}]}, "outputTranscription": {"text": "Hola"}}}).to_string());
        eventually("audio and words", || host.events.lock().unwrap().len() == 2);
        let events = host.events.lock().unwrap().clone();
        assert!(matches!(&events[0], Event::Audio(samples) if samples.len() == 4));
        assert_eq!(events[1], Event::Said("Hola".into()));
        session.hang_up();
        assert_eq!(
            host.states.lock().unwrap().last(),
            Some(&State::Ended(None))
        );
    }

    #[test]
    fn a_microphone_keeps_its_way_in_after_the_session_moves() {
        // No READY yet: the call is still connecting.
        let far = Far::default();
        let host = Arc::new(Recorder::default());
        let session = Session::start(gemini(), far.clone(), host.clone(), quick());
        let microphone = session.microphone();
        // The session goes to its owner, as the app hands it to the call.
        let owner = Mutex::new(Some(session));
        let session = owner.lock().unwrap().take().unwrap();
        eventually("the setup", || far.sent(0).len() == 1);
        // Heard while connecting: never sent, not even late.
        microphone.send(vec![0.1; 640]);
        far.say(READY);
        eventually("live", || {
            host.states.lock().unwrap().contains(&State::Live)
        });
        microphone.send(vec![0.2; 640]);
        eventually("two frames", || far.sent(0).len() == 3);
        std::thread::sleep(Duration::from_millis(60));
        let sent = far.sent(0);
        assert_eq!(sent.len(), 3, "only what was heard once live");
        assert!(sent[1].contains("realtimeInput") && sent[2].contains("realtimeInput"));
        session.hang_up();
        // After the call it goes nowhere, and does not fail.
        microphone.send(vec![0.3; 640]);
    }

    #[test]
    fn a_quick_tool_answers_directly() {
        let far = Far::default().script(vec![vec![say(READY)]]);
        let host = Arc::new(Recorder::default());
        *host.answer.lock().unwrap() = json!({"answer": "done"});
        let session = Session::start(gemini(), far.clone(), host.clone(), quick());
        eventually("live", || {
            host.states.lock().unwrap().contains(&State::Live)
        });
        far.say(r#"{"toolCall":{"functionCalls":[{"id":"c1","name":"ask_agent","args":{"request":"x"}}]}}"#);
        eventually("the answer", || {
            far.sent(0).iter().any(|m| m.contains("toolResponse"))
        });
        let answer = far
            .sent(0)
            .into_iter()
            .find(|m| m.contains("toolResponse"))
            .unwrap();
        assert!(answer.contains("\"done\"") && answer.contains("\"c1\""));
        assert!(
            !far.sent(0).iter().any(|m| m.contains("running")),
            "no interim"
        );
        assert_eq!(host.tools.lock().unwrap()[0].args, json!({"request": "x"}));
        session.hang_up();
    }

    #[test]
    fn a_slow_tool_says_it_is_running_then_reports_late() {
        let far = Far::default().script(vec![vec![say(READY)]]);
        let host = Arc::new(Recorder::default());
        *host.delay.lock().unwrap() = Duration::from_millis(250);
        *host.answer.lock().unwrap() = json!({"answer": "found it"});
        let session = Session::start(gemini(), far.clone(), host.clone(), quick());
        eventually("live", || {
            host.states.lock().unwrap().contains(&State::Live)
        });
        far.say(r#"{"toolCall":{"functionCalls":[{"id":"c1","name":"ask_agent","args":{}}]}}"#);
        eventually("running", || {
            far.sent(0).iter().any(|m| m.contains("\"running\""))
        });
        eventually("late note", || {
            far.sent(0).iter().any(|m| m.contains("clientContent"))
        });
        let late = far
            .sent(0)
            .into_iter()
            .find(|m| m.contains("clientContent"))
            .unwrap();
        assert!(late.contains("found it"));
        session.hang_up();
    }

    #[test]
    fn too_many_tools_at_once_are_refused_and_cancelled_ones_stay_quiet() {
        let far = Far::default().script(vec![vec![say(READY)]]);
        let host = Arc::new(Recorder::default());
        *host.delay.lock().unwrap() = Duration::from_secs(5);
        let options = Options {
            most_tools: 1,
            patience: Duration::from_secs(10),
            ..quick()
        };
        let session = Session::start(gemini(), far.clone(), host.clone(), options);
        eventually("live", || {
            host.states.lock().unwrap().contains(&State::Live)
        });
        far.say(r#"{"toolCall":{"functionCalls":[{"id":"a","name":"ask_agent","args":{}},{"id":"b","name":"ask_agent","args":{}}]}}"#);
        eventually("b refused", || {
            far.sent(0).iter().any(|m| m.contains("Too many"))
        });
        far.say(r#"{"toolCallCancellation":{"ids":["a"]}}"#);
        // The cancelled tool stops early, and its answer is never sent.
        eventually("a ran", || host.tools.lock().unwrap().len() == 1);
        std::thread::sleep(Duration::from_millis(100));
        let answers = far
            .sent(0)
            .iter()
            .filter(|m| m.contains("toolResponse"))
            .count();
        assert_eq!(answers, 1, "only b's refusal");
        session.hang_up();
    }

    #[test]
    fn go_away_reconnects_with_the_resumption_handle() {
        let far = Far::default().script(vec![
            vec![
                say(READY),
                say(r#"{"sessionResumptionUpdate":{"newHandle":"h-9","resumable":true}}"#),
                say(r#"{"goAway":{"timeLeft":"5s"}}"#),
            ],
            vec![say(READY)],
        ]);
        let host = Arc::new(Recorder::default());
        let session = Session::start(gemini(), far.clone(), host.clone(), quick());
        eventually("second connection", || {
            far.connections() == 2 && !far.sent(1).is_empty()
        });
        assert!(far.sent(1)[0].contains("\"handle\":\"h-9\""));
        eventually("live again", || {
            host.states
                .lock()
                .unwrap()
                .iter()
                .filter(|s| **s == State::Live)
                .count()
                == 2
        });
        assert!(host.states.lock().unwrap().contains(&State::Reconnecting));
        session.hang_up();
    }

    #[test]
    fn a_network_that_is_away_for_a_moment_does_not_end_the_call() {
        let far = Far::default().script(vec![vec![say(READY)], vec![say(READY)]]);
        let host = Arc::new(Recorder::default());
        let session = Session::start(gemini(), far.clone(), host.clone(), quick());
        eventually("live", || {
            host.states.lock().unwrap().contains(&State::Live)
        });
        // The line drops, and the first two tries to come back find no
        // network.
        *far.offline.lock().unwrap() = 2;
        far.say(r#"{"goAway":{"timeLeft":"5s"}}"#);
        eventually("live again", || {
            host.states
                .lock()
                .unwrap()
                .iter()
                .filter(|state| **state == State::Live)
                .count()
                >= 2
        });
        assert!(
            !host
                .states
                .lock()
                .unwrap()
                .iter()
                .any(|state| matches!(state, State::Ended(_))),
            "the call went on"
        );
        session.hang_up();
    }

    #[test]
    fn a_call_that_cannot_begin_says_so_at_once() {
        let far = Far::default();
        *far.offline.lock().unwrap() = 1;
        let host = Arc::new(Recorder::default());
        let session = Session::start(gemini(), far.clone(), host.clone(), quick());
        eventually("ended", || {
            matches!(
                host.states.lock().unwrap().last(),
                Some(State::Ended(Some(why))) if why.contains("unreachable")
            )
        });
        assert_eq!(far.connections(), 0);
        drop(session);
    }

    #[test]
    fn typed_text_goes_up_as_a_turn_of_the_users() {
        let far = Far::default().script(vec![vec![say(READY)]]);
        let host = Arc::new(Recorder::default());
        let session = Session::start(gemini(), far.clone(), host.clone(), quick());
        eventually("live", || {
            host.states.lock().unwrap().contains(&State::Live)
        });
        session.send_text("¿Cuánto gasté?".into());
        eventually("the turn", || far.sent(0).len() == 2);
        let turn: serde_json::Value = serde_json::from_str(&far.sent(0)[1]).unwrap();
        assert_eq!(
            turn["clientContent"]["turns"][0]["parts"][0]["text"],
            "¿Cuánto gasté?"
        );
        assert_eq!(turn["clientContent"]["turnComplete"], true);
        session.hang_up();
    }

    #[test]
    fn a_refused_model_steps_down_the_ladder() {
        let gone = Closed {
            code: 1008,
            reason: "models/gemini-3.8-live is not found".into(),
        };
        let far = Far::default().script(vec![vec![Step::Close(gone)], vec![say(READY)]]);
        let host = Arc::new(Recorder::default());
        let session = Session::start(gemini(), far.clone(), host.clone(), quick());
        eventually("live", || {
            host.states.lock().unwrap().contains(&State::Live)
        });
        assert!(far.sent(0)[0].contains(crate::gemini::MODELS[0]));
        assert!(far.sent(1)[0].contains(crate::gemini::MODELS[1]));
        session.hang_up();
    }

    #[test]
    fn a_refused_key_ends_the_call_saying_why() {
        let refused = Closed {
            code: 1008,
            reason: "API key not valid. Please pass a valid API key.".into(),
        };
        let far = Far::default().script(vec![vec![Step::Close(refused)]]);
        let host = Arc::new(Recorder::default());
        let session = Session::start(gemini(), far.clone(), host.clone(), quick());
        eventually("ended", || {
            matches!(
                host.states.lock().unwrap().last(),
                Some(State::Ended(Some(_)))
            )
        });
        let Some(State::Ended(Some(why))) = host.states.lock().unwrap().last().cloned() else {
            unreachable!()
        };
        assert!(why.contains("Settings › Voice"), "{why}");
        assert_eq!(far.connections(), 1, "no retry for a bad key");
        drop(session);
    }

    #[test]
    fn a_far_end_that_never_answers_times_out() {
        let far = Far::default();
        let host = Arc::new(Recorder::default());
        let options = Options {
            setup_timeout: Duration::from_millis(100),
            ..quick()
        };
        let session = Session::start(gemini(), far, host.clone(), options);
        eventually("ended", || {
            matches!(
                host.states.lock().unwrap().last(),
                Some(State::Ended(Some(_)))
            )
        });
        drop(session);
    }

    #[test]
    fn phonic_reminds_while_a_tool_runs_and_answers_in_place() {
        let far =
            Far::default().script(vec![vec![say(r#"{"type":"ready_to_start_conversation"}"#)]]);
        let host = Arc::new(Recorder::default());
        *host.delay.lock().unwrap() = Duration::from_millis(300);
        *host.answer.lock().unwrap() = json!({"result": "ok"});
        let options = Options {
            remind_every: Some(Duration::from_millis(60)),
            ..quick()
        };
        let session = Session::start(
            Phonic::new("ph-key-test", "Hi.", Vec::new()),
            far.clone(),
            host.clone(),
            options,
        );
        eventually("live", || {
            host.states.lock().unwrap().contains(&State::Live)
        });
        far.say(
            r#"{"type":"tool_call","tool_call_id":"t1","tool_name":"ask_agent","parameters":{}}"#,
        );
        eventually("answer", || {
            far.sent(0).iter().any(|m| m.contains("tool_call_output"))
        });
        let says = far.sent(0).iter().filter(|m| m.contains("\"say\"")).count();
        assert!(says >= 2, "reminded more than once: {says}");
        session.hang_up();
    }

    #[test]
    fn a_dropped_phonic_call_ends_rather_than_resuming() {
        let far = Far::default().script(vec![vec![
            say(r#"{"type":"ready_to_start_conversation"}"#),
            Step::Close(Closed {
                code: 1006,
                reason: "reset".into(),
            }),
        ]]);
        let host = Arc::new(Recorder::default());
        let session = Session::start(
            Phonic::new("ph-key-test", "Hi.", Vec::new()),
            far.clone(),
            host.clone(),
            quick(),
        );
        eventually("ended", || {
            matches!(
                host.states.lock().unwrap().last(),
                Some(State::Ended(Some(_)))
            )
        });
        assert_eq!(far.connections(), 1);
        drop(session);
    }
}
