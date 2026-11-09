//! One dictation: the microphone through the voice detector, each finished
//! phrase transcribed as soon as the detector closes it. The microphone opens
//! at once; Parakeet loads and transcribes on a thread of its own
//! (`Decoder`), so the first words are kept while it loads and listening
//! never waits on a phrase being transcribed.
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, TryRecvError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sherpa_onnx::{LinearResampler, SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

use crate::capture::Microphone;
use crate::{model, text, Error};

const RATE: i32 = 16_000;
/// Kept ahead of each phrase the detector reports, so first consonants are
/// not clipped (Echo measured the same 0.2 s).
const PRE_ROLL: usize = 3_200;
/// Longest dictation kept in memory: ten minutes at 16 kHz.
const MAX_HISTORY: usize = 16_000 * 600;
/// How far past that the history grows before its oldest audio is cut, so a
/// long dictation moves the buffer twice a minute, not on every chunk.
const HISTORY_SLACK: usize = 16_000 * 30;

/// What was said in a phrase (mono, 16 kHz).
pub(crate) type Hear = Box<dyn Fn(&[f32]) -> String + Send>;
/// Loads the recognizer; runs on the decoding thread while the microphone
/// already listens.
pub(crate) type Loader = Box<dyn FnOnce() -> Result<Hear, Error> + Send>;

#[derive(Clone, Debug)]
pub enum Event {
    /// How loud the microphone is right now, 0 to 1.
    Level(f32),
    /// Everything transcribed so far.
    Partial(String),
    /// The voice detector heard speech start (true) or stop (false), so a
    /// call can tell when the user has finished their turn.
    Speech(bool),
    /// The microphone went away (unplugged, say). The session ends after
    /// this, and stop still returns what was heard.
    Lost(String),
    /// Sent once by a call's session: it decides when each turn ends.
    DecidesTurns,
    /// A call turn ended, with all that was said in it.
    TurnEnd(String),
}

/// What a call's session needs to tell when a turn ends.
#[derive(Clone)]
pub struct CallMode {
    /// "patient", "normal" or "quick".
    pub pace: String,
    pub smart_turn: Option<Arc<crate::smart_turn::Model>>,
}

/// How long the voice detector waits before it reports quiet.
const MIN_SILENCE_MS: u32 = 450;

pub struct Session {
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    worker: Option<JoinHandle<String>>,
}

impl Session {
    /// `longest_phrase` is the most seconds the recognizer that `load`
    /// brings takes at once.
    pub(crate) fn start(
        dir: &Path,
        load: Loader,
        longest_phrase: f32,
        call: Option<CallMode>,
        on_event: impl Fn(Event) + Send + 'static,
    ) -> Result<Self, Error> {
        let vad = VoiceActivityDetector::create(
            &silero(model::path(dir, "silero_vad.onnx")?, longest_phrase),
            30.0,
        )
        .ok_or(Error::Load)?;
        let mic = Microphone::open()?;
        let resampler = (mic.sample_rate as i32 != RATE)
            .then(|| LinearResampler::create(mic.sample_rate as i32, RATE).ok_or(Error::Load))
            .transpose()?;
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = Arc::new(AtomicBool::new(false));
        let decoder = Decoder::start(load, cancel.clone())?;
        let worker = {
            let stop = stop.clone();
            let cancel = cancel.clone();
            std::thread::Builder::new()
                .name("dictation".into())
                .spawn(move || {
                    let mut run = Run {
                        decoder,
                        vad,
                        resampler,
                        history: Vec::new(),
                        seen: 0,
                        delivered: 0,
                        phrases: Vec::new(),
                        speaking: false,
                        turn: call.map(|call| {
                            (
                                crate::turn::Tracker::new(&call.pace, MIN_SILENCE_MS),
                                call.smart_turn,
                            )
                        }),
                        turn_audio_from: 0,
                        clock: 0,
                    };
                    if run.turn.is_some() {
                        on_event(Event::DecidesTurns);
                    }
                    run.go(mic, &stop, &cancel, &on_event)
                })?
        };
        Ok(Self {
            stop,
            cancel,
            worker: Some(worker),
        })
    }

    /// Stops listening and returns the whole transcript.
    pub fn stop(mut self) -> String {
        self.stop.store(true, Ordering::Release);
        self.worker
            .take()
            .and_then(|worker| worker.join().ok())
            .unwrap_or_default()
    }

    /// Stops listening and drops what was said.
    pub fn cancel(mut self) {
        self.cancel.store(true, Ordering::Release);
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        self.stop.store(true, Ordering::Release);
    }
}

/// Parakeet on its own thread: it loads there, then transcribes the phrases
/// handed over, in order.
struct Decoder {
    /// Dropped once no more phrases will come, which ends the thread after
    /// the ones already queued.
    phrases: Option<mpsc::Sender<Vec<f32>>>,
    heard: mpsc::Receiver<Result<String, Error>>,
    /// Phrases handed over and not yet transcribed.
    pending: usize,
}

impl Decoder {
    fn start(load: Loader, cancel: Arc<AtomicBool>) -> std::io::Result<Self> {
        let (phrases, queued) = mpsc::channel::<Vec<f32>>();
        let (results, heard) = mpsc::channel();
        std::thread::Builder::new()
            .name("dictation-decode".into())
            .spawn(move || {
                let hear = match load() {
                    Ok(hear) => hear,
                    Err(error) => {
                        let _ = results.send(Err(error));
                        return;
                    }
                };
                for audio in queued {
                    // A cancelled dictation drops what is still queued.
                    if cancel.load(Ordering::Acquire) || results.send(Ok(hear(&audio))).is_err() {
                        return;
                    }
                }
            })?;
        Ok(Self {
            phrases: Some(phrases),
            heard,
            pending: 0,
        })
    }

    fn hear(&mut self, audio: Vec<f32>) {
        let sent = self
            .phrases
            .as_ref()
            .is_some_and(|phrases| phrases.send(audio).is_ok());
        self.pending += usize::from(sent);
    }

    /// No more phrases will come.
    fn close(&mut self) {
        self.phrases = None;
    }

    fn busy(&self) -> bool {
        self.pending > 0
    }

    /// The next phrase transcribed, or why the recognizer did not load,
    /// waiting up to `wait` for it.
    fn next(&mut self, wait: Duration) -> Option<Result<String, Error>> {
        let received = if wait.is_zero() {
            self.heard.try_recv().map_err(|error| match error {
                TryRecvError::Empty => RecvTimeoutError::Timeout,
                TryRecvError::Disconnected => RecvTimeoutError::Disconnected,
            })
        } else {
            self.heard.recv_timeout(wait)
        };
        match received {
            Ok(Ok(text)) => {
                self.pending = self.pending.saturating_sub(1);
                Some(Ok(text))
            }
            Ok(Err(error)) => {
                self.pending = 0;
                Some(Err(error))
            }
            Err(RecvTimeoutError::Timeout) => None,
            // The thread is gone: nothing else will be transcribed.
            Err(RecvTimeoutError::Disconnected) => {
                self.pending = 0;
                None
            }
        }
    }
}

struct Run {
    decoder: Decoder,
    vad: VoiceActivityDetector,
    resampler: Option<LinearResampler>,
    /// Every 16 kHz sample heard, for the pre-roll ahead of a phrase.
    history: Vec<f32>,
    /// Samples given to the detector so far.
    seen: usize,
    /// Where the last transcribed phrase ended, so pre-roll never repeats it.
    delivered: usize,
    phrases: Vec<String>,
    /// Whether the detector was hearing speech at the last check.
    speaking: bool,
    /// A call's turn, and Smart Turn when it is installed.
    turn: Option<(crate::turn::Tracker, Option<Arc<crate::smart_turn::Model>>)>,
    /// Where the current turn's audio starts, in heard samples.
    turn_audio_from: usize,
    /// Milliseconds of audio heard at the last turn check.
    clock: u64,
}

impl Run {
    fn go(
        &mut self,
        mut mic: Microphone,
        stop: &AtomicBool,
        cancel: &AtomicBool,
        on_event: &impl Fn(Event),
    ) -> String {
        let mut last_level = Instant::now();
        loop {
            if cancel.load(Ordering::Acquire) {
                return String::new();
            }
            if stop.load(Ordering::Acquire) {
                mic.close();
                while let Ok(chunk) = mic.chunks.try_recv() {
                    self.feed(&chunk, false);
                }
                self.feed(&[], true);
                self.vad.flush();
                self.hand_over();
                self.decoder.close();
                // The last phrases, and Parakeet itself when the dictation
                // was shorter than its load.
                while self.decoder.busy() {
                    if cancel.load(Ordering::Acquire) {
                        return String::new();
                    }
                    self.collect(Duration::from_millis(40), stop, on_event);
                }
                return text::join(&self.phrases);
            }
            match mic.chunks.recv_timeout(Duration::from_millis(40)) {
                Ok(chunk) => self.feed(&chunk, false),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    if let Some(reason) = mic.failure() {
                        on_event(Event::Lost(reason));
                    }
                    stop.store(true, Ordering::Release);
                    continue;
                }
            }
            if last_level.elapsed() >= Duration::from_millis(40) {
                on_event(Event::Level(mic.level()));
                last_level = Instant::now();
            }
            let speaking = self.vad.detected();
            if speaking != self.speaking {
                self.speaking = speaking;
                on_event(Event::Speech(speaking));
            }
            self.hand_over();
            self.collect(Duration::ZERO, stop, on_event);
            self.follow_turn(on_event);
        }
    }

    /// A call's turn: ends it after its pause, asking Smart Turn first.
    fn follow_turn(&mut self, on_event: &impl Fn(Event)) {
        let now = self.seen as u64 * 1_000 / RATE as u64;
        let ms = now.saturating_sub(self.clock) as u32;
        self.clock = now;
        let speaking = self.speaking;
        let seen = self.seen;
        let oldest = seen.saturating_sub(self.history.len());
        let Some((tracker, model)) = self.turn.as_mut() else {
            return;
        };
        if speaking && !tracker.in_turn() {
            self.turn_audio_from =
                seen.saturating_sub(PRE_ROLL + ms as usize * RATE as usize / 1_000);
        }
        tracker.heard(now, ms, speaking);
        if tracker.should_ask(now) {
            let verdict = model.as_ref().and_then(|model| {
                let from = self.turn_audio_from.max(oldest) - oldest;
                model.complete(&self.history[from..]).ok()
            });
            // Without the model, or when it fails, the words decide alone.
            tracker.verdict(verdict.unwrap_or(crate::turn::COMPLETE_AT));
        }
        // A phrase still being transcribed belongs to this turn: its words
        // come first.
        if self.decoder.busy() {
            return;
        }
        let partial = text::join(&self.phrases);
        if !tracker.done(now, &partial) {
            return;
        }
        tracker.reset();
        on_event(Event::TurnEnd(partial));
    }

    fn feed(&mut self, chunk: &[f32], flush: bool) {
        let resampled;
        let samples: &[f32] = match &self.resampler {
            Some(resampler) => {
                resampled = resampler.resample(chunk, flush);
                &resampled
            }
            None => chunk,
        };
        if samples.is_empty() {
            return;
        }
        self.seen += samples.len();
        self.history.extend_from_slice(samples);
        if self.history.len() > MAX_HISTORY + HISTORY_SLACK {
            // The oldest audio goes; offsets below stay in heard-sample units.
            let excess = self.history.len() - MAX_HISTORY;
            self.history.drain(..excess);
        }
        self.vad.accept_waveform(samples);
    }

    /// Hands every phrase the detector has closed to the decoder.
    fn hand_over(&mut self) {
        while let Some(segment) = self.vad.front() {
            let start = segment.start().max(0) as usize;
            let mut audio = self.pre_roll(start);
            audio.extend_from_slice(segment.samples());
            self.delivered = start + segment.samples().len();
            self.vad.pop();
            self.decoder.hear(audio);
        }
    }

    /// Takes in what the decoder has transcribed, waiting up to `wait` for
    /// the first phrase. A recognizer that did not load ends the dictation.
    fn collect(&mut self, wait: Duration, stop: &AtomicBool, on_event: &impl Fn(Event)) {
        let mut changed = false;
        let mut wait = wait;
        while let Some(result) = self.decoder.next(wait) {
            wait = Duration::ZERO;
            match result {
                Ok(heard) if heard.trim().is_empty() => {}
                Ok(heard) => {
                    self.phrases.push(heard);
                    changed = true;
                }
                Err(error) => {
                    on_event(Event::Lost(error.to_string()));
                    stop.store(true, Ordering::Release);
                }
            }
        }
        if changed {
            on_event(Event::Partial(text::join(&self.phrases)));
        }
    }

    fn pre_roll(&self, start: usize) -> Vec<f32> {
        let oldest = self.seen.saturating_sub(self.history.len());
        let end = start.clamp(oldest, self.seen);
        let begin = start
            .saturating_sub(PRE_ROLL)
            .max(self.delivered)
            .max(oldest)
            .min(end);
        self.history[begin - oldest..end - oldest].to_vec()
    }
}

