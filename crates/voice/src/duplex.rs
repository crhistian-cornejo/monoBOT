//! A live call's audio: the microphone goes up in 16 kHz frames while the
//! model's voice comes down and plays as it arrives. Without echo
//! cancellation the call is half duplex, as IVY's is: no microphone frame
//! goes up while the voice is audible on speakers or for the echo tail after
//! it. With headphones nothing is held back, so the user can talk over it.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::player;

/// The rate frames go up at.
pub const RATE: u32 = 16_000;

enum Play {
    Speech(Vec<f32>, u32),
    Flush,
}

/// When the voice stops being audible, in milliseconds since the gate was
/// made; 0 until anything plays.
#[derive(Clone)]
pub struct Gate {
    start: Instant,
    audible_until: Arc<AtomicU64>,
    in_ears: Arc<AtomicBool>,
}

impl Gate {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            audible_until: Arc::new(AtomicU64::new(0)),
            in_ears: Arc::new(AtomicBool::new(false)),
        }
    }

    fn now(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    /// `queued` more speech is waiting to play from now.
    pub fn playing(&self, queued: Duration) {
        let until = (self.now() + queued.as_millis() as u64).max(1);
        self.audible_until.fetch_max(until, Ordering::AcqRel);
    }

    /// The user talked over the voice: it stops now.
    pub fn silenced(&self) {
        self.audible_until
            .store(self.now().max(1), Ordering::Release);
    }

    pub fn set_in_ears(&self, in_ears: bool) {
        self.in_ears.store(in_ears, Ordering::Release);
    }

    /// Whether the microphone may go up now.
    pub fn open(&self) -> bool {
        if self.in_ears.load(Ordering::Acquire) {
            return true;
        }
        let until = self.audible_until.load(Ordering::Acquire);
        let tail = player::echo_tail(false).as_millis() as u64;
        until == 0 || self.now() >= until + tail
    }
}

impl Default for Gate {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Duplex {
    speech: Sender<Play>,
    gate: Gate,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl Duplex {
    /// Opens the microphone and the output; `frame` gets each 16 kHz frame
    /// the gate lets through, on the microphone's thread.
    pub fn start(frame: impl Fn(Vec<f32>) + Send + 'static) -> Result<Self, String> {
        let mic = monobot_dictation::Microphone::open().map_err(|error| error.to_string())?;
        let gate = Gate::new();
        let stop = Arc::new(AtomicBool::new(false));
        let (speech, playable) = mpsc::channel();
        let mut threads = Vec::new();
        {
            let (gate, stop) = (gate.clone(), stop.clone());
            threads.push(
                std::thread::Builder::new()
                    .name("live-voice".into())
                    .spawn(move || speak(playable, gate, stop))
                    .map_err(|error| error.to_string())?,
            );
        }
        {
            let (gate, stop) = (gate.clone(), stop.clone());
            threads.push(
                std::thread::Builder::new()
                    .name("live-mic".into())
                    .spawn(move || listen(mic, gate, stop, frame))
                    .map_err(|error| error.to_string())?,
            );
        }
        Ok(Self {
            speech,
            gate,
            stop,
            threads,
        })
    }

    /// Plays speech (mono, at `rate`) after what is queued.
    pub fn play(&self, samples: Vec<f32>, rate: u32) {
        let _ = self.speech.send(Play::Speech(samples, rate));
    }

    /// Drops what is still queued to play.
    pub fn flush(&self) {
        self.gate.silenced();
        let _ = self.speech.send(Play::Flush);
    }

    pub fn gate(&self) -> &Gate {
        &self.gate
    }
}

impl Drop for Duplex {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

fn speak(playable: Receiver<Play>, gate: Gate, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Acquire) {
        match playable.recv_timeout(Duration::from_millis(50)) {
            Ok(Play::Speech(samples, rate)) => {
                if let Ok(queued) = player::feed(&samples, rate) {
                    gate.set_in_ears(player::in_ears());
                    gate.playing(queued);
                }
            }
            Ok(Play::Flush) => player::flush(),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
    player::flush();
}

fn listen(
    mut mic: monobot_dictation::Microphone,
    gate: Gate,
    stop: Arc<AtomicBool>,
    frame: impl Fn(Vec<f32>),
) {
    let resampler = (mic.sample_rate != RATE)
        .then(|| sherpa_onnx::LinearResampler::create(mic.sample_rate as i32, RATE as i32));
    while !stop.load(Ordering::Acquire) {
        match mic.chunks.recv_timeout(Duration::from_millis(50)) {
            Ok(chunk) => {
                let samples = match resampler.as_ref() {
                    Some(Some(resampler)) => resampler.resample(&chunk, false),
                    _ => chunk,
                };
                if !samples.is_empty() && gate.open() {
                    frame(samples);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
    mic.close();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gate_holds_the_mic_while_speakers_play_and_for_the_tail() {
        let gate = Gate::new();
        assert!(gate.open());
        gate.playing(Duration::from_millis(100));
        assert!(!gate.open(), "the voice is playing");
        std::thread::sleep(Duration::from_millis(150));
        assert!(!gate.open(), "the room still rings");
        std::thread::sleep(player::echo_tail(false));
        assert!(gate.open());
    }

    #[test]
    fn headphones_never_hold_the_mic_and_an_interruption_opens_it_soon() {
        let gate = Gate::new();
        gate.set_in_ears(true);
        gate.playing(Duration::from_secs(5));
        assert!(gate.open());
        gate.set_in_ears(false);
        assert!(!gate.open());
        gate.silenced();
        std::thread::sleep(player::echo_tail(false) + Duration::from_millis(20));
        assert!(gate.open(), "only the tail after the cut");
    }
}
