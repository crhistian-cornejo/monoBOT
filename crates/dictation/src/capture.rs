//! The default microphone, mixed to mono. The stream lives on its own thread
//! (it is not `Send` on every platform) until the session stops it.
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::Error;

/// Audio from the microphone: mono chunks at `sample_rate`, and how loud the
/// last chunk was (0 to 1) for the waveform.
pub struct Microphone {
    pub chunks: Receiver<Vec<f32>>,
    pub sample_rate: u32,
    pub level: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Microphone {
    pub fn open() -> Result<Self, Error> {
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, Error>>();
        let (tx, chunks) = mpsc::sync_channel::<Vec<f32>>(512);
        let level = Arc::new(AtomicU32::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let level = level.clone();
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("dictation-mic".into())
                .spawn(move || {
                    let stream = match build(tx, level) {
                        Ok((stream, rate)) => {
                            let _ = ready_tx.send(Ok(rate));
                            stream
                        }
                        Err(error) => {
                            let _ = ready_tx.send(Err(error));
                            return;
                        }
                    };
                    while !stop.load(Ordering::Acquire) {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    drop(stream);
                })?
        };
        let sample_rate = ready_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| Error::Microphone("the microphone did not start".into()))??;
        Ok(Self {
            chunks,
            sample_rate,
            level,
            stop,
            thread: Some(thread),
        })
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    /// Stops recording; chunks already captured stay readable.
    pub fn close(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Microphone {
    fn drop(&mut self) {
        self.close();
    }
}

fn build(tx: SyncSender<Vec<f32>>, level: Arc<AtomicU32>) -> Result<(cpal::Stream, u32), Error> {
    let device = cpal::default_host()
        .default_input_device()
        .ok_or_else(|| Error::Microphone("no microphone found".into()))?;
    let supported = device
        .default_input_config()
        .map_err(|error| Error::Microphone(format!("check microphone permission: {error}")))?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    if config.channels == 0 || config.sample_rate < 8_000 || config.sample_rate > 192_000 {
        return Err(Error::Microphone("unsupported microphone format".into()));
    }
    let rate = config.sample_rate;
    let stream = match format {
        cpal::SampleFormat::F32 => stream::<f32>(&device, &config, tx, level),
        cpal::SampleFormat::F64 => stream::<f64>(&device, &config, tx, level),
        cpal::SampleFormat::I16 => stream::<i16>(&device, &config, tx, level),
        cpal::SampleFormat::I32 => stream::<i32>(&device, &config, tx, level),
        cpal::SampleFormat::U16 => stream::<u16>(&device, &config, tx, level),
        cpal::SampleFormat::I8 => stream::<i8>(&device, &config, tx, level),
        cpal::SampleFormat::U8 => stream::<u8>(&device, &config, tx, level),
        other => return Err(Error::Microphone(format!("unsupported sample format {other}"))),
    }?;
    stream
        .play()
        .map_err(|error| Error::Microphone(format!("could not start the microphone: {error}")))?;
    Ok((stream, rate))
}

fn stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    tx: SyncSender<Vec<f32>>,
    level: Arc<AtomicU32>,
) -> Result<cpal::Stream, Error>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let channels = config.channels as usize;
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let mono: Vec<f32> = data
                    .chunks_exact(channels)
                    .map(|frame| {
                        frame
                            .iter()
                            .map(|sample| <f32 as cpal::FromSample<T>>::from_sample_(*sample))
                            .sum::<f32>()
                            / channels as f32
                    })
                    .collect();
                level.store(loudness(&mono).to_bits(), Ordering::Relaxed);
                // A full queue drops this chunk rather than block the audio thread.
                let _ = tx.try_send(mono);
            },
            |_| {},
            None,
        )
        .map_err(|error| Error::Microphone(format!("could not open the microphone: {error}")))
}

/// RMS on a decibel scale, mapped so quiet rooms read near 0 and speech
/// fills most of the waveform's height.
pub fn loudness(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
    let db = 20.0 * rms.max(1e-6).log10();
    ((db + 60.0) / 50.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::loudness;

    #[test]
    fn silence_is_low_and_speech_is_high() {
        assert_eq!(loudness(&[]), 0.0);
        assert!(loudness(&[0.0; 512]) < 0.05);
        assert!(loudness(&[0.3; 512]) > 0.7);
    }
}
