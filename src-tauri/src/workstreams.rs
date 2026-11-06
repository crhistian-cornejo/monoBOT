//! A workstream's shared folder: plans, notes, findings and deliverables that
//! are not code (code ships as pull requests). It lives under the app data
//! folder so every agent of the workstream, in whatever worktree, can reach it.
use std::path::PathBuf;

use tauri::{AppHandle, Manager};

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn folder(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err("Invalid workstream id.".into());
    }
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("workstreams")
        .join(id))
}

/// Creates the workstream's shared folder if needed and returns its path.
#[tauri::command]
pub async fn workstream_folder(app: AppHandle, id: String) -> Result<String, String> {
    let path = folder(&app, &id)?;
    let created = path.clone();
    tauri::async_runtime::spawn_blocking(move || std::fs::create_dir_all(&created))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workstream_ids_stay_inside_the_folder() {
        assert!(valid_id("a1b2-c3"));
        assert!(!valid_id("../x"));
        assert!(!valid_id("a/b"));
        assert!(!valid_id(""));
    }
}
