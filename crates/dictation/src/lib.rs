//! Local dictation for monoBOT's composer: the microphone feeds
//! a Silero voice detector, and each phrase it closes is transcribed by
//! Parakeet v3 while the user keeps talking, so the text is ready the moment
//! they stop. On Apple silicon Parakeet runs on the Neural Engine
//! (`neural`), elsewhere on the CPU. Everything runs on this machine; nothing
//! is sent anywhere.
#[cfg(test)]
mod bench_tests;
mod capture;
pub mod devices;
pub mod model;
#[cfg(target_os = "macos")]
mod neural;
mod session;
pub mod smart_turn;
mod spoken;
pub mod text;
pub mod turn;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub use capture::Microphone;
pub use model::Engine;
pub use session::{CallMode, Event, Options, Session};
pub use text::{add_replacement, apply_vocabulary, Replacement, VocabularyError};

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
    #[error("The speech model is in use; stop dictating first")]
    Busy,
}

/// A loaded Parakeet, either build.
pub enum Ear {
    Cpu(sherpa_onnx::OfflineRecognizer),
    #[cfg(target_os = "macos")]
    Neural(neural::Parakeet),
}

impl Ear {
    pub fn engine(&self) -> Engine {
        match self {
            Self::Cpu(_) => Engine::Cpu,
            #[cfg(target_os = "macos")]
            Self::Neural(_) => Engine::Neural,
        }
    }

    /// What was said in `samples` (mono, 16 kHz); empty when nothing was
    /// heard or the model failed on it.
    pub fn transcribe(&self, samples: &[f32]) -> String {
        if samples.is_empty() {
            return String::new();
        }
        match self {
            Self::Cpu(recognizer) => {
                let stream = recognizer.create_stream();
                stream.accept_waveform(16_000, samples);
                recognizer.decode(&stream);
                stream
                    .get_result()
                    .map(|result| result.text)
                    .unwrap_or_default()
            }
            #[cfg(target_os = "macos")]
            Self::Neural(parakeet) => parakeet.transcribe(samples).unwrap_or_default(),
        }
    }
}

/// The longest phrase the voice detector should hand over, in seconds: the
/// Neural build's window is 15 s, so a phrase fits in one. A Neural build
/// that falls back to the CPU one takes the shorter phrases as well.
fn longest_phrase(engine: Engine) -> f32 {
    match engine {
        Engine::Cpu => 20.0,
        Engine::Neural => 14.5,
    }
}

/// The recognizer, loaded on the first dictation and kept while the user keeps
/// dictating. The CPU build takes seconds to load and holds over half a
/// gigabyte; the Neural build loads in a tenth of a second and holds about
/// 70 MB. Either way the app releases it once idle.
pub struct Dictation {
    dir: PathBuf,
    recognizer: Mutex<Option<Arc<Ear>>>,
    /// Smart Turn, loaded for calls only and let go with the recognizer.
    smart_turn: Mutex<Option<Arc<smart_turn::Model>>>,
}

