//! The composer's dictation: commands over the `monobot-dictation` crate.
//! The model lives under the app data folder; one dictation runs at a time.
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use monobot_dictation::{Dictation, Event, Session};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::idle::IdleRelease;

/// How long Parakeet stays in memory after the last dictation. Long enough
/// that dictating a few messages in a row loads it once.
const IDLE: Duration = Duration::from_secs(3 * 60);

struct State {
    engine: Arc<Dictation>,
    session: Mutex<Option<Session>>,
    installing: AtomicBool,
    idle: IdleRelease,
}

static STATE: OnceLock<State> = OnceLock::new();

fn models_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("models")
        .join("dictation"))
}

fn state(app: &AppHandle) -> Result<&'static State, String> {
    if let Some(state) = STATE.get() {
        return Ok(state);
    }
    let dir = models_dir(app)?;
    Ok(STATE.get_or_init(|| State {
        engine: Arc::new(Dictation::new(dir)),
        session: Mutex::new(None),
        installing: AtomicBool::new(false),
        idle: IdleRelease::new(),
    }))
}

/// Lets Parakeet go once the user has not dictated for `IDLE`.
fn release_when_idle(state: &'static State) {
    let engine = state.engine.clone();
    state.idle.touch(IDLE, move || {
        if engine.release() {
            crate::memory::release_soon();
        }
    });
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationStatus {
    installed: bool,
    installing: bool,
    /// The model's download size, for the install prompt.
    bytes: u64,
}

#[tauri::command(async)]
pub fn dictation_status(app: AppHandle) -> Result<DictationStatus, String> {
    let state = state(&app)?;
    Ok(DictationStatus {
        installed: state.engine.installed(),
        installing: state.installing.load(Ordering::Acquire),
        bytes: monobot_dictation::model::total_bytes(),
    })
}

#[derive(Clone, Serialize)]
struct Progress {
    done: u64,
    total: u64,
}

/// Downloads the model and verifies it, reporting `dictation-install`
/// progress. It is loaded on the first dictation, not here: holding it in
/// memory costs over half a gigabyte while nobody is dictating.
async fn install(app: &AppHandle) -> Result<(), String> {
    let state = state(app)?;
    if state.installing.swap(true, Ordering::AcqRel) {
        return Err("The speech model is already downloading.".into());
    }
    let engine = state.engine.clone();
    let emitter = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut last = 0u64;
        engine.install(|done, total| {
            // About every megabyte, so the bar moves without flooding events.
            if done == total || done - last >= 1 << 20 {
                last = done;
                let _ = emitter.emit("dictation-install", Progress { done, total });
            }
        })
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|result| result.map_err(|e| e.to_string()));
    state.installing.store(false, Ordering::Release);
    result
}

#[tauri::command]
pub async fn dictation_install(app: AppHandle) -> Result<(), String> {
    install(&app).await
}

/// The app downloads the speech model on its own the first time it runs, in
/// the background, so dictation works when the user first reaches for the
/// mic. When it cannot (no network, say) the composer offers the download
/// instead. An installed model stays on disk until the first dictation.
pub fn init(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Ok(state) = state(&app) else { return };
        if !state.engine.installed() {
            let _ = install(&app).await;
        }
    });
}

/// Opens the microphone. Emits `dictation-level` (0 to 1) as it hears,
/// `dictation-partial` with the transcript so far, and `dictation-speech`
/// when the user starts or stops talking.
#[tauri::command]
pub async fn dictation_start(app: AppHandle) -> Result<(), String> {
    let state = state(&app)?;
    let previous = state.session.lock().map_err(|e| e.to_string())?.take();
    if let Some(previous) = previous {
        previous.cancel();
    }
    // Keep Parakeet loaded while the microphone is open.
    state.idle.hold();
    let engine = state.engine.clone();
    let emitter = app.clone();
    let session = tauri::async_runtime::spawn_blocking(move || {
        engine.start(move |event| match event {
            Event::Level(level) => {
                let _ = emitter.emit("dictation-level", level);
            }
            Event::Partial(text) => {
                let _ = emitter.emit("dictation-partial", text);
            }
            Event::Speech(speaking) => {
                let _ = emitter.emit("dictation-speech", speaking);
            }
        })
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|result| result.map_err(|e| e.to_string()));
    let session = match session {
        Ok(session) => session,
        Err(err) => {
            // A failed start (no microphone, say) must not pin the model.
            release_when_idle(state);
            return Err(err);
        }
    };
    *state.session.lock().map_err(|e| e.to_string())? = Some(session);
    Ok(())
}

/// Stops listening and returns the whole transcript.
#[tauri::command]
pub async fn dictation_stop(app: AppHandle) -> Result<String, String> {
    let state = state(&app)?;
    let session = state.session.lock().map_err(|e| e.to_string())?.take();
    let Some(session) = session else {
        return Ok(String::new());
    };
    let text = tauri::async_runtime::spawn_blocking(move || session.stop())
        .await
        .map_err(|e| e.to_string());
    release_when_idle(state);
    text
}

/// Stops listening and drops what was said.
#[tauri::command]
pub async fn dictation_cancel(app: AppHandle) -> Result<(), String> {
    let state = state(&app)?;
    let session = state.session.lock().map_err(|e| e.to_string())?.take();
    if let Some(session) = session {
        let cancelled = tauri::async_runtime::spawn_blocking(move || session.cancel())
            .await
            .map_err(|e| e.to_string());
        release_when_idle(state);
        cancelled?;
    }
    Ok(())
}
