use crate::{
    capture::{preflight, CaptureOptions},
    contracts::{CaptureLifecycle, CaptureSource, MeetingType, RecordingStateDto},
    AppState,
};
use tauri::State;

#[tauri::command]
pub fn show_recording_window(window: tauri::Window) -> Result<(), String> {
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn start_recording(
    state: State<'_, AppState>,
    sources: Vec<CaptureSource>,
    meeting_type: MeetingType,
    title: Option<String>,
    single_person_mic: Option<bool>,
) -> Result<RecordingStateDto, String> {
    let options = CaptureOptions {
        sources,
        meeting_type,
        single_person_mic: single_person_mic.unwrap_or(false),
        title: title.unwrap_or_default(),
        output_root: state.media_root.clone(),
    };
    preflight::validate_storage(&options).map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    crate::capture::linux::preflight(&options).map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    crate::capture::windows::preflight(&options).map_err(|e| e.to_string())?;
    state.capture.start(options).map_err(|e| e.to_string())
}
#[tauri::command]
pub fn pause_recording(state: State<'_, AppState>) -> Result<RecordingStateDto, String> {
    state
        .capture
        .transition(CaptureLifecycle::Paused)
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn resume_recording(state: State<'_, AppState>) -> Result<RecordingStateDto, String> {
    state
        .capture
        .transition(CaptureLifecycle::Recording)
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn stop_recording(state: State<'_, AppState>) -> Result<RecordingStateDto, String> {
    state
        .capture
        .transition(CaptureLifecycle::Finalizing)
        .map_err(|e| e.to_string())?;
    let result = state
        .capture
        .transition(CaptureLifecycle::Saved)
        .map_err(|e| e.to_string())?;
    if let Some(meeting_id) = result.meeting_id.as_deref() {
        state.pipeline.enqueue(
            meeting_id,
            crate::pipeline::orchestrator::PipelineMode::Balanced,
        )?;
    }
    Ok(result)
}
