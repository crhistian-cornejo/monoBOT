//! One dictation: the microphone through the voice detector, each finished
//! phrase transcribed as soon as the detector closes it.
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sherpa_onnx::{
    LinearResampler, OfflineRecognizer, SileroVadModelConfig, VadModelConfig, VoiceActivityDetector,
};

use crate::capture::Microphone;
use crate::{model, text, Error};

const RATE: i32 = 16_000;
/// Kept ahead of each phrase the detector reports, so first consonants are
/// not clipped (Echo measured the same 0.2 s).
const PRE_ROLL: usize = 3_200;
/// Longest dictation kept in memory: ten minutes at 16 kHz.
const MAX_HISTORY: usize = 16_000 * 600;

#[derive(Clone, Debug)]
pub enum Event {
    /// How loud the microphone is right now, 0 to 1.
    Level(f32),
    /// Everything transcribed so far.
    Partial(String),
    /// The voice detector heard speech start (true) or stop (false), so a
    /// call can tell when the user has finished their turn.
    Speech(bool),
}

pub struct Session {
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    worker: Option<JoinHandle<String>>,
}

impl Session {
    pub(crate) fn start(
        dir: &Path,
        recognizer: Arc<OfflineRecognizer>,
        on_event: impl Fn(Event) + Send + 'static,
    ) -> Result<Self, Error> {
        let vad = VoiceActivityDetector::create(&silero(model::path(dir, "silero_vad.onnx")?), 30.0)
            .ok_or(Error::Load)?;
        let mic = Microphone::open()?;
        let resampler = (mic.sample_rate as i32 != RATE)
            .then(|| LinearResampler::create(mic.sample_rate as i32, RATE).ok_or(Error::Load))
            .transpose()?;
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = Arc::new(AtomicBool::new(false));
        let worker = {
            let stop = stop.clone();
            let cancel = cancel.clone();
            std::thread::Builder::new()
                .name("dictation".into())
                .spawn(move || {
                    let mut run = Run {
                        recognizer,
                        vad,
                        resampler,
                        history: Vec::new(),
                        seen: 0,
                        delivered: 0,
                        phrases: Vec::new(),
                        speaking: false,
                    };
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

struct Run {
    recognizer: Arc<OfflineRecognizer>,
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
                self.drain(on_event);
                return text::join(&self.phrases);
            }
            match mic.chunks.recv_timeout(Duration::from_millis(40)) {
                Ok(chunk) => self.feed(&chunk, false),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
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
            self.drain(on_event);
        }
    }

    fn feed(&mut self, chunk: &[f32], flush: bool) {
        let samples = match &self.resampler {
            Some(resampler) => resampler.resample(chunk, flush),
            None => chunk.to_vec(),
        };
        if samples.is_empty() {
            return;
        }
        self.seen += samples.len();
        self.history.extend_from_slice(&samples);
        if self.history.len() > MAX_HISTORY {
            // The oldest audio goes; offsets below stay in heard-sample units.
            let excess = self.history.len() - MAX_HISTORY;
            self.history.drain(..excess);
        }
        self.vad.accept_waveform(&samples);
    }

    /// Transcribes every phrase the detector has closed.
    fn drain(&mut self, on_event: &impl Fn(Event)) {
        let mut changed = false;
        while let Some(segment) = self.vad.front() {
            let start = segment.start().max(0) as usize;
            let mut audio = self.pre_roll(start);
            audio.extend_from_slice(segment.samples());
            self.delivered = start + segment.samples().len();
            self.vad.pop();
            let heard = transcribe(&self.recognizer, &audio);
            if !heard.trim().is_empty() {
                self.phrases.push(heard);
                changed = true;
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

fn transcribe(recognizer: &OfflineRecognizer, samples: &[f32]) -> String {
    if samples.is_empty() {
        return String::new();
    }
    let stream = recognizer.create_stream();
    stream.accept_waveform(RATE, samples);
    recognizer.decode(&stream);
    stream.get_result().map(|result| result.text).unwrap_or_default()
}

fn silero(model: String) -> VadModelConfig {
    VadModelConfig {
        silero_vad: SileroVadModelConfig {
            model: Some(model),
            threshold: 0.4,
            min_silence_duration: 0.45,
            min_speech_duration: 0.15,
            window_size: 512,
            max_speech_duration: 20.0,
        },
        sample_rate: RATE,
        num_threads: 1,
        provider: Some("cpu".into()),
        ..Default::default()
    }
}
