//! Local dictation for MonoBOT's composer: the microphone feeds
//! a Silero voice detector, and each phrase it closes is transcribed by
//! Parakeet v3 while the user keeps talking, so the text is ready the moment
//! they stop. Everything runs on this machine; nothing is sent anywhere.
mod capture;
pub mod model;
mod session;
mod text;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub use session::{Event, Session};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("Could not download {0}")]
    Download(String),
    #[error("{0} did not match its checksum; download it again")]
    Integrity(String),
    #[error("Invalid model path: {0}")]
    Path(PathBuf),
    #[error("The speech model is not installed")]
    NotInstalled,
    #[error("Could not load the speech model")]
    Load,
    #[error("Microphone: {0}")]
    Microphone(String),
}

/// The recognizer, loaded on the first dictation and kept while the user keeps
/// dictating: loading Parakeet takes seconds, transcribing a phrase a fraction
/// of one. It holds over half a gigabyte, so the app releases it once idle.
pub struct Dictation {
    dir: PathBuf,
    recognizer: Mutex<Option<Arc<sherpa_onnx::OfflineRecognizer>>>,
}

impl Dictation {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            recognizer: Mutex::new(None),
        }
    }

    pub fn installed(&self) -> bool {
        model::installed(&self.dir)
    }

    pub fn install(&self, progress: impl FnMut(u64, u64)) -> Result<(), Error> {
        model::install(&self.dir, progress)
    }

    /// Loads the recognizer ahead of the first dictation.
    pub fn prepare(&self) -> Result<Arc<sherpa_onnx::OfflineRecognizer>, Error> {
        let mut slot = self.recognizer.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(recognizer) = slot.as_ref() {
            return Ok(recognizer.clone());
        }
        if !self.installed() {
            return Err(Error::NotInstalled);
        }
        let recognizer = Arc::new(load(&self.dir)?);
        *slot = Some(recognizer.clone());
        Ok(recognizer)
    }

    /// Frees the recognizer's memory unless a session still uses it. Returns
    /// whether it was released; the next dictation loads it again.
    pub fn release(&self) -> bool {
        let mut slot = self.recognizer.lock().unwrap_or_else(|e| e.into_inner());
        match slot.as_ref() {
            Some(recognizer) if Arc::strong_count(recognizer) == 1 => {
                *slot = None;
                true
            }
            _ => false,
        }
    }

    /// Opens the microphone and starts transcribing.
    pub fn start(&self, on_event: impl Fn(Event) + Send + 'static) -> Result<Session, Error> {
        let recognizer = self.prepare()?;
        Session::start(&self.dir, recognizer, on_event)
    }
}

fn load(dir: &std::path::Path) -> Result<sherpa_onnx::OfflineRecognizer, Error> {
    use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};
    let mut config = OfflineRecognizerConfig::default();
    config.model_config.transducer = OfflineTransducerModelConfig {
        encoder: Some(model::path(dir, "encoder.int8.onnx")?),
        decoder: Some(model::path(dir, "decoder.int8.onnx")?),
        joiner: Some(model::path(dir, "joiner.int8.onnx")?),
    };
    config.model_config.tokens = Some(model::path(dir, "tokens.txt")?);
    config.model_config.model_type = Some("nemo_transducer".into());
    config.model_config.provider = Some("cpu".into());
    config.model_config.num_threads = std::thread::available_parallelism()
        .map_or(2, |n| n.get())
        .clamp(1, 8) as i32;
    config.decoding_method = Some("greedy_search".into());
    OfflineRecognizer::create(&config).ok_or(Error::Load)
}
