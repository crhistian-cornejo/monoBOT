//! Parakeet v3 on Apple's Neural Engine through Core ML, without Swift: the
//! four models FluidInference converted (`model::NEURAL_FILES`) and the TDT
//! greedy decoding FluidAudio runs over them (`TdtDecoderV3`), which IVY's
//! dictation uses. Ten seconds of speech take about 70 ms instead of the
//! CPU build's 750 ms, the models load in a tenth of a second once Core ML
//! has compiled them for this Mac, and the process keeps some 70 MB.
//!
//! The preprocessor takes a fixed window of 15 s, so a longer phrase is
//! transcribed window by window. Every call runs inside its own autorelease
//! pool on the caller's thread (never the interface's), so nothing Core ML
//! hands back outlives the phrase.
use std::path::Path;
use std::ptr::NonNull;
use std::sync::Mutex;

use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::AnyThread;
use objc2_core_ml::{
    MLComputeUnits, MLDictionaryFeatureProvider, MLFeatureProvider, MLFeatureValue, MLModel,
    MLModelConfiguration, MLMultiArray, MLMultiArrayDataType,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSURL};

use crate::Error;

/// The preprocessor's fixed input: 15 s at 16 kHz.
pub const WINDOW: usize = 240_000;
/// The token that means "nothing new in this frame"; it also primes the
/// decoder, as NeMo does. One past the last of the 8192 pieces.
const BLANK: i32 = 8192;
/// How many tokens one encoder frame may emit before decoding moves on.
const MAX_SYMBOLS_PER_STEP: usize = 10;
/// A runaway decode stops here (FluidAudio's `maxTokensPerChunk`).
const MAX_TOKENS_PER_WINDOW: usize = 150;
/// The joint's duration bins: how many frames each decision skips.
const DURATIONS: [usize; 5] = [0, 1, 2, 3, 4];
const ENCODER_HIDDEN: usize = 1024;
const DECODER_HIDDEN: usize = 640;
const DECODER_LAYERS: usize = 2;

/// The four models, loaded once and used one phrase at a time.
pub struct Parakeet {
    models: Mutex<Models>,
    vocabulary: Vec<String>,
}

struct Models {
    preprocessor: Retained<MLModel>,
    encoder: Retained<MLModel>,
    decoder: Retained<MLModel>,
    joint: Retained<MLModel>,
}

// SAFETY: Core ML documents `MLModel` predictions as safe from any thread;
// here they are also serialized by the mutex, and nothing else is shared.
unsafe impl Send for Models {}

impl Parakeet {
    /// Loads the models in `dir` (the Neural folder). The first load on a Mac
    /// compiles them for its Neural Engine, which takes a few seconds; later
    /// loads reuse what the system cached.
    pub fn load(dir: &Path) -> Result<Self, Error> {
        let vocabulary = vocabulary(dir)?;
        let models = autoreleasepool(|_| -> Result<Models, Error> {
            Ok(Models {
                // The mel front end is small and exact on the CPU, as
                // FluidAudio runs it.
                preprocessor: model(dir, "Preprocessor", MLComputeUnits::CPUOnly)?,
                encoder: model(dir, "Encoder", MLComputeUnits::CPUAndNeuralEngine)?,
                decoder: model(dir, "Decoder", MLComputeUnits::CPUAndNeuralEngine)?,
                joint: model(dir, "JointDecisionv3", MLComputeUnits::CPUAndNeuralEngine)?,
            })
        })?;
        Ok(Self {
            models: Mutex::new(models),
            vocabulary,
        })
    }

    /// What was said in `samples` (mono, 16 kHz).
    pub fn transcribe(&self, samples: &[f32]) -> Result<String, String> {
        let models = self.models.lock().unwrap_or_else(|e| e.into_inner());
        let mut words = Vec::new();
        for window in samples.chunks(WINDOW) {
            let tokens = autoreleasepool(|_| window_tokens(&models, window))?;
            let text = detokenize(&tokens, &self.vocabulary);
            if !text.is_empty() {
                words.push(text);
            }
        }
        Ok(words.join(" "))
    }
}

