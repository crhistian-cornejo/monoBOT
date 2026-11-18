//! One dictation: the microphone through the voice detector, each finished
//! phrase transcribed as soon as the detector closes it. The microphone opens
//! at once; Parakeet loads and transcribes on a thread of its own
//! (`Decoder`), so the first words are kept while it loads and listening
//! never waits on a phrase being transcribed.
//!
//! Stopping keeps listening a moment (the release tail), so the last
//! syllable said as the key comes up is heard. A dictation short enough for
//! one pass of the recognizer is then heard again whole, which reads better
//! than its phrases transcribed apart.
use std::ops::Range;
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
/// How long listening goes on once stop is asked.
const TAIL: Duration = Duration::from_millis(200);
/// How long it may go on while the voice detector still hears speech.
const LONGEST_TAIL: Duration = Duration::from_millis(600);

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

/// What a dictation can be told when it starts.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// The user's own words, put in the transcript as they write them.
    pub vocabulary: Vec<text::Replacement>,
}

/// How long the voice detector waits before it reports quiet.
const MIN_SILENCE_MS: u32 = 450;
/// How often the phrase being spoken is heard for the live caption, at
/// most; a recognizer slower than half of this is asked less often.
const PREVIEW_EVERY: Duration = Duration::from_millis(350);
/// The least of an open phrase worth previewing, and the most: its last
/// seconds, so a slow recognizer answers in time.
const PREVIEW_LEAST: usize = RATE as usize * 6 / 10;
const PREVIEW_MOST: usize = RATE as usize * 8;

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
        options: Options,
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
                        first_heard: None,
                        longest: (longest_phrase * RATE as f32) as usize,
                        vocabulary: options.vocabulary,
                        preview_sent: None,
                        preview_took: None,
                        preview_from: 0,
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
/// What the decoder thread is asked to hear.
enum Job {
    /// A closed phrase, kept in the transcript.
    Phrase(Vec<f32>),
    /// The phrase still being spoken, for the live caption only.
    Preview(Vec<f32>),
}

struct Decoder {
    /// Dropped once no more phrases will come, which ends the thread after
    /// the ones already queued.
    phrases: Option<mpsc::Sender<Job>>,
    heard: mpsc::Receiver<Result<String, Error>>,
    previews: mpsc::Receiver<String>,
    /// Phrases handed over and not yet transcribed.
    pending: usize,
    /// A preview is queued or being heard; one at a time.
    previewing: bool,
}

impl Decoder {
    fn start(load: Loader, cancel: Arc<AtomicBool>) -> std::io::Result<Self> {
        let (phrases, queued) = mpsc::channel::<Job>();
        let (results, heard) = mpsc::channel();
        let (previewed, previews) = mpsc::channel();
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
                let mut next: Option<Job> = None;
                loop {
                    let job = match next.take() {
                        Some(job) => job,
                        None => match queued.recv() {
                            Ok(job) => job,
                            Err(_) => return,
                        },
                    };
                    // A cancelled dictation drops what is still queued.
                    if cancel.load(Ordering::Acquire) {
                        return;
                    }
                    match job {
                        Job::Phrase(audio) => {
                            if results.send(Ok(hear(&audio))).is_err() {
                                return;
                            }
                        }
                        // A preview with anything behind it is already old,
                        // and must not hold up a phrase: skip it.
                        Job::Preview(audio) => match queued.try_recv() {
                            Ok(after) => {
                                next = Some(after);
                                let _ = previewed.send(String::new());
                            }
                            Err(_) => {
                                let _ = previewed.send(hear(&audio));
                            }
                        },
                    }
                }
            })?;
        Ok(Self {
            phrases: Some(phrases),
            heard,
            previews,
            pending: 0,
            previewing: false,
        })
    }

    fn hear(&mut self, audio: Vec<f32>) {
        let sent = self
            .phrases
            .as_ref()
            .is_some_and(|phrases| phrases.send(Job::Phrase(audio)).is_ok());
        self.pending += usize::from(sent);
    }

    /// Hears the phrase still being spoken, unless a preview is in flight.
    fn preview(&mut self, audio: Vec<f32>) -> bool {
        if self.previewing {
            return false;
        }
        self.previewing = self
            .phrases
            .as_ref()
            .is_some_and(|phrases| phrases.send(Job::Preview(audio)).is_ok());
        self.previewing
    }

    /// The preview's words, once heard.
    fn previewed(&mut self) -> Option<String> {
        let words = self.previews.try_recv().ok()?;
        self.previewing = false;
        Some(words)
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
    /// Where the first phrase started, in heard samples.
    first_heard: Option<usize>,
    /// The most samples the recognizer takes at once.
    longest: usize,
    vocabulary: Vec<text::Replacement>,
    /// When the live caption last asked for the open phrase, how long the
    /// last answer took, and where that preview's audio began.
    preview_sent: Option<Instant>,
    preview_took: Option<Duration>,
    preview_from: usize,
}

