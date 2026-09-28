use crate::{contracts::StorageInfoDto, deletion, AppState};
use tauri::State;

#[tauri::command]
pub fn get_storage_info(state: State<'_, AppState>) -> StorageInfoDto {
    let capture_active = !matches!(
        state.capture.state().state,
        crate::contracts::CaptureLifecycle::Idle
    );
    state.storage.info(capture_active)
}

#[tauri::command]
pub fn migrate_models(state: State<'_, AppState>, target_path: String) -> Result<String, String> {
    let capture_active = !matches!(
        state.capture.state().state,
        crate::contracts::CaptureLifecycle::Idle
    );
    let result = state
        .storage
        .migrate_models(target_path, capture_active, true)
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "{}; originals retained for optional cleanup",
        result.state
    ))
}

#[tauri::command]
pub fn relocate_storage(state: State<'_, AppState>, target_path: String) -> Result<String, String> {
    let capture_active = !matches!(
        state.capture.state().state,
        crate::contracts::CaptureLifecycle::Idle
    );
    let result = state
        .storage
        .relocate_data(target_path, capture_active, true)
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "{}; restart is required before writable stores switch",
        result.state
    ))
}

#[tauri::command]
pub fn retry_cleanup(state: State<'_, AppState>) -> Result<usize, String> {
    deletion::retry_pending_cleanup(&state.database, &state.media_root).map_err(|e| e.to_string())
}
