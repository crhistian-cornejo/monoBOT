//! Local voices for MonoBOT's group calls: each member's reply is spoken by
//! Supertonic 3 through sherpa-onnx, in the language it was written in and in
//! a voice of its own. Like dictation, everything runs on this machine.
pub mod model;
#[cfg(feature = "playback")]
pub mod player;
#[cfg(feature = "playback")]
pub mod queue;
mod wav;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};

use sherpa_onnx::{
    GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsModelConfig,
    OfflineTtsSupertonicModelConfig,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Download(#[from] monobot_dictation::Error),
    #[error("Invalid model path: {0}")]
    Path(PathBuf),
    #[error("The voice download has no {0}; download it again")]
    Missing(&'static str),
    #[error("The voice model is not installed")]
    NotInstalled,
    #[error("Could not load the voice model")]
    Load,
    #[error("Calls speak Spanish and English, not {0:?}")]
    Language(String),
    #[error("Nothing to say, or too much at once")]
    Text,
    #[error("Could not speak that sentence")]
    Synthesis,
}

/// What the call asks for; Supertonic knows more, the call only needs these.
pub const LANGUAGES: [&str; 2] = ["en", "es"];
/// Calls speak a sentence at a time; anything longer is a mistake upstream.
const MAX_CHARS: usize = 2_000;
/// Diffusion steps per sentence: the model's examples use 8, and on a CPU it
/// still speaks many times faster than real time.
const STEPS: i32 = 8;

/// The synthesizer, loaded on the first sentence and kept while a call keeps
/// speaking, like the dictation recognizer; the app releases it once idle.
/// One sentence is spoken at a time.
pub struct Voice {
    dir: PathBuf,
    tts: Mutex<Option<Arc<OfflineTts>>>,
    turn: Mutex<()>,
    /// The model's voice count, kept after release so asking again does not
    /// load the whole synthesizer. Zero until known.
    speakers: AtomicI32,
}

impl Voice {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            tts: Mutex::new(None),
            turn: Mutex::new(()),
            speakers: AtomicI32::new(0),
        }
    }

    /// Frees the synthesizer's memory unless a sentence is being spoken.
    /// Returns whether it was released; the next sentence loads it again.
    pub fn release(&self) -> bool {
        let mut slot = self.tts.lock().unwrap_or_else(|e| e.into_inner());
        match slot.as_ref() {
            Some(tts) if Arc::strong_count(tts) == 1 => {
                *slot = None;
                true
            }
            _ => false,
        }
    }

    pub fn installed(&self) -> bool {
        model::installed(&self.dir)
    }

    pub fn install(&self, progress: impl FnMut(u64, u64)) -> Result<(), Error> {
        model::install(&self.dir, progress)
    }

    /// Loads the synthesizer ahead of the first sentence.
    pub fn prepare(&self) -> Result<Arc<OfflineTts>, Error> {
        let mut slot = self.tts.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tts) = slot.as_ref() {
            return Ok(tts.clone());
        }
        if !self.installed() {
            return Err(Error::NotInstalled);
        }
        let tts = Arc::new(load(&self.dir)?);
        *slot = Some(tts.clone());
        Ok(tts)
    }

    /// How many voices the model has; members are given one each.
    pub fn speakers(&self) -> Result<i32, Error> {
        let known = self.speakers.load(Ordering::Relaxed);
        if known > 0 {
            return Ok(known);
        }
        let count = self.prepare()?.num_speakers().max(1);
        self.speakers.store(count, Ordering::Relaxed);
        Ok(count)
    }

    /// Speaks one sentence and returns it as a WAV file.
    pub fn speak(&self, text: &str, lang: &str, speaker: i32) -> Result<Vec<u8>, Error> {
        let config = generation(text, lang, speaker)?;
        let tts = self.prepare()?;
        let config = GenerationConfig {
            sid: config.sid.rem_euclid(tts.num_speakers().max(1)),
            ..config
        };
        let _turn = self.turn.lock().unwrap_or_else(|e| e.into_inner());
        let audio = tts
            .generate_with_config(text.trim(), &config, None::<fn(&[f32], f32) -> bool>)
            .ok_or(Error::Synthesis)?;
        if audio.samples().is_empty() {
            return Err(Error::Synthesis);
        }
        Ok(wav::encode(
            audio.samples(),
            audio.sample_rate().max(1) as u32,
        ))
    }
}