/// The pieces' text by id, from `parakeet_v3_vocab.json` (`{"0": "<unk>", …}`).
fn vocabulary(dir: &Path) -> Result<Vec<String>, Error> {
    let bytes = std::fs::read(dir.join("parakeet_v3_vocab.json"))?;
    let map: std::collections::HashMap<String, String> =
        serde_json::from_slice(&bytes).map_err(|_| Error::Load)?;
    let mut vocabulary = vec![String::new(); map.len()];
    for (id, piece) in map {
        let id: usize = id.parse().map_err(|_| Error::Load)?;
        *vocabulary.get_mut(id).ok_or(Error::Load)? = piece;
    }
    Ok(vocabulary)
}

fn model(dir: &Path, name: &str, units: MLComputeUnits) -> Result<Retained<MLModel>, Error> {
    let path = dir.join(format!("{name}.mlmodelc"));
    let url = NSURL::from_directory_path(&path).ok_or(Error::Path(path))?;
    unsafe {
        let config = MLModelConfiguration::new();
        config.setComputeUnits(units);
        MLModel::modelWithContentsOfURL_configuration_error(&url, &config).map_err(|_| Error::Load)
    }
}

/// Greedy TDT decoding of one window, as FluidAudio's `TdtDecoderV3` does it
/// for a single chunk: the joint picks a token and how many frames to skip; a
/// blank always moves on at least one frame, and one frame emits at most ten
/// tokens.
fn window_tokens(models: &Models, samples: &[f32]) -> Result<Vec<i32>, String> {
    let audio = array(&[1, WINDOW], MLMultiArrayDataType::Float32)?;
    fill_f32(&audio, samples);
    let length = int_array(&[1], samples.len() as i32)?;
    let mel = predict(
        &models.preprocessor,
        &[("audio_signal", &audio), ("audio_length", &length)],
    )?;
    let encoded = predict(
        &models.encoder,
        &[
            ("mel", &*output(&mel, "mel")?),
            ("mel_length", &*output(&mel, "mel_length")?),
        ],
    )?;
    let frames = Frames::read(&*output(&encoded, "encoder")?)?;
    let valid = (read_i32(&*output(&encoded, "encoder_length")?, 0) as usize).min(frames.count);
    if valid <= 1 {
        return Ok(Vec::new());
    }

    let mut decoder = Decoder::new()?;
    decoder.step(&models.decoder, BLANK)?;
    let step = array(&[1, ENCODER_HIDDEN, 1], MLMultiArrayDataType::Float32)?;
    let mut tokens = Vec::new();
    let mut time = 0usize;
    let mut last_emission: Option<usize> = None;
    let mut emissions_here = 0usize;
    while time < valid {
        frames.copy_into(time, &step);
        let decision = predict(
            &models.joint,
            &[
                ("encoder_step", &step),
                ("decoder_step", &*decoder.projection()?),
            ],
        )?;
        let token = read_i32(&*output(&decision, "token_id")?, 0);
        let bin = read_i32(&*output(&decision, "duration")?, 0);
        let mut skip = *DURATIONS
            .get(usize::try_from(bin).map_err(|_| "duration out of range")?)
            .ok_or("duration out of range")?;
        let at = time;
        if token == BLANK {
            time += skip.max(1);
            continue;
        }
        if skip == 0 && last_emission == Some(at) && emissions_here >= 1 {
            skip = 1;
        }
        tokens.push(token);
        if tokens.len() > MAX_TOKENS_PER_WINDOW {
            break;
        }
        decoder.step(&models.decoder, token)?;
        if last_emission == Some(at) {
            emissions_here += 1;
        } else {
            last_emission = Some(at);
            emissions_here = 1;
        }
        time += skip;
        if emissions_here >= MAX_SYMBOLS_PER_STEP {
            time += 1;
            emissions_here = 0;
            last_emission = None;
        }
    }
    Ok(tokens)
}

