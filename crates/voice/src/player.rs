//! Interruptible local playback for the synthesizer's mono PCM WAV, through
//! one output stream that stays open while a call speaks.
use std::{
    cell::RefCell,
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

struct Audio {
    rate: u32,
    samples: Vec<f32>,
}

fn decode(wav: &[u8]) -> Result<Audio, String> {
    if wav.len() < 12 || &wav[..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return Err("Invalid voice audio".into());
    }
    let mut at = 12usize;
    let mut rate = None;
    let mut samples = None;
    while at.checked_add(8).is_some_and(|end| end <= wav.len()) {
        let size = u32::from_le_bytes(wav[at + 4..at + 8].try_into().unwrap()) as usize;
        let start = at + 8;
        let end = start
            .checked_add(size)
            .filter(|end| *end <= wav.len())
            .ok_or("Incomplete voice audio")?;
        let chunk = &wav[start..end];
        if &wav[at..at + 4] == b"fmt " {
            if chunk.len() < 16
                || u16::from_le_bytes(chunk[0..2].try_into().unwrap()) != 1
                || u16::from_le_bytes(chunk[2..4].try_into().unwrap()) != 1
                || u16::from_le_bytes(chunk[14..16].try_into().unwrap()) != 16
            {
                return Err("Unsupported voice audio format".into());
            }
            let sample_rate = u32::from_le_bytes(chunk[4..8].try_into().unwrap());
            if !(8_000..=192_000).contains(&sample_rate) {
                return Err("Unsupported voice sample rate".into());
            }
            rate = Some(sample_rate);
        } else if &wav[at..at + 4] == b"data" {
            if !chunk.len().is_multiple_of(2) {
                return Err("Incomplete voice samples".into());
            }
            samples = Some(
                chunk
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|sample| i16::from_le_bytes([sample[0], sample[1]]) as f32 / 32768.0)
                    .collect::<Vec<_>>(),
            );
        }
        at = end.checked_add(size % 2).ok_or("Invalid voice audio")?;
    }
    let samples = samples
        .filter(|samples| !samples.is_empty())
        .ok_or("No voice samples")?;
    Ok(Audio {
        rate: rate.ok_or("No voice format")?,
        samples,
    })
}

/// Advances a mono signal at the output rate, interpolating between samples.
fn next_sample(samples: &[f32], position: &mut f64, step: f64) -> Option<f32> {
    let index = *position as usize;
    let first = *samples.get(index)?;
    let second = samples.get(index + 1).copied().unwrap_or(first);
    let fraction = (*position - index as f64) as f32;
    *position += step;
    Some(first + (second - first) * fraction)
}

/// After a round's last sentence, how long the room keeps ringing before the
/// microphone may listen again: speakers reverberate (IVY waits 400 ms too),
/// headphones do not.
pub fn echo_tail(in_ears: bool) -> Duration {
    Duration::from_millis(if in_ears { 120 } else { 400 })
}

/// Silence ahead of the first sentence on a freshly opened output, so a
/// Bluetooth link that wakes with the stream does not clip it.
const LEAD_IN: Duration = Duration::from_millis(150);
/// `play` returns with this much still queued, so the next sentence follows
/// without a gap.
const OVERLAP: Duration = Duration::from_millis(20);

/// One output stream, open for as long as the thread that plays keeps
/// playing: a call's player thread, from its first sentence to hang-up.
/// Reopening the device for every sentence flipped Bluetooth headsets
/// between profiles and clipped sentence starts.
struct Output {
    _stream: cpal::Stream,
    device: String,
    rate: u32,
    pending: Arc<Mutex<VecDeque<f32>>>,
    failure: Arc<Mutex<Option<String>>>,
    in_ears: bool,
}

thread_local! {
    static OUTPUT: RefCell<Option<Output>> = const { RefCell::new(None) };
}

fn default_device() -> Result<(cpal::Device, String), String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("No audio output device found")?;
    let id = device.id().map(|id| id.to_string()).unwrap_or_default();
    Ok((device, id))
}

