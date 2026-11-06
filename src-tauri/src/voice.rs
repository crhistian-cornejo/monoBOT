//! Group calls' voices: commands over the `monobot-voice` crate. The model
//! lives beside dictation's under the app data folder; the page plays the
//! WAV each sentence comes back as.
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use monobot_voice::Voice;
use serde::Serialize;
use tauri::ipc::Response;
use tauri::{AppHandle, Emitter, Manager};

use crate::idle::IdleRelease;

/// How long the synthesizer stays in memory after the last sentence: a call
/// speaks in bursts, so pauses between turns do not reload it.
const IDLE: Duration = Duration::from_secs(2 * 60);

struct State {
    engine: Arc<Voice>,
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
        idle: IdleRelease::new(),
    }))
}

/// Lets the synthesizer go once no sentence has been spoken for `IDLE`.
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
/// progress. The first sentence loads it; it is not kept in memory before.
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
        })
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
    let state = state(&app)?;
    let engine = state.engine.clone();
    let count = tauri::async_runtime::spawn_blocking(move || engine.speakers())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string());
    release_when_idle(state);
    count
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
    let state = state(&app)?;
    let engine = state.engine.clone();
    let wav = tauri::async_runtime::spawn_blocking(move || engine.speak(&text, &lang, speaker))
        .await
        .map_err(|e| e.to_string())
        .and_then(|result| result.map_err(|e| e.to_string()));
    release_when_idle(state);
    Ok(Response::new(wav?))
}
