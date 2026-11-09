//! Smart Turn v3.2 (pipecat-ai): whether the user has finished what they
//! were saying, from the audio itself, as JOSIE-Voice's and IVY's turn
//! detector does (`IVY/crates/voice/src/smart_turn.rs`, ported as is). The
//! last 8 seconds of the turn go in as Whisper's log-mel spectrogram (80
//! bands × 800 frames) and the model answers the probability that the turn
//! is complete. It is the unquantised checkpoint, which tract runs exactly as
//! onnxruntime does; pure Rust (tract and rustfft), nothing native added.

use std::sync::Arc;

use rustfft::{num_complex::Complex, Fft, FftPlanner};
use tract_onnx::prelude::*;

/// The sample rate the model takes.
pub const SAMPLE_RATE: usize = 16_000;
/// The model looks at this many seconds of audio: the last ones of the turn (what came before is zeros).
const SAMPLES: usize = 8 * SAMPLE_RATE;
const BANDS: usize = 80;
const FRAMES: usize = 800;
const FFT_SIZE: usize = 400;
const HOP: usize = 160;
const BINS: usize = FFT_SIZE / 2 + 1;
const MAX_HZ: f32 = 8_000.;

/// Smart Turn loaded: its graph ready to run, and the features it is fed.
pub struct Model {
    plan: Arc<TypedRunnableModel>,
    features: Features,
}

/// The log-mel features of the model's input: the filter bank, the window and the FFT, built once.
pub struct Features {
    /// `BINS × BANDS`, by frequency bin: the Slaney-normalised triangles of the mel bands.
    filters: Vec<f64>,
    window: Vec<f64>,
    fft: Arc<dyn Fft<f64>>,
}

fn problem(error: impl std::fmt::Display) -> String {
    format!("Smart Turn: {error}")
}

impl Model {
    /// Loads the model at `path` (already checked by `pack::checked`).
    pub fn load(path: &str) -> Result<Model, String> {
        let plan = tract_onnx::onnx()
            .model_for_path(path)
            .map_err(problem)?
            .into_optimized()
            .map_err(problem)?
            .into_runnable()
            .map_err(problem)?;
        Ok(Model {
            plan,
            features: Features::new(),
        })
    }

    /// The probability that the turn ending `audio` is complete (0 to 1); the turn is complete at or above
    /// `turn::COMPLETE_AT`.
    pub fn complete(&self, audio: &[f32]) -> Result<f32, String> {
        let input: TValue = Tensor::from_shape(&[1, BANDS, FRAMES], &self.features.extract(audio))
            .map_err(problem)?
            .into();
        let output = self.plan.run(tvec!(input)).map_err(problem)?;
        let probability: &Tensor = &output[0];
        probability
            .to_plain_array_view::<f32>()
            .map_err(problem)?
            .iter()
            .next()
            .copied()
            .ok_or_else(|| problem("no output"))
    }
}

impl Default for Features {
    fn default() -> Self {
        Self::new()
    }
}

impl Features {
    pub fn new() -> Features {
        // (In f64 as the reference computes: the quiet bands of a loud tone sit near f32's precision.)
        let window = (0..FFT_SIZE)
            .map(|n| 0.5 - 0.5 * (2. * std::f64::consts::PI * n as f64 / FFT_SIZE as f64).cos())
            .collect();
        Features {
            filters: filter_bank(),
            window,
            fft: FftPlanner::<f64>::new().plan_fft_forward(FFT_SIZE),
        }
    }

