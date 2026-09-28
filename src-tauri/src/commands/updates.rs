use crate::{contracts::UpdateCheckDto, updates, AppState};
use tauri::State;

#[tauri::command]
pub fn check_for_update(
    _state: State<'_, AppState>,
    endpoint: Option<String>,
    current_version: String,
) -> Result<UpdateCheckDto, String> {
    updates::check_stable_channel(
        &reqwest::blocking::Client::new(),
        endpoint.as_deref().unwrap_or_default(),
        &current_version,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn verify_update_package(
    _state: State<'_, AppState>,
    path: String,
    sha256: String,
) -> Result<(), String> {
    updates::verify_package(path, &sha256).map_err(|e| e.to_string())
}
