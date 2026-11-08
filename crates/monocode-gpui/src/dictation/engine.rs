//! src-tauri/src/dictation.rs: the commands the composer's dictation calls,
//! over the `monobot-dictation` crate. The model lives in Tauri's data
//! folder, so either app reuses what the other downloaded; one dictation
//! runs at a time.
use monobot_dictation::{Dictation, Event, Session};
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;

/// How long Parakeet stays in memory after the last dictation. The CPU build
/// holds over half a gigabyte and takes seconds to load, so it is kept while
/// the user dictates a few messages in a row. The Neural build holds about
/// 70 MB and loads in a fraction of a second, so it goes sooner.
fn idle(engine: Option<monobot_dictation::Engine>) -> Duration {
    match engine {
        Some(monobot_dictation::Engine::Neural) => Duration::from_secs(45),
        _ => Duration::from_secs(3 * 60),
    }
}

/// `dictation_status`, with the download's progress for a composer that
/// follows a setup it did not start.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Status {
    pub installed: bool,
    pub installing: bool,
    /// The model's download size, for the install prompt.
    pub bytes: u64,
    pub done: u64,
    pub total: u64,
}

/// What the composers dictate with; tests stand in for the microphone.
pub(crate) trait Engine: Send + Sync {
    fn status(&self) -> Status;
    /// `init`: downloads the model the first time the app runs. An installed
    /// model stays on disk until the first dictation.
    fn prepare(&self);
    /// `dictation_install`: downloads and verifies the model.
    fn install(&self, progress: &mut dyn FnMut(u64, u64)) -> Result<(), String>;
    /// `dictation_start`: opens the microphone, ending any other dictation.
    /// The ticket names this one for stop and cancel.
    fn start(&self, on_event: Box<dyn Fn(Event) + Send>) -> Result<u64, String>;
    /// `dictation_stop`: the whole transcript, or nothing once another
    /// dictation took the microphone.
    fn stop(&self, ticket: u64) -> String;
    /// `dictation_cancel`: stops listening and drops what was said.
    fn cancel(&self, ticket: u64);
}

/// The app's one engine.
pub(crate) fn local() -> Arc<dyn Engine> {
    static LOCAL: OnceLock<Arc<Local>> = OnceLock::new();
    LOCAL
        .get_or_init(|| Arc::new(Local::new(monocode_core::dictation::models_dir())))
        .clone()
}

struct Local {
    engine: Arc<Dictation>,
    session: Mutex<Option<(u64, Session)>>,
    tickets: AtomicU64,
    installing: AtomicBool,
    done: AtomicU64,
    total: AtomicU64,
    /// Counts dictations, so a release scheduled after one is skipped when
    /// another started before it ran.
    uses: Arc<AtomicU64>,
}

