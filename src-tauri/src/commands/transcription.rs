use crate::{contracts::PipelineStatusDto, AppState};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegmentDto {
    pub id: String,
    pub start_seconds: f32,
    pub end_seconds: f32,
    pub text: String,
    pub language: Option<String>,
    pub confidence: Option<f32>,
}

#[tauri::command]
pub fn get_transcription_status(
    state: State<'_, AppState>,
    meeting_id: String,
) -> Result<crate::transcription::TranscriptionStatus, String> {
    state.transcription.status(&meeting_id)
}

#[tauri::command]
pub fn set_meeting_language(
    state: State<'_, AppState>,
    meeting_id: String,
    language: Option<String>,
) -> Result<(), String> {
    state
        .transcription
        .set_language(&meeting_id, language.as_deref())
}

#[tauri::command]
pub fn rename_speaker(
    state: State<'_, AppState>,
    speaker_id: String,
    display_name: String,
) -> Result<(), String> {
    state
        .transcription
        .rename_speaker(&speaker_id, &display_name)
}

#[tauri::command]
pub fn list_transcript_segments(
    state: State<'_, AppState>,
    meeting_id: String,
) -> Result<Vec<TranscriptSegmentDto>, String> {
    state.database.run(move |connection| {
        let mut statement = connection.prepare("SELECT id, start_seconds, end_seconds, text, language, confidence FROM transcript_segments WHERE meeting_id = ?1 ORDER BY ordinal").map_err(|e| e.to_string())?;
        let result = statement.query_map([meeting_id], |row| Ok(TranscriptSegmentDto { id: row.get(0)?, start_seconds: row.get(1)?, end_seconds: row.get(2)?, text: row.get(3)?, language: row.get(4)?, confidence: row.get(5)? })).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string());
        result
    }).map_err(|e| e.to_string())
}

#[allow(dead_code)]
fn _pipeline_status(
    job_id: String,
    kind: String,
    state: String,
    progress: f64,
) -> PipelineStatusDto {
    PipelineStatusDto {
        job_id,
        kind,
        state,
        progress,
        reason: None,
    }
}