    /// The log-mel features of the last 8 seconds of `audio`, as `[band][frame]` (the model's input, row by row).
    pub fn extract(&self, audio: &[f32]) -> Vec<f32> {
        let mut wave = vec![0f32; SAMPLES];
        let taken = audio.len().min(SAMPLES);
        wave[SAMPLES - taken..].copy_from_slice(&audio[audio.len() - taken..]);
        let mean = wave.iter().sum::<f32>() / SAMPLES as f32;
        let variance = wave.iter().map(|s| (s - mean) * (s - mean)).sum::<f32>() / SAMPLES as f32;
        let scale = 1. / (variance + 1e-7).sqrt();
        for sample in &mut wave {
            *sample = (*sample - mean) * scale;
        }
        // (Frame `t` starts `FFT_SIZE / 2` before sample `t × HOP`: the STFT is centred, its edges reflected.)
        let edge = FFT_SIZE as isize / 2;
        let reflected = |index: isize| -> f32 {
            let last = SAMPLES as isize - 1;
            let index = if index < 0 {
                -index
            } else if index > last {
                2 * last - index
            } else {
                index
            };
            wave[index as usize]
        };
        let mut mel = vec![0f64; BANDS * FRAMES];
        let mut buffer = vec![Complex::new(0f64, 0.); FFT_SIZE];
        let mut power = [0f64; BINS];
        for frame in 0..FRAMES {
            let start = (frame * HOP) as isize - edge;
            for (k, slot) in buffer.iter_mut().enumerate() {
                *slot = Complex::new(reflected(start + k as isize) as f64 * self.window[k], 0.);
            }
            self.fft.process(&mut buffer);
            for (bin, value) in power.iter_mut().enumerate() {
                *value = buffer[bin].norm_sqr();
            }
            for band in 0..BANDS {
                let energy: f64 = (0..BINS)
                    .map(|bin| self.filters[bin * BANDS + band] * power[bin])
                    .sum();
                mel[band * FRAMES + frame] = energy.max(1e-10).log10();
            }
        }
        let peak = mel.iter().copied().fold(f64::MIN, f64::max);
        mel.iter()
            .map(|value| ((value.max(peak - 8.) + 4.) / 4.) as f32)
            .collect()
    }
}

/// Hz to mel on Slaney's scale: linear below 1 kHz, logarithmic above (as the Whisper features use it).
fn hz_to_mel(hz: f64) -> f64 {
    let log_step = 6.4f64.ln() / 27.;
    if hz >= 1_000. {
        15. + (hz / 1_000.).ln() / log_step
    } else {
        hz * 3. / 200.
    }
}

fn mel_to_hz(mel: f64) -> f64 {
    let log_step = 6.4f64.ln() / 27.;
    if mel >= 15. {
        1_000. * ((mel - 15.) * log_step).exp()
    } else {
        mel * 200. / 3.
    }
}