/// Checks what the page asked for before the model is touched.
fn generation(text: &str, lang: &str, speaker: i32) -> Result<GenerationConfig, Error> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > MAX_CHARS || text.contains('\0') {
        return Err(Error::Text);
    }
    if !LANGUAGES.contains(&lang) {
        return Err(Error::Language(lang.into()));
    }
    Ok(GenerationConfig {
        sid: speaker.max(0),
        num_steps: STEPS,
        extra: Some(HashMap::from([(
            "lang".to_owned(),
            serde_json::Value::from(lang),
        )])),
        ..Default::default()
    })
}

fn load(dir: &std::path::Path) -> Result<OfflineTts, Error> {
    let file = |name| model::path(dir, name).map(Some);
    let config = OfflineTtsConfig {
        model: OfflineTtsModelConfig {
            supertonic: OfflineTtsSupertonicModelConfig {
                duration_predictor: file("duration_predictor.int8.onnx")?,
                text_encoder: file("text_encoder.int8.onnx")?,
                vector_estimator: file("vector_estimator.int8.onnx")?,
                vocoder: file("vocoder.int8.onnx")?,
                tts_json: file("tts.json")?,
                unicode_indexer: file("unicode_indexer.bin")?,
                voice_style: file("voice.bin")?,
            },
            num_threads: std::thread::available_parallelism()
                .map_or(2, |n| n.get())
                .clamp(1, 4) as i32,
            provider: Some("cpu".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    OfflineTts::create(&config).ok_or(Error::Load)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_for_a_language_and_voice_the_call_knows() {
        let config = generation(" Hola, ¿qué tal? ", "es", 3).unwrap();
        assert_eq!(config.sid, 3);
        assert_eq!(config.num_steps, STEPS);
        assert_eq!(config.extra.unwrap()["lang"], "es");
        assert!(matches!(generation("Hi", "fr", 0), Err(Error::Language(_))));
        assert!(matches!(generation("   ", "en", 0), Err(Error::Text)));
        assert!(matches!(
            generation(&"a".repeat(MAX_CHARS + 1), "en", 0),
            Err(Error::Text)
        ));
        assert_eq!(generation("Hi", "en", -4).unwrap().sid, 0);
    }

    #[test]
    fn speaking_without_the_model_says_so() {
        let voice = Voice::new(std::env::temp_dir().join("monobot-voice-absent"));
        assert!(!voice.installed());
        assert!(matches!(
            voice.speak("Hello.", "en", 0),
            Err(Error::NotInstalled)
        ));
    }

    /// Speaks with the real model: `MONOBOT_VOICE_DIR=<dir> cargo test -p
    /// monobot-voice -- --ignored`. With `MONOBOT_VOICE_ARCHIVE` naming an
    /// already downloaded archive, it is unpacked instead of fetched.
    #[test]
    #[ignore]
    fn speaks_spanish_and_english_with_the_installed_model() {
        let dir = PathBuf::from(std::env::var("MONOBOT_VOICE_DIR").expect("MONOBOT_VOICE_DIR"));
        let voice = Voice::new(dir.clone());
        if let Ok(archive) = std::env::var("MONOBOT_VOICE_ARCHIVE") {
            std::fs::create_dir_all(&dir).unwrap();
            let file = std::fs::File::open(archive).unwrap();
            model::unpack(std::io::BufReader::new(file), &dir).unwrap();
            std::fs::write(dir.join(model::MARKER), model::ARCHIVE.sha256).unwrap();
        }
        if !voice.installed() {
            voice.install(|_, _| {}).unwrap();
        }
        assert!(voice.speakers().unwrap() >= 2);
        for (text, lang) in [
            ("Hola, soy Ana. ¿Qué opinas del viaje?", "es"),
            ("Hi, I'm Leo.", "en"),
        ] {
            let started = std::time::Instant::now();
            let wav = voice.speak(text, lang, 1).unwrap();
            let rate = u32::from_le_bytes(wav[24..28].try_into().unwrap());
            let seconds = (wav.len() - 44) as f32 / 2.0 / rate as f32;
            eprintln!("{lang}: {seconds:.2}s of audio in {:?}", started.elapsed());
            assert!(seconds > 0.5, "{lang}: {seconds}");
        }
    }
}