/// The prediction network: its LSTM state and its last projection.
struct Decoder {
    hidden: Retained<MLMultiArray>,
    cell: Retained<MLMultiArray>,
    projection: Option<Retained<MLMultiArray>>,
    target: Retained<MLMultiArray>,
    target_length: Retained<MLMultiArray>,
}

impl Decoder {
    fn new() -> Result<Self, String> {
        let state = || -> Result<Retained<MLMultiArray>, String> {
            let state = array(
                &[DECODER_LAYERS, 1, DECODER_HIDDEN],
                MLMultiArrayDataType::Float32,
            )?;
            fill_f32(&state, &[]);
            Ok(state)
        };
        Ok(Self {
            hidden: state()?,
            cell: state()?,
            projection: None,
            target: int_array(&[1, 1], BLANK)?,
            target_length: int_array(&[1], 1)?,
        })
    }

    /// Feeds `token` and keeps the new state and projection.
    fn step(&mut self, model: &MLModel, token: i32) -> Result<(), String> {
        write_i32(&self.target, 0, token);
        let out = predict(
            model,
            &[
                ("targets", &self.target),
                ("target_length", &self.target_length),
                ("h_in", &self.hidden),
                ("c_in", &self.cell),
            ],
        )?;
        self.hidden = output(&out, "h_out")?;
        self.cell = output(&out, "c_out")?;
        self.projection = Some(output(&out, "decoder")?);
        Ok(())
    }

    fn projection(&self) -> Result<Retained<MLMultiArray>, String> {
        self.projection
            .clone()
            .ok_or_else(|| "decoder not primed".into())
    }
}

/// The encoder's frames, copied out once as `[frame][hidden]` so each step is
/// a straight copy.
struct Frames {
    values: Vec<f32>,
    count: usize,
}

impl Frames {
    fn read(encoder: &MLMultiArray) -> Result<Self, String> {
        let shape = dims(unsafe { &encoder.shape() });
        let strides = dims(unsafe { &encoder.strides() });
        if shape.len() != 3 || shape[1] != ENCODER_HIDDEN {
            return Err(format!("unexpected encoder shape {shape:?}"));
        }
        let count = shape[2];
        let mut values = vec![0.0; count * ENCODER_HIDDEN];
        let reader = Reader::of(encoder)?;
        for frame in 0..count {
            for hidden in 0..ENCODER_HIDDEN {
                values[frame * ENCODER_HIDDEN + hidden] =
                    reader.f32(hidden * strides[1] + frame * strides[2]);
            }
        }
        Ok(Self { values, count })
    }

    fn copy_into(&self, frame: usize, step: &MLMultiArray) {
        let from = &self.values[frame * ENCODER_HIDDEN..(frame + 1) * ENCODER_HIDDEN];
        let stride = dims(unsafe { &step.strides() })[1];
        let to = data(step).cast::<f32>();
        for (hidden, value) in from.iter().enumerate() {
            // SAFETY: `step` is a [1, ENCODER_HIDDEN, 1] Float32 array this
            // module created, and `hidden * stride` stays inside it.
            unsafe { to.add(hidden * stride).write(*value) };
        }
    }
}

/// Reads a Core ML array's values as f32, whichever float type it holds.
struct Reader {
    at: NonNull<std::ffi::c_void>,
    half: bool,
}

impl Reader {
    fn of(array: &MLMultiArray) -> Result<Self, String> {
        let kind = unsafe { array.dataType() };
        let half = if kind == MLMultiArrayDataType::Float32 {
            false
        } else if kind == MLMultiArrayDataType::Float16 {
            true
        } else {
            return Err(format!("unexpected array type {}", kind.0));
        };
        Ok(Self {
            at: NonNull::new(data(array)).ok_or("empty array")?,
            half,
        })
    }