impl Run {
    fn go(
        &mut self,
        mic: Microphone,
        stop: &AtomicBool,
        cancel: &AtomicBool,
        on_event: &impl Fn(Event),
    ) -> String {
        let mut last_level = Instant::now();
        // When stop was asked; the microphone is read a while longer.
        let mut stopping: Option<Instant> = None;
        let mut mic_gone = false;
        loop {
            if cancel.load(Ordering::Acquire) {
                return String::new();
            }
            if stop.load(Ordering::Acquire) {
                let since = *stopping.get_or_insert_with(Instant::now);
                if mic_gone || tail_over(since.elapsed(), self.vad.detected()) {
                    return self.finish(mic, stop, cancel, on_event);
                }
            }
            match mic.chunks.recv_timeout(Duration::from_millis(40)) {
                Ok(chunk) => self.feed(&chunk, false),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    if let Some(reason) = mic.failure() {
                        on_event(Event::Lost(reason));
                    }
                    mic_gone = true;
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
            // Once the user stopped, the turn ends with the session.
            if stopping.is_none() {
                self.follow_turn(on_event);
                if self.turn.is_none() {
                    self.live_caption(on_event);
                }
            }
        }
    }

    /// The end of the dictation: the last phrases, Parakeet itself when the
    /// dictation was shorter than its load, and the whole clip heard again
    /// when it fits in one pass.
    fn finish(
        &mut self,
        mut mic: Microphone,
        stop: &AtomicBool,
        cancel: &AtomicBool,
        on_event: &impl Fn(Event),
    ) -> String {
        mic.close();
        while let Ok(chunk) = mic.chunks.try_recv() {
            self.feed(&chunk, false);
        }
        self.feed(&[], true);
        self.vad.flush();
        self.hand_over();
        let clip = self.whole_clip();
        while self.decoder.busy() {
            if cancel.load(Ordering::Acquire) {
                return String::new();
            }
            self.collect(Duration::from_millis(40), stop, on_event);
        }
        let whole = clip.and_then(|clip| second_pass(&mut self.decoder, clip, cancel));
        self.decoder.close();
        if cancel.load(Ordering::Acquire) {
            return String::new();
        }
        match whole {
            Some(whole) => final_text(&text::sentences(&whole), &self.vocabulary),
            None => self.joined(),
        }
    }

    /// Everything heard so far, as the user reads it.
    fn joined(&self) -> String {
        if self.turn.is_some() {
            text::apply_vocabulary(&text::join_call(&self.phrases), &self.vocabulary)
        } else {
            final_text(&self.phrases, &self.vocabulary)
        }
    }

    /// The whole dictation's audio, from just before its first phrase, when
    /// all of it is still kept and the recognizer takes it at once. Not for
    /// calls, whose turns are heard as they end.
    fn whole_clip(&self) -> Option<Vec<f32>> {
        if self.turn.is_some() {
            return None;
        }
        let range = clip_range(
            self.first_heard?,
            self.seen,
            self.history.len(),
            self.longest,
        )?;
        Some(self.history[range].to_vec())
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
        let partial = text::apply_vocabulary(&text::join_call(&self.phrases), &self.vocabulary);
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
            self.first_heard.get_or_insert(start);
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
            on_event(Event::Partial(self.joined()));
        }
    }

