//! Interruptible local playback for the synthesizer's mono PCM WAV.
use std::{
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
            if chunk.len() % 2 != 0 {
                return Err("Incomplete voice samples".into());
            }
            samples = Some(
                chunk
                    .chunks_exact(2)
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

/// Plays one synthesized sentence on a background thread. Set `stop` to stop
/// promptly; the caller owns the flag so an interrupt can also cancel queued
/// work before playback begins.
pub fn play(wav: &[u8], stop: Arc<AtomicBool>) -> Result<(), String> {
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    let audio = decode(wav)?;
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("No audio output device found")?;
    let supported = device
        .default_output_config()
        .map_err(|error| format!("Could not read the audio output format: {error}"))?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    if config.channels == 0 || !(8_000..=192_000).contains(&config.sample_rate) {
        return Err("Unsupported audio output format".into());
    }
    let done = Arc::new(AtomicBool::new(false));
    let error = Arc::new(Mutex::new(None));
    let timeout = Duration::from_secs_f64(audio.samples.len() as f64 / f64::from(audio.rate))
        .saturating_add(Duration::from_secs(5));
    let samples = Arc::new(audio.samples);
    let step = f64::from(audio.rate) / f64::from(config.sample_rate);
    let stream = match format {
        cpal::SampleFormat::F32 => {
            output::<f32>(&device, &config, samples, step, &stop, &done, &error)
        }
        cpal::SampleFormat::F64 => {
            output::<f64>(&device, &config, samples, step, &stop, &done, &error)
        }
        cpal::SampleFormat::I16 => {
            output::<i16>(&device, &config, samples, step, &stop, &done, &error)
        }
        cpal::SampleFormat::I32 => {
            output::<i32>(&device, &config, samples, step, &stop, &done, &error)
        }
        cpal::SampleFormat::U16 => {
            output::<u16>(&device, &config, samples, step, &stop, &done, &error)
        }
        cpal::SampleFormat::I8 => {
            output::<i8>(&device, &config, samples, step, &stop, &done, &error)
        }
        cpal::SampleFormat::U8 => {
            output::<u8>(&device, &config, samples, step, &stop, &done, &error)
        }
        other => return Err(format!("Unsupported audio sample format {other}")),
    }?;
    stream
        .play()
        .map_err(|error| format!("Could not play voice audio: {error}"))?;
    let started = Instant::now();
    let mut finished_at = None;
    while !stop.load(Ordering::Acquire) {
        if error.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
            break;
        }
        if done.load(Ordering::Acquire) {
            let since = finished_at.get_or_insert_with(Instant::now);
            if since.elapsed() >= Duration::from_millis(100) {
                break;
            }
        }
        if started.elapsed() > timeout {
            return Err("Audio output did not finish in time".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(stream);
    let result = error.lock().unwrap_or_else(|e| e.into_inner()).take();
    match result {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn output<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples: Arc<Vec<f32>>,
    step: f64,
    stop: &Arc<AtomicBool>,
    done: &Arc<AtomicBool>,
    error: &Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = config.channels as usize;
    let stop = stop.clone();
    let done = done.clone();
    let error = error.clone();
    let mut position = 0.0;
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                for frame in data.chunks_mut(channels) {
                    let sample = if stop.load(Ordering::Acquire) {
                        0.0
                    } else if let Some(sample) = next_sample(&samples, &mut position, step) {
                        sample
                    } else {
                        done.store(true, Ordering::Release);
                        0.0
                    };
                    for value in frame {
                        *value = T::from_sample_(sample);
                    }
                }
            },
            move |reason| {
                *error.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some(format!("Audio output failed: {reason}"));
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
    fn a_cancelled_playback_never_opens_the_device() {
        let stop = Arc::new(AtomicBool::new(true));
        assert!(play(b"not a wav", stop).is_ok());
    }
}
