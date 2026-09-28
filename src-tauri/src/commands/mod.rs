pub mod knowledge;
pub mod meetings;
pub mod models;
pub mod recording;
pub mod storage;
pub mod summarization;
pub mod transcription;
pub mod updates;

use crate::AppState;
use tauri::State;

#[tauri::command]
pub fn greet(name: String) -> String {
    format!("Hello, {name}. Locus local engine is ready.")
}

#[tauri::command]
pub fn get_recording_state(state: State<'_, AppState>) -> crate::contracts::RecordingStateDto {
    state.capture.state()
}
