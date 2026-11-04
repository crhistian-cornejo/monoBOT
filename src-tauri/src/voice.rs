//! Group calls' voices: commands over the `monobot-voice` crate. The model
//! lives beside dictation's under the app data folder; the page plays the
//! WAV each sentence comes back as.
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use monobot_voice::Voice;
use serde::Serialize;
use tauri::ipc::Response;
use tauri::{AppHandle, Emitter, Manager};

struct State {
    engine: Arc<Voice>,
    installing: AtomicBool,
}

static STATE: OnceLock<State> = OnceLock::new();

fn models_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("models")
        .join("voice"))
}

fn state(app: &AppHandle) -> Result<&'static State, String> {
    if let Some(state) = STATE.get() {
        return Ok(state);
    }
    let dir = models_dir(app)?;
    Ok(STATE.get_or_init(|| State {
        engine: Arc::new(Voice::new(dir)),
        installing: AtomicBool::new(false),
    }))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceStatus {
    installed: bool,
    installing: bool,
    /// The model's download size, for the install prompt.
    bytes: u64,
}

#[tauri::command]
pub fn voice_status(app: AppHandle) -> Result<VoiceStatus, String> {
    let state = state(&app)?;
    Ok(VoiceStatus {
        installed: state.engine.installed(),
        installing: state.installing.load(Ordering::Acquire),
        bytes: monobot_voice::model::ARCHIVE.bytes,
    })
}

#[derive(Clone, Serialize)]
struct Progress {
    done: u64,
    total: u64,
}

/// Downloads, verifies and unpacks the model, reporting `voice-install`
/// progress, then loads it so the first sentence is spoken at once.
#[tauri::command]
pub async fn voice_install(app: AppHandle) -> Result<(), String> {
    let state = state(&app)?;
    if state.installing.swap(true, Ordering::AcqRel) {
        return Err("The voices are already downloading.".into());
    }
    let engine = state.engine.clone();
    let emitter = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut last = 0u64;
        engine.install(|done, total| {
            if done == total || done - last >= 1 << 20 {
                last = done;
                let _ = emitter.emit("voice-install", Progress { done, total });
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

/// How many voices there are to share among a group's members.
#[tauri::command]
pub async fn voice_speakers(app: AppHandle) -> Result<i32, String> {
    let engine = state(&app)?.engine.clone();
    tauri::async_runtime::spawn_blocking(move || engine.speakers())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Speaks one sentence in `lang` ("es" or "en") with voice `speaker`, and
/// returns the WAV bytes as an ArrayBuffer.
#[tauri::command]
pub async fn voice_speak(
    app: AppHandle,
    text: String,
    lang: String,
    speaker: i32,
) -> Result<Response, String> {
    let engine = state(&app)?.engine.clone();
    let wav = tauri::async_runtime::spawn_blocking(move || engine.speak(&text, &lang, speaker))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    Ok(Response::new(wav))
}