impl Output {
    fn open() -> Result<Self, String> {
        let (device, id) = default_device()?;
        let supported = device
            .default_output_config()
            .map_err(|error| format!("Could not read the audio output format: {error}"))?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        if config.channels == 0 || !(8_000..=192_000).contains(&config.sample_rate) {
            return Err("Unsupported audio output format".into());
        }
        let pending = Arc::new(Mutex::new(VecDeque::new()));
        let failure = Arc::new(Mutex::new(None));
        let (p, f) = (&pending, &failure);
        let stream = match format {
            cpal::SampleFormat::F32 => output::<f32>(&device, &config, p, f),
            cpal::SampleFormat::F64 => output::<f64>(&device, &config, p, f),
            cpal::SampleFormat::I16 => output::<i16>(&device, &config, p, f),
            cpal::SampleFormat::I32 => output::<i32>(&device, &config, p, f),
            cpal::SampleFormat::U16 => output::<u16>(&device, &config, p, f),
            cpal::SampleFormat::I8 => output::<i8>(&device, &config, p, f),
            cpal::SampleFormat::U8 => output::<u8>(&device, &config, p, f),
            other => return Err(format!("Unsupported audio sample format {other}")),
        }?;
        stream
            .play()
            .map_err(|error| format!("Could not play voice audio: {error}"))?;
        let rate = config.sample_rate;
        pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend(std::iter::repeat_n(0.0, samples_in(LEAD_IN, rate)));
        Ok(Self {
            _stream: stream,
            device: id,
            rate,
            pending,
            failure,
            in_ears: monobot_dictation::devices::in_ears(),
        })
    }

    /// Still usable: nothing failed and the user did not switch outputs.
    fn current(&self) -> bool {
        self.failure
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none()
            && default_device().is_ok_and(|(_, id)| id == self.device)
    }