    /// Words as they are said: the open phrase is heard again every
    /// moment and shown after the phrases already transcribed, never kept.
    /// The phrase itself is transcribed when the detector closes it.
    fn live_caption(&mut self, on_event: &impl Fn(Event)) {
        if let Some(words) = self.decoder.previewed() {
            self.preview_took = self.preview_sent.map(|sent| sent.elapsed());
            // A preview of audio already handed over as a phrase is stale.
            if let Some(shown) =
                live_text(&self.joined(), &words, self.preview_from >= self.delivered)
            {
                on_event(Event::Partial(shown));
            }
        }
        let every = self
            .preview_took
            .map_or(PREVIEW_EVERY, |took| (took * 2).max(PREVIEW_EVERY));
        if !self.speaking
            || self.decoder.busy()
            || self.preview_sent.is_some_and(|sent| sent.elapsed() < every)
        {
            return;
        }
        let oldest = self.seen.saturating_sub(self.history.len());
        let from = self
            .delivered
            .max(oldest)
            .max(self.seen.saturating_sub(self.longest.min(PREVIEW_MOST)));
        if self.seen.saturating_sub(from) < PREVIEW_LEAST {
            return;
        }
        if self.decoder.preview(self.history[from - oldest..].to_vec()) {
            self.preview_sent = Some(Instant::now());
            self.preview_from = from;
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

/// Whether the release tail is over: `TAIL` after stop, or up to
/// `LONGEST_TAIL` while the voice detector still hears speech.
/// The live caption: the transcript so far and the words of the phrase
/// still being spoken, or nothing for a stale or empty preview.
fn live_text(joined: &str, words: &str, fresh: bool) -> Option<String> {
    let words = words.trim();
    if !fresh || words.is_empty() {
        return None;
    }
    Some(if joined.is_empty() {
        words.to_owned()
    } else {
        format!("{joined} {words}")
    })
}

fn tail_over(since_stop: Duration, speaking: bool) -> bool {
    since_stop >= LONGEST_TAIL || (since_stop >= TAIL && !speaking)
}

/// Where in the history (which holds the last `kept` of `seen` samples) the
/// dictation's audio lies, from the pre-roll before its first phrase at
/// `first`; none when part of it was cut or it is longer than `longest`.
fn clip_range(first: usize, seen: usize, kept: usize, longest: usize) -> Option<Range<usize>> {
    let oldest = seen.saturating_sub(kept);
    let begin = first.saturating_sub(PRE_ROLL);
    (begin >= oldest && begin <= seen && seen - begin <= longest).then(|| begin - oldest..kept)
}

/// The whole clip through the recognizer, after the phrases already queued;
/// none when it heard nothing, failed or the dictation was cancelled.
fn second_pass(decoder: &mut Decoder, clip: Vec<f32>, cancel: &AtomicBool) -> Option<String> {
    decoder.hear(clip);
    while decoder.busy() {
        if cancel.load(Ordering::Acquire) {
            return None;
        }
        match decoder.next(Duration::from_millis(40)) {
            Some(Ok(heard)) => return Some(heard).filter(|heard| !heard.trim().is_empty()),
            Some(Err(_)) => return None,
            None => {}
        }
    }
    None
}

/// A dictation's text: phrases joined, then the user's vocabulary.
fn final_text(phrases: &[String], vocabulary: &[text::Replacement]) -> String {
    text::apply_vocabulary(&text::join(phrases), vocabulary)
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
    fn a_preview_is_heard_apart_and_never_counts_as_a_phrase() {
        let mut decoder = Decoder::start(Box::new(|| Ok(counting())), Arc::default()).unwrap();
        decoder.hear(vec![0.; 2]);
        assert!(decoder.preview(vec![0.; 5]));
        // One preview at a time.
        assert!(!decoder.preview(vec![0.; 6]));
        assert_eq!(decoder.next(LONG).unwrap().unwrap(), "2");
        assert!(!decoder.busy(), "a preview never holds up the finish");
        let deadline = Instant::now() + LONG;
        let words = loop {
            if let Some(words) = decoder.previewed() {
                break words;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(words, "5");
        assert!(decoder.preview(vec![0.; 7]), "the next one may go");
    }

    #[test]
    fn a_preview_with_a_phrase_behind_it_is_skipped() {
        let (loaded, wait) = mpsc::channel::<()>();
        let load: Loader = Box::new(move || {
            wait.recv().unwrap();
            Ok(counting())
        });
        let mut decoder = Decoder::start(load, Arc::default()).unwrap();
        assert!(decoder.preview(vec![0.; 9]));
        decoder.hear(vec![0.; 3]);
        loaded.send(()).unwrap();
        assert_eq!(decoder.next(LONG).unwrap().unwrap(), "3");
        let deadline = Instant::now() + LONG;
        let words = loop {
            if let Some(words) = decoder.previewed() {
                break words;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(words, "", "skipped, not heard");
    }

    #[test]
    fn the_live_caption_follows_the_transcript_and_drops_stale_words() {
        assert_eq!(live_text("", " hola ", true).as_deref(), Some("hola"));
        assert_eq!(
            live_text("Buenos días.", "qué tal", true).as_deref(),
            Some("Buenos días. qué tal")
        );
        assert_eq!(live_text("Buenos días.", "qué tal", false), None);
        assert_eq!(live_text("Buenos días.", "  ", true), None);
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

    #[test]
    fn listening_goes_on_a_moment_after_stop_and_longer_while_speech_goes_on() {
        let ms = Duration::from_millis;
        assert!(!tail_over(ms(0), false));
        assert!(!tail_over(ms(150), false));
        assert!(tail_over(ms(200), false));
        // Still talking: up to 600 ms, never longer.
        assert!(!tail_over(ms(200), true));
        assert!(!tail_over(ms(550), true));
        assert!(tail_over(ms(600), true));
        assert!(tail_over(ms(2_000), true));
    }

    #[test]
    fn the_whole_clip_is_heard_again_only_when_it_fits_and_is_all_kept() {
        let second = RATE as usize;
        let longest = 20 * second;
        // Speech from 1 s on, 5 s heard, all kept: from the pre-roll to now.
        assert_eq!(
            clip_range(second, 5 * second, 5 * second, longest),
            Some(second - PRE_ROLL..5 * second)
        );
        // The pre-roll never reaches before the start.
        assert_eq!(
            clip_range(100, 3 * second, 3 * second, longest),
            Some(0..3 * second)
        );
        // Longer than the recognizer's window: the phrases stand.
        assert_eq!(clip_range(second, 25 * second, 25 * second, longest), None);
        // Its start was cut from the history.
        assert_eq!(clip_range(second, 15 * second, 10 * second, longest), None);
        // A history that was cut, with the dictation inside what is kept.
        assert_eq!(
            clip_range(12 * second, 15 * second, 10 * second, longest),
            Some(7 * second - PRE_ROLL..10 * second)
        );
    }

    #[test]
    fn the_second_pass_hears_the_clip_after_the_phrases_queued() {
        let mut decoder = Decoder::start(Box::new(|| Ok(counting())), Arc::default()).unwrap();
        decoder.hear(vec![0.; 3]);
        assert_eq!(decoder.next(LONG).unwrap().unwrap(), "3");
        let whole = second_pass(&mut decoder, vec![0.; 11], &AtomicBool::new(false));
        assert_eq!(whole.as_deref(), Some("11"));
        assert!(!decoder.busy());
    }

    #[test]
    fn a_second_pass_that_hears_nothing_leaves_the_phrases() {
        let silent: Loader = Box::new(|| Ok(Box::new(|_: &[f32]| " ".to_owned()) as Hear));
        let mut decoder = Decoder::start(silent, Arc::default()).unwrap();
        assert_eq!(
            second_pass(&mut decoder, vec![0.; 8], &AtomicBool::new(false)),
            None
        );
        // A recognizer that did not load leaves them too.
        let mut broken = Decoder::start(Box::new(|| Err(Error::Load)), Arc::default()).unwrap();
        assert_eq!(
            second_pass(&mut broken, vec![0.; 8], &AtomicBool::new(false)),
            None
        );
        // A cancelled dictation does not wait for it.
        let (_loaded, wait) = mpsc::channel::<()>();
        let stuck: Loader = Box::new(move || {
            let _ = wait.recv();
            Ok(counting())
        });
        let mut decoder = Decoder::start(stuck, Arc::default()).unwrap();
        assert_eq!(
            second_pass(&mut decoder, vec![0.; 8], &AtomicBool::new(true)),
            None
        );
    }

    #[test]
    fn the_final_text_is_tidied_and_takes_the_users_words() {
        let vocabulary = [text::Replacement {
            heard: "mono bot".into(),
            write: "monoBOT".into(),
        }];
        // The whole clip, one transcript, is read sentence by sentence.
        let whole = "hola, abre mono bot. Can you check the build?";
        assert_eq!(
            final_text(&text::sentences(whole), &vocabulary),
            "Hola, abre monoBOT. Can you check the build?"
        );
        let phrases = ["el el informe de mono bot.".to_owned()];
        assert_eq!(final_text(&phrases, &vocabulary), "El informe de monoBOT.");
        assert_eq!(final_text(&phrases, &[]), "El informe de mono bot.");
    }
}