impl Dictation {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            recognizer: Mutex::new(None),
            smart_turn: Mutex::new(None),
        }
    }

    /// Some build is on disk, so dictation works.
    pub fn installed(&self) -> bool {
        model::installed(&self.dir)
    }

    /// The build on disk, if any.
    pub fn engine(&self) -> Option<Engine> {
        model::installed_engine(&self.dir)
    }

    /// The build this computer should use is not on disk yet (a Mac that
    /// still has only the CPU build, say).
    pub fn wants_upgrade(&self) -> bool {
        !Engine::preferred().installed(&self.dir)
    }

    pub fn install(&self, progress: impl FnMut(u64, u64)) -> Result<(), Error> {
        let result = model::install(&self.dir, progress);
        // A CPU recognizer loaded before the Neural build arrived goes once
        // no session holds it, so the next dictation loads the faster one.
        self.release();
        result
    }

    /// Deletes the model from disk, once no dictation holds it.
    pub fn remove(&self) -> Result<(), Error> {
        let slot = self.recognizer.lock().unwrap_or_else(|e| e.into_inner());
        if slot.as_ref().is_some_and(|ear| Arc::strong_count(ear) > 1) {
            return Err(Error::Busy);
        }
        drop(slot);
        self.release();
        model::remove(&self.dir)
    }

    /// What the model takes on disk, in bytes.
    pub fn disk_bytes(&self) -> u64 {
        model::disk_bytes(&self.dir)
    }

    /// Loads the recognizer ahead of the first dictation: the best build on
    /// disk, falling back to the CPU one when the Neural Engine refuses.
    pub fn prepare(&self) -> Result<Arc<Ear>, Error> {
        let mut slot = self.recognizer.lock().unwrap_or_else(|e| e.into_inner());
        let wanted = self.engine().ok_or(Error::NotInstalled)?;
        if let Some(ear) = slot.as_ref() {
            if ear.engine() == wanted || Arc::strong_count(ear) > 1 {
                return Ok(ear.clone());
            }
        }
        let ear = Arc::new(load(&self.dir, wanted)?);
        *slot = Some(ear.clone());
        Ok(ear)
    }

    /// Frees the recognizer's memory unless a session still uses it. Returns
    /// whether it was released; the next dictation loads it again.
    pub fn release(&self) -> bool {
        let mut turn = self.smart_turn.lock().unwrap_or_else(|e| e.into_inner());
        if turn
            .as_ref()
            .is_some_and(|model| Arc::strong_count(model) == 1)
        {
            *turn = None;
        }
        drop(turn);
        let mut slot = self.recognizer.lock().unwrap_or_else(|e| e.into_inner());
        match slot.as_ref() {
            Some(recognizer) if Arc::strong_count(recognizer) == 1 => {
                *slot = None;
                true
            }
            _ => false,
        }
    }

    /// Opens the microphone and starts transcribing. The microphone listens
    /// at once: a recognizer not yet in memory loads on the session's own
    /// thread, and what was said meanwhile is transcribed when it is ready.
    pub fn start(
        self: &Arc<Self>,
        on_event: impl Fn(Event) + Send + 'static,
    ) -> Result<Session, Error> {
        self.start_with(Options::default(), on_event)
    }

    /// As `start`, with the user's own words (`Options::vocabulary`) put in
    /// the transcript.
    pub fn start_with(
        self: &Arc<Self>,
        options: Options,
        on_event: impl Fn(Event) + Send + 'static,
    ) -> Result<Session, Error> {
        let (load, longest) = self.loader()?;
        Session::start(&self.dir, load, longest, None, options, on_event)
    }

    /// How a session brings the recognizer, and the longest phrase it takes.
    /// A missing model fails here, before the microphone opens.
    fn loader(self: &Arc<Self>) -> Result<(session::Loader, f32), Error> {
        let engine = self.engine().ok_or(Error::NotInstalled)?;
        let this = self.clone();
        let load: session::Loader = Box::new(move || {
            let ear = this.prepare()?;
            let hear: session::Hear = Box::new(move |samples| ear.transcribe(samples));
            Ok(hear)
        });
        Ok((load, longest_phrase(engine)))
    }

    /// Smart Turn, when it is on disk; a load that fails leaves the words to
    /// decide alone.
    fn smart_turn(&self) -> Option<Arc<smart_turn::Model>> {
        let mut slot = self.smart_turn.lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_none() && model::turn_installed(&self.dir) {
            let path = model::path(&self.dir, model::SMART_TURN.name).ok()?;
            *slot = smart_turn::Model::load(&path).ok().map(Arc::new);
        }
        slot.clone()
    }

    /// Opens the microphone for a call turn: as `start`, and the session
    /// ends the turn itself (`Event::TurnEnd`) at `pace`.
    pub fn start_call(
        self: &Arc<Self>,
        pace: &str,
        on_event: impl Fn(Event) + Send + 'static,
    ) -> Result<Session, Error> {
        let (load, longest) = self.loader()?;
        let call = CallMode {
            pace: pace.to_owned(),
            smart_turn: self.smart_turn(),
        };
        Session::start(
            &self.dir,
            load,
            longest,
            Some(call),
            Options::default(),
            on_event,
        )
    }

    /// Downloads Smart Turn when it is missing (32 MB).
    pub fn install_turn(&self) -> Result<(), Error> {
        model::install_turn(&self.dir)
    }
}

fn load(dir: &std::path::Path, engine: Engine) -> Result<Ear, Error> {
    match engine {
        #[cfg(target_os = "macos")]
        Engine::Neural => match neural::Parakeet::load(&dir.join(model::NEURAL_FOLDER)) {
            Ok(parakeet) => Ok(Ear::Neural(parakeet)),
            Err(error) if Engine::Cpu.installed(dir) => {
                let _ = error;
                load_cpu(dir).map(Ear::Cpu)
            }
            Err(error) => Err(error),
        },
        #[cfg(not(target_os = "macos"))]
        Engine::Neural => Err(Error::Load),
        Engine::Cpu => load_cpu(dir).map(Ear::Cpu),
    }
}

fn load_cpu(dir: &std::path::Path) -> Result<sherpa_onnx::OfflineRecognizer, Error> {
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

/// 16-bit mono PCM samples of a WAV file as floats, for the tests that hear
/// a recording.
#[cfg(test)]
pub(crate) fn wav16(bytes: &[u8]) -> Vec<f32> {
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        if &bytes[at..at + 4] == b"data" {
            let body = &bytes[at + 8..(at + 8 + size).min(bytes.len())];
            return body
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| i16::from_le_bytes(*pair) as f32 / 32768.0)
                .collect();
        }
        at += 8 + size + size % 2;
    }
    Vec::new()
}