    fn queued(&self) -> usize {
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    fn failed(&self) -> Option<String> {
        self.failure
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn clear(&self) {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
}

fn samples_in(duration: Duration, rate: u32) -> usize {
    (duration.as_secs_f64() * f64::from(rate)) as usize
}

/// The thread's output, reopened when it failed or the default device changed.
fn with_output<T>(work: impl FnOnce(&Output) -> Result<T, String>) -> Result<T, String> {
    OUTPUT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if !slot.as_ref().is_some_and(Output::current) {
            *slot = None;
            *slot = Some(Output::open()?);
        }
        work(slot.as_ref().expect("opened above"))
    })
}

/// Plays one synthesized sentence on this thread's output and returns as it
/// ends. Set `stop` to cut it short; the caller owns the flag so an interrupt
/// can also cancel queued work before playback begins.
pub fn play(wav: &[u8], stop: Arc<AtomicBool>) -> Result<(), String> {
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    let audio = decode(wav)?;
    with_output(|output| {
        let step = f64::from(audio.rate) / f64::from(output.rate);
        let mut position = 0.0;
        let resampled: Vec<f32> =
            std::iter::from_fn(|| next_sample(&audio.samples, &mut position, step)).collect();
        let timeout = Duration::from_secs_f64(resampled.len() as f64 / f64::from(output.rate))
            .saturating_add(Duration::from_secs(5));
        output
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend(resampled);
        let left = samples_in(OVERLAP, output.rate);
        let started = Instant::now();
        loop {
            if stop.load(Ordering::Acquire) {
                output.clear();
                return Ok(());
            }
            if let Some(error) = output.failed() {
                output.clear();
                return Err(error);
            }
            if output.queued() <= left {
                return Ok(());
            }
            if started.elapsed() > timeout {
                output.clear();
                return Err("Audio output did not finish in time".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })
}

/// Queues raw speech (mono, at `rate`) on this thread's output without
/// waiting, for a live call that streams as it arrives. Returns how long is
/// queued now.
pub fn feed(samples: &[f32], rate: u32) -> Result<Duration, String> {
    with_output(|output| {
        let step = f64::from(rate) / f64::from(output.rate);
        let mut position = 0.0;
        let resampled = std::iter::from_fn(|| next_sample(samples, &mut position, step));
        let mut pending = output.pending.lock().unwrap_or_else(|e| e.into_inner());
        pending.extend(resampled);
        Ok(Duration::from_secs_f64(
            pending.len() as f64 / f64::from(output.rate),
        ))
    })
}

/// Drops whatever this thread's output still has queued: the user talked
/// over it.
pub fn flush() {
    OUTPUT.with(|slot| {
        if let Some(output) = slot.borrow().as_ref() {
            output.clear();
        }
    });
}

/// How long this thread's output still has queued.
pub fn queued() -> Duration {
    OUTPUT.with(|slot| {
        slot.borrow().as_ref().map_or(Duration::ZERO, |output| {
            Duration::from_secs_f64(output.queued() as f64 / f64::from(output.rate))
        })
    })
}

/// Whether this thread's output goes into the user's ears.
pub fn in_ears() -> bool {
    OUTPUT.with(|slot| slot.borrow().as_ref().is_some_and(|output| output.in_ears))
}

/// Waits for what is still playing on this thread's output, then for the
/// echo tail, so the microphone that opens next does not hear the voice.
pub fn settle(stop: &AtomicBool) {
    let in_ears = OUTPUT.with(|slot| {
        let slot = slot.borrow();
        let output = slot.as_ref()?;
        while output.queued() > 0 && output.failed().is_none() && !stop.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(10));
        }
        Some(output.in_ears)
    });
    let Some(in_ears) = in_ears else {
        return;
    };
    let until = Instant::now() + echo_tail(in_ears);
    while Instant::now() < until && !stop.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn output<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    pending: &Arc<Mutex<VecDeque<f32>>>,
    failure: &Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = config.channels as usize;
    let pending = pending.clone();
    let failure = failure.clone();
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                let mut pending = pending.lock().unwrap_or_else(|e| e.into_inner());
                for frame in data.chunks_mut(channels) {
                    // Silence between sentences keeps the device awake.
                    let sample = pending.pop_front().unwrap_or(0.0);
                    for value in frame {
                        *value = T::from_sample_(sample);
                    }
                }
            },
            move |reason| {
                if matches!(reason, cpal::StreamError::BufferUnderrun) {
                    return;
                }
                let mut failure = failure.lock().unwrap_or_else(|e| e.into_inner());
                failure.get_or_insert_with(|| format!("Audio output failed: {reason}"));
            },
            None,
        )
        .map_err(|error| format!("Could not open the audio output: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_synthesized_wav_and_rejects_bad_input() {
        let wav = crate::wav::encode(&[0.0, 0.5, -1.0], 24_000);
        let audio = decode(&wav).unwrap();
        assert_eq!(audio.rate, 24_000);
        assert_eq!(audio.samples.len(), 3);
        assert!((audio.samples[1] - 0.5).abs() < 0.001);
        assert!(decode(&wav[..wav.len() - 1]).is_err());
        assert!(decode(b"not a wav").is_err());
    }

    #[test]
    fn interpolates_across_output_rates() {
        let samples = [0.0, 0.5, 1.0];
        let mut position = 0.0;
        assert_eq!(next_sample(&samples, &mut position, 0.5), Some(0.0));
        assert_eq!(next_sample(&samples, &mut position, 0.5), Some(0.25));
        assert_eq!(next_sample(&samples, &mut position, 0.5), Some(0.5));
        position = 3.0;
        assert_eq!(next_sample(&samples, &mut position, 0.5), None);
    }

    #[test]
    fn speakers_wait_longer_for_the_room_than_headphones() {
        assert!(echo_tail(false) >= Duration::from_millis(400));
        assert!(echo_tail(true) < echo_tail(false));
    }

    #[test]
    fn settling_without_an_output_returns_at_once() {
        let started = Instant::now();
        settle(&AtomicBool::new(false));
        assert!(started.elapsed() < Duration::from_millis(50));
    }

    /// Plays two sentences back to back through this computer's output,
    /// one stream for both: `cargo test -p monobot-voice --features playback
    /// -- --ignored --nocapture plays_on_this_computer`.
    #[test]
    #[ignore]
    fn plays_on_this_computer() {
        let tone: Vec<f32> = (0..12_000)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 24_000.0).sin() * 0.2)
            .collect();
        let wav = crate::wav::encode(&tone, 24_000);
        let started = Instant::now();
        play(&wav, Arc::new(AtomicBool::new(false))).unwrap();
        play(&wav, Arc::new(AtomicBool::new(false))).unwrap();
        settle(&AtomicBool::new(false));
        eprintln!(
            "two half-second sentences and the tail in {:?}",
            started.elapsed()
        );
        assert!(started.elapsed() >= Duration::from_millis(1_000));
    }

    #[test]
    fn a_cancelled_playback_never_opens_the_device() {
        let stop = Arc::new(AtomicBool::new(true));
        assert!(play(b"not a wav", stop).is_ok());
    }
}