fn silero(model: String, longest_phrase: f32) -> VadModelConfig {
    VadModelConfig {
        silero_vad: SileroVadModelConfig {
            model: Some(model),
            threshold: 0.4,
            min_silence_duration: MIN_SILENCE_MS as f32 / 1_000.0,
            min_speech_duration: 0.15,
            window_size: 512,
            max_speech_duration: longest_phrase,
        },
        sample_rate: RATE,
        num_threads: 1,
        provider: Some("cpu".into()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONG: Duration = Duration::from_secs(5);

    /// A recognizer that answers each phrase with its length.
    fn counting() -> Hear {
        Box::new(|samples| samples.len().to_string())
    }

    #[test]
    fn phrases_said_while_the_model_loads_are_heard_in_order() {
        let (loaded, wait) = mpsc::channel::<()>();
        let load: Loader = Box::new(move || {
            wait.recv().unwrap();
            Ok(counting())
        });
        let mut decoder = Decoder::start(load, Arc::default()).unwrap();
        for length in 1..=3 {
            decoder.hear(vec![0.; length]);
        }
        // Still loading: listening goes on and nothing is lost.
        assert!(decoder.busy());
        assert!(decoder.next(Duration::ZERO).is_none());
        loaded.send(()).unwrap();
        let heard: Vec<String> = (0..3)
            .map(|_| decoder.next(LONG).unwrap().unwrap())
            .collect();
        assert_eq!(heard, ["1", "2", "3"]);
        assert!(!decoder.busy());
    }

    #[test]
    fn a_model_that_does_not_load_says_why_and_leaves_nothing_waiting() {
        let mut decoder = Decoder::start(Box::new(|| Err(Error::Load)), Arc::default()).unwrap();
        decoder.hear(vec![0.; 4]);
        assert!(matches!(decoder.next(LONG), Some(Err(Error::Load))));
        assert!(!decoder.busy());
        // The thread is gone; stopping does not wait for it.
        decoder.close();
        assert!(decoder.next(LONG).is_none());
    }

    #[test]
    fn stopping_finishes_the_phrases_already_handed_over() {
        let mut decoder = Decoder::start(Box::new(|| Ok(counting())), Arc::default()).unwrap();
        decoder.hear(vec![0.; 7]);
        decoder.hear(vec![0.; 9]);
        decoder.close();
        let mut heard = Vec::new();
        while decoder.busy() {
            heard.extend(decoder.next(LONG).map(Result::unwrap));
        }
        assert_eq!(heard, ["7", "9"]);
    }

    #[test]
    fn a_cancelled_dictation_drops_what_is_still_queued() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (loaded, wait) = mpsc::channel::<()>();
        let load: Loader = Box::new(move || {
            wait.recv().unwrap();
            Ok(counting())
        });
        let mut decoder = Decoder::start(load, cancel.clone()).unwrap();
        decoder.hear(vec![0.; 2]);
        cancel.store(true, Ordering::Release);
        loaded.send(()).unwrap();
        assert!(decoder.next(LONG).is_none());
        assert!(!decoder.busy());
    }
}