/// The triangles of the 80 mel bands over the 201 bins of a 400-point FFT, scaled to equal area (Slaney's norm).
fn filter_bank() -> Vec<f64> {
    let (low, high) = (hz_to_mel(0.), hz_to_mel(MAX_HZ as f64));
    let edges: Vec<f64> = (0..BANDS + 2)
        .map(|i| mel_to_hz(low + (high - low) * i as f64 / (BANDS + 1) as f64))
        .collect();
    let mut filters = vec![0f64; BINS * BANDS];
    for band in 0..BANDS {
        let (left, center, right) = (edges[band], edges[band + 1], edges[band + 2]);
        let norm = 2. / (right - left);
        for bin in 0..BINS {
            let hz = MAX_HZ as f64 * bin as f64 / (BINS - 1) as f64;
            let slope = ((hz - left) / (center - left)).min((right - hz) / (right - center));
            filters[bin * BANDS + band] = slope.max(0.) * norm;
        }
    }
    filters
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sine of `hz` for `seconds` at the model's rate.
    fn tone(hz: f32, seconds: f32) -> Vec<f32> {
        (0..(seconds * SAMPLE_RATE as f32) as usize)
            .map(|n| {
                (2. * std::f64::consts::PI * hz as f64 * n as f64 / SAMPLE_RATE as f64).sin() as f32
            })
            .collect()
    }

    #[test]
    fn the_filter_bank_has_whole_triangles_of_equal_area() {
        let filters = filter_bank();
        assert_eq!(filters.len(), BINS * BANDS);
        let areas: Vec<f64> = (0..BANDS)
            .map(|band| {
                (0..BINS)
                    .map(|bin| filters[bin * BANDS + band])
                    .sum::<f64>()
            })
            .collect();
        for (band, _) in areas.iter().enumerate() {
            let column: Vec<f64> = (0..BINS).map(|bin| filters[bin * BANDS + band]).collect();
            let first = column
                .iter()
                .position(|&v| v > 0.)
                .unwrap_or_else(|| panic!("band {band} is empty"));
            let last = column.iter().rposition(|&v| v > 0.).unwrap();
            assert!(
                column[first..=last].iter().all(|&v| v > 0.),
                "band {band} has a hole"
            );
        }
        let (min, max) = areas
            .iter()
            .fold((f64::MAX, f64::MIN), |(lo, hi), &a| (lo.min(a), hi.max(a)));
        assert!(max / min < 1.6, "the bands are not alike: {min} to {max}");
    }

    #[test]
    fn the_features_are_the_reference_pipeline_on_the_same_tones() {
        // Values from the reference (`inference.py`'s pipeline in numpy, with the transformers' Whisper extractor) on
        // the same signals: a 440 Hz sine for 3 s; 0.2 × 220 Hz + 0.1 × 1000 Hz for 5 s; 0.25 × 300 Hz for 9 s.
        // Each is `[band][frame]`.
        let features = Features::new();
        let at = |values: &[f32], band: usize, frame: usize| values[band * FRAMES + frame];
        let close =
            |got: f32, want: f32| assert!((got - want).abs() < 1e-3, "got {got}, want {want}");

        let sine: Vec<f32> = tone(440., 3.).iter().map(|s| 0.3 * s).collect();
        let sine = features.extract(&sine);
        close(at(&sine, 0, 0), -0.229532);
        close(at(&sine, 40, 400), -0.229532);
        close(at(&sine, 79, 799), -0.229532);

        let mut mix: Vec<f32> = tone(220., 5.).iter().map(|s| 0.2 * s).collect();
        for (m, t) in mix.iter_mut().zip(tone(1_000., 5.)) {
            *m += 0.1 * t;
        }
        let mix = features.extract(&mix);
        close(at(&mix, 0, 0), -0.334641);
        close(at(&mix, 79, 799), -0.232517);

        let nine: Vec<f32> = tone(300., 9.).iter().map(|s| 0.25 * s).collect();
        let nine = features.extract(&nine);
        close(at(&nine, 0, 0), 1.294860);
        close(at(&nine, 40, 400), -0.361441);
    }

    /// The real model on silence and on speech: `MONOBOT_SMART_TURN=<path to
    /// smart-turn-v3.2.onnx> MONOBOT_DICTATION_WAV=<16 kHz wav> cargo test -p
    /// monobot-dictation --release -- --ignored --nocapture smart_turn`.
    #[test]
    #[ignore]
    fn the_model_answers_a_probability() {
        let path = std::env::var("MONOBOT_SMART_TURN").expect("MONOBOT_SMART_TURN");
        let started = std::time::Instant::now();
        let model = Model::load(&path).unwrap();
        eprintln!("loaded in {:?}", started.elapsed());
        let silent = model.complete(&vec![0.0; SAMPLE_RATE]).unwrap();
        assert!((0.0..=1.0).contains(&silent));
        if let Some(wav) = std::env::var_os("MONOBOT_DICTATION_WAV") {
            let samples = crate::wav16(&std::fs::read(wav).unwrap());
            let started = std::time::Instant::now();
            let complete = model.complete(&samples).unwrap();
            eprintln!(
                "complete {complete:.3} in {:?} (silence {silent:.3})",
                started.elapsed()
            );
            assert!((0.0..=1.0).contains(&complete));
        }
    }
}