    fn f32(&self, index: usize) -> f32 {
        // SAFETY: callers index within the array's shape and strides.
        unsafe {
            if self.half {
                f16_to_f32(self.at.cast::<u16>().add(index).read())
            } else {
                self.at.cast::<f32>().add(index).read()
            }
        }
    }
}

fn f16_to_f32(bits: u16) -> f32 {
    let sign = u32::from(bits >> 15) << 31;
    let exponent = u32::from((bits >> 10) & 0x1f);
    let mantissa = u32::from(bits & 0x3ff);
    let value = match exponent {
        0 if mantissa == 0 => sign,
        0 => {
            // Subnormal: normalize it.
            let shift = mantissa.leading_zeros() - 21;
            sign | ((113 - shift) << 23) | (((mantissa << shift) & 0x3ff) << 13)
        }
        0x1f => sign | 0x7f80_0000 | (mantissa << 13),
        _ => sign | ((exponent + 112) << 23) | (mantissa << 13),
    };
    f32::from_bits(value)
}

/// Joins the pieces, `▁` marking where a word starts; control tokens
/// (`<unk>`, `<|endoftext|>`…) are dropped.
fn detokenize(tokens: &[i32], vocabulary: &[String]) -> String {
    let mut text = String::new();
    for token in tokens {
        let Some(piece) = usize::try_from(*token)
            .ok()
            .and_then(|id| vocabulary.get(id))
        else {
            continue;
        };
        if piece.starts_with('<') && piece.ends_with('>') {
            continue;
        }
        text.push_str(&piece.replace('\u{2581}', " "));
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn dims(numbers: &NSArray<NSNumber>) -> Vec<usize> {
    numbers
        .iter()
        .map(|number| number.integerValue().max(0) as usize)
        .collect()
}

#[allow(deprecated)]
fn data(array: &MLMultiArray) -> *mut std::ffi::c_void {
    // `dataPointer` is what FluidAudio uses too: the arrays here live in
    // memory, never in a pixel buffer.
    unsafe { array.dataPointer().as_ptr() }
}

fn array(shape: &[usize], kind: MLMultiArrayDataType) -> Result<Retained<MLMultiArray>, String> {
    let shape: Vec<Retained<NSNumber>> = shape
        .iter()
        .map(|dim| NSNumber::new_isize(*dim as isize))
        .collect();
    let shape = NSArray::from_retained_slice(&shape);
    unsafe { MLMultiArray::initWithShape_dataType_error(MLMultiArray::alloc(), &shape, kind) }
        .map_err(|error| error.localizedDescription().to_string())
}

fn int_array(shape: &[usize], value: i32) -> Result<Retained<MLMultiArray>, String> {
    let array = array(shape, MLMultiArrayDataType::Int32)?;
    let count = unsafe { array.count() }.max(0) as usize;
    for index in 0..count {
        write_i32(&array, index, value);
    }
    Ok(array)
}

/// Writes `samples` from the start of a contiguous Float32 array and zeros
/// the rest.
fn fill_f32(array: &MLMultiArray, samples: &[f32]) {
    let count = unsafe { array.count() }.max(0) as usize;
    let to = data(array).cast::<f32>();
    // SAFETY: arrays this module creates are contiguous Float32 of `count`.
    let values = unsafe { std::slice::from_raw_parts_mut(to, count) };
    let used = samples.len().min(count);
    values[..used].copy_from_slice(&samples[..used]);
    values[used..].fill(0.0);
}

fn write_i32(array: &MLMultiArray, index: usize, value: i32) {
    // SAFETY: only called on Int32 arrays this module created, in bounds.
    unsafe { data(array).cast::<i32>().add(index).write(value) };
}

fn read_i32(array: &MLMultiArray, index: usize) -> i32 {
    let kind = unsafe { array.dataType() };
    // SAFETY: the outputs read here are one-element arrays.
    unsafe {
        if kind == MLMultiArrayDataType::Int32 {
            data(array).cast::<i32>().add(index).read()
        } else {
            Reader::of(array).map_or(0, |reader| reader.f32(index) as i32)
        }
    }
}

fn predict(
    model: &MLModel,
    inputs: &[(&str, &MLMultiArray)],
) -> Result<Retained<ProtocolObject<dyn MLFeatureProvider>>, String> {
    let names: Vec<Retained<NSString>> = inputs
        .iter()
        .map(|(name, _)| NSString::from_str(name))
        .collect();
    let keys: Vec<&NSString> = names.iter().map(|name| &**name).collect();
    let values: Vec<Retained<AnyObject>> = inputs
        .iter()
        .map(|(_, array)| {
            let value = unsafe { MLFeatureValue::featureValueWithMultiArray(array) };
            Retained::into_super(Retained::into_super(value))
        })
        .collect();
    let dictionary = NSDictionary::from_retained_objects(&keys, &values);
    unsafe {
        let provider = MLDictionaryFeatureProvider::initWithDictionary_error(
            MLDictionaryFeatureProvider::alloc(),
            &dictionary,
        )
        .map_err(|error| error.localizedDescription().to_string())?;
        model
            .predictionFromFeatures_error(ProtocolObject::from_ref(&*provider))
            .map_err(|error| error.localizedDescription().to_string())
    }
}

fn output(
    features: &ProtocolObject<dyn MLFeatureProvider>,
    name: &str,
) -> Result<Retained<MLMultiArray>, String> {
    unsafe { features.featureValueForName(&NSString::from_str(name)) }
        .and_then(|value| unsafe { value.multiArrayValue() })
        .ok_or_else(|| format!("no {name} in the model's output"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_floats_read_as_their_values() {
        assert_eq!(f16_to_f32(0x0000), 0.0);
        assert_eq!(f16_to_f32(0x3c00), 1.0);
        assert_eq!(f16_to_f32(0xc000), -2.0);
        assert_eq!(f16_to_f32(0x3555), 0.333_251_95);
        assert_eq!(f16_to_f32(0x7bff), 65504.0);
        assert_eq!(f16_to_f32(0x0001), 5.960_464_5e-8);
        assert!(f16_to_f32(0x7c00).is_infinite());
    }

    #[test]
    fn pieces_join_into_words_without_control_tokens() {
        let vocabulary: Vec<String> = [
            "<unk>",
            "\u{2581}hola",
            "\u{2581}mun",
            "do",
            ",",
            "<|endoftext|>",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            detokenize(&[1, 2, 3, 4, 5, 0, 99], &vocabulary),
            "hola mundo,"
        );
        assert_eq!(detokenize(&[], &vocabulary), "");
    }

    /// Transcribes a recording with the Neural build in `MONOBOT_NEURAL_DIR`
    /// (a folder holding `NEURAL_FILES`) and a 16 kHz mono 16-bit WAV in
    /// `MONOBOT_DICTATION_WAV`; with neither, it decodes a second of silence
    /// with the app's own copy. `cargo test -p monobot-dictation -- --ignored
    /// --nocapture neural`.
    #[test]
    #[ignore]
    fn neural_transcribes_on_this_mac() {
        let dir = std::env::var_os("MONOBOT_NEURAL_DIR")
            .map(std::path::PathBuf::from)
            .expect("MONOBOT_NEURAL_DIR");
        let started = std::time::Instant::now();
        let parakeet = Parakeet::load(&dir).unwrap();
        eprintln!("loaded in {:?}", started.elapsed());
        assert_eq!(parakeet.transcribe(&vec![0.0; 16_000]).unwrap(), "");
        if let Some(path) = std::env::var_os("MONOBOT_DICTATION_WAV") {
            let samples = crate::wav16(&std::fs::read(path).unwrap());
            let started = std::time::Instant::now();
            let heard = parakeet.transcribe(&samples).unwrap();
            eprintln!(
                "heard {:.1}s in {:?}: {heard}",
                samples.len() as f32 / 16_000.0,
                started.elapsed()
            );
            assert!(!heard.is_empty());
        }
    }
}