impl Local {
    fn new(dir: std::path::PathBuf) -> Self {
        Self {
            engine: Arc::new(Dictation::new(dir)),
            session: Mutex::new(None),
            tickets: AtomicU64::new(0),
            installing: AtomicBool::new(false),
            done: AtomicU64::new(0),
            total: AtomicU64::new(0),
            uses: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Lets Parakeet go once no dictation has started for `self.idle`.
    fn release_when_idle(&self) {
        let this_use = self.uses.fetch_add(1, Ordering::AcqRel) + 1;
        let uses = self.uses.clone();
        let engine = self.engine.clone();
        let idle = idle(engine.engine());
        let _ = std::thread::Builder::new()
            .name("dictation-idle".into())
            .spawn(move || {
                std::thread::sleep(idle);
                if uses.load(Ordering::Acquire) == this_use {
                    engine.release();
                }
            });
    }

    fn take(&self, ticket: u64) -> Option<Session> {
        let mut slot = self.session.lock().unwrap_or_else(|e| e.into_inner());
        if slot.as_ref().is_some_and(|(current, _)| *current == ticket) {
            slot.take().map(|(_, session)| session)
        } else {
            None
        }
    }
}

impl Engine for Local {
    fn status(&self) -> Status {
        Status {
            installed: self.engine.installed(),
            installing: self.installing.load(Ordering::Acquire),
            bytes: monobot_dictation::model::total_bytes(),
            done: self.done.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
        }
    }

    fn prepare(&self) {
        // The first run fetches the model; a Mac that still has only the CPU
        // build fetches the Neural one, dictating with the CPU meanwhile and
        // deleting it after. When it cannot (no network, say) the composer
        // offers it again.
        if self.engine.wants_upgrade() {
            let _ = self.install(&mut |_, _| {});
        }
    }

    fn install(&self, progress: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
        if self.installing.swap(true, Ordering::AcqRel) {
            return Err("The speech model is already downloading.".into());
        }
        self.done.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
        let result = self
            .engine
            .install(|done, total| {
                self.done.store(done, Ordering::Relaxed);
                self.total.store(total, Ordering::Relaxed);
                progress(done, total);
            })
            .map_err(|e| e.to_string());
        self.installing.store(false, Ordering::Release);
        result
    }

    fn start(&self, on_event: Box<dyn Fn(Event) + Send>) -> Result<u64, String> {
        let previous = self
            .session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some((_, previous)) = previous {
            previous.cancel();
        }
        // Keep Parakeet loaded while the microphone is open: this cancels a
        // release already waiting.
        self.uses.fetch_add(1, Ordering::AcqRel);
        let opened = (|| {
            // A missing model fails here, before anything asks for the microphone.
            self.engine.prepare().map_err(|e| e.to_string())?;
            super::permission::microphone()?;
            self.engine.start(on_event).map_err(|e| e.to_string())
        })();
        let session = match opened {
            Ok(session) => session,
            Err(err) => {
                // A failed start (no microphone, say) must not pin the model.
                self.release_when_idle();
                return Err(err);
            }
        };
        let ticket = self.tickets.fetch_add(1, Ordering::Relaxed) + 1;
        let replaced = self
            .session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace((ticket, session));
        if let Some((_, replaced)) = replaced {
            replaced.cancel();
        }
        Ok(ticket)
    }

    fn stop(&self, ticket: u64) -> String {
        let Some(session) = self.take(ticket) else {
            return String::new();
        };
        let text = session.stop();
        self.release_when_idle();
        text
    }

    fn cancel(&self, ticket: u64) {
        if let Some(session) = self.take(ticket) {
            session.cancel();
            self.release_when_idle();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_without_the_model_needs_it_and_stops_nothing() {
        let dir = std::env::temp_dir().join(format!("monocode-dictation-{}", std::process::id()));
        let engine = Local::new(dir.clone());
        let status = engine.status();
        assert!(!status.installed && !status.installing);
        assert_eq!(status.bytes, monobot_dictation::model::total_bytes());
        // Without the model nothing opens the microphone.
        assert_eq!(
            engine.start(Box::new(|_| {})),
            Err("The speech model is not installed".into())
        );
        assert_eq!(engine.stop(1), "");
        engine.cancel(1);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 16-bit PCM samples of a WAV file, as floats.
    fn wav(bytes: &[u8]) -> (u32, Vec<f32>) {
        assert_eq!(&bytes[..4], b"RIFF");
        let mut at = 12;
        let mut rate = 0;
        while at + 8 <= bytes.len() {
            let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
            let body = &bytes[at + 8..(at + 8 + size).min(bytes.len())];
            match &bytes[at..at + 4] {
                b"fmt " => {
                    assert_eq!(u16::from_le_bytes([body[0], body[1]]), 1, "PCM");
                    assert_eq!(u16::from_le_bytes([body[2], body[3]]), 1, "mono");
                    assert_eq!(u16::from_le_bytes([body[14], body[15]]), 16, "16-bit");
                    rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
                }
                b"data" => {
                    let samples = body
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|pair| i16::from_le_bytes(*pair) as f32 / 32768.)
                        .collect();
                    return (rate, samples);
                }
                _ => {}
            }
            at += 8 + size + size % 2;
        }
        panic!("no data chunk");
    }

    /// Transcribes a recording with the model in the desktop app's folder,
    /// as each phrase is transcribed while dictating. Needs the model and a
    /// 16 kHz mono WAV, and beside it, optionally, what it says in a .txt:
    /// `MONOBOT_DICTATION_WAV=speech.wav cargo test ... -- --ignored
    /// --nocapture transcribes_a_recording`.
    #[test]
    #[ignore]
    fn transcribes_a_recording_with_the_shared_model() {
        let path = std::env::var("MONOBOT_DICTATION_WAV").expect("MONOBOT_DICTATION_WAV");
        let expected = std::fs::read_to_string(std::path::Path::new(&path).with_extension("txt"));
        let (rate, samples) = wav(&std::fs::read(path).unwrap());
        let engine = Dictation::new(monocode_core::dictation::models_dir());
        assert!(
            engine.installed(),
            "the speech model is in the desktop app's folder"
        );
        assert_eq!(rate, 16_000, "a 16 kHz recording");
        let started = std::time::Instant::now();
        let ear = engine.prepare().unwrap();
        eprintln!("loaded {:?} in {:?}", ear.engine(), started.elapsed());
        let heard = ear.transcribe(&samples);
        eprintln!("heard {:.1}s: {heard}", samples.len() as f32 / rate as f32);
        if let Ok(expected) = expected {
            let words = |text: &str| {
                text.to_lowercase()
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|word| !word.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            };
            assert_eq!(words(&heard), words(&expected));
        } else {
            assert!(!heard.trim().is_empty());
        }
    }

    /// Opens this machine's microphone through the app's engine for two
    /// seconds: the waveform's levels arrive and stopping hands back the
    /// transcript. `cargo test ... -- --ignored --nocapture opens_the_microphone`.
    #[test]
    #[ignore]
    fn opens_the_microphone() {
        let engine = local();
        let (events, heard) = std::sync::mpsc::channel();
        let ticket = engine
            .start(Box::new(move |event| {
                let _ = events.send(event);
            }))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_secs(2));
        let text = engine.stop(ticket);
        let levels: Vec<f32> = heard
            .try_iter()
            .filter_map(|event| match event {
                Event::Level(level) => Some(level),
                _ => None,
            })
            .collect();
        eprintln!(
            "{} levels, loudest {:?}, heard {text:?}",
            levels.len(),
            levels.iter().copied().reduce(f32::max)
        );
        assert!(levels.len() > 10, "{levels:?}");
        // The ticket is spent: stopping again hears nothing.
        assert_eq!(engine.stop(ticket), "");
    }
}
