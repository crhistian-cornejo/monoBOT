//! The composer's dictation: commands over the `monobot-dictation` crate.
//! The model lives under the app data folder; one dictation runs at a time.
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use monobot_dictation::{Dictation, Event, Session};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

struct State {
    engine: Arc<Dictation>,
    session: Mutex<Option<Session>>,
    installing: AtomicBool,
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
    }))
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
/// progress, then loads it so the first dictation starts at once.
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
        })?;
        engine.prepare().map(|_| ())
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

/// The app sets the speech model up on its own the first time it runs, in the
/// background, so dictation works when the user first reaches for the mic.
/// When it cannot (no network, say) the composer offers the download instead.
pub fn init(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Ok(state) = state(&app) else { return };
        if state.engine.installed() {
            // Load it ahead of the first dictation.
            let engine = state.engine.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || engine.prepare().map(|_| ()))
                .await;
            return;
        }
        let _ = install(&app).await;
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
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
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
    tauri::async_runtime::spawn_blocking(move || session.stop())
        .await
        .map_err(|e| e.to_string())
}

/// Stops listening and drops what was said.
#[tauri::command]
pub async fn dictation_cancel(app: AppHandle) -> Result<(), String> {
    let state = state(&app)?;
    let session = state.session.lock().map_err(|e| e.to_string())?.take();
    if let Some(session) = session {
        tauri::async_runtime::spawn_blocking(move || session.cancel())
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
