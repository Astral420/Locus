use crate::{
    contracts::{MeetingDto, MeetingType, PipelineStatusDto, SlideDto},
    deletion, AppState,
};
use tauri::State;

#[tauri::command]
pub fn list_meetings(state: State<'_, AppState>) -> Result<Vec<MeetingDto>, String> {
    state
        .database
        .run(|connection| {
            let mut stmt = connection
                .prepare(
                    "SELECT 
                        m.id, 
                        m.title, 
                        m.title_origin, 
                        m.title_revision, 
                        m.recorded_at, 
                        m.timezone, 
                        m.duration_seconds, 
                        m.requested_type, 
                        m.detected_type, 
                        m.lifecycle, 
                        m.deleted_at,
                        (SELECT COUNT(*) FROM speakers s WHERE s.meeting_id = m.id) as speaker_count
                     FROM meetings m
                     WHERE m.deleted_at IS NULL
                     ORDER BY m.recorded_at DESC",
                )
                .map_err(|e| e.to_string())?;

            let rows = stmt
                .query_map([], |row| {
                    let req_type_str: String = row.get(7)?;
                    let req_type = match req_type_str.as_str() {
                        "meeting" => MeetingType::Meeting,
                        "lecture" => MeetingType::Lecture,
                        _ => MeetingType::Auto,
                    };
                    let speaker_cnt: i64 = row.get(11)?;

                    Ok(MeetingDto {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        title_origin: row.get(2)?,
                        title_revision: row.get(3)?,
                        recorded_at: row.get(4)?,
                        timezone: row.get(5)?,
                        duration_seconds: row.get(6)?,
                        requested_type: req_type,
                        detected_type: row.get(8)?,
                        lifecycle: row.get(9)?,
                        deleted_at: row.get(10)?,
                        speaker_count: Some(speaker_cnt),
                    })
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;

            Ok(rows)
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_meeting(state: State<'_, AppState>, id: String) -> Result<Option<MeetingDto>, String> {
    state
        .database
        .run(move |connection| {
            let mut stmt = connection
                .prepare(
                    "SELECT 
                        m.id, 
                        m.title, 
                        m.title_origin, 
                        m.title_revision, 
                        m.recorded_at, 
                        m.timezone, 
                        m.duration_seconds, 
                        m.requested_type, 
                        m.detected_type, 
                        m.lifecycle, 
                        m.deleted_at,
                        (SELECT COUNT(*) FROM speakers s WHERE s.meeting_id = m.id) as speaker_count
                     FROM meetings m
                     WHERE m.id = ?1 AND m.deleted_at IS NULL",
                )
                .map_err(|e| e.to_string())?;

            let mut rows = stmt
                .query_map([&id], |row| {
                    let req_type_str: String = row.get(7)?;
                    let req_type = match req_type_str.as_str() {
                        "meeting" => MeetingType::Meeting,
                        "lecture" => MeetingType::Lecture,
                        _ => MeetingType::Auto,
                    };
                    let speaker_cnt: i64 = row.get(11)?;

                    Ok(MeetingDto {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        title_origin: row.get(2)?,
                        title_revision: row.get(3)?,
                        recorded_at: row.get(4)?,
                        timezone: row.get(5)?,
                        duration_seconds: row.get(6)?,
                        requested_type: req_type,
                        detected_type: row.get(8)?,
                        lifecycle: row.get(9)?,
                        deleted_at: row.get(10)?,
                        speaker_count: Some(speaker_cnt),
                    })
                })
                .map_err(|e| e.to_string())?;

            if let Some(first) = rows.next() {
                Ok(Some(first.map_err(|e| e.to_string())?))
            } else {
                Ok(None)
            }
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_meeting(state: State<'_, AppState>, meeting_id: String) -> Result<(), String> {
    deletion::tombstone_and_cleanup(&state.database, &state.media_root, meeting_id)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_pipeline_status(
    state: State<'_, AppState>,
    meeting_id: String,
) -> Result<Vec<PipelineStatusDto>, String> {
    state
        .database
        .run(move |connection| {
            let mut stmt = connection
                .prepare(
                    "SELECT id, kind, state, progress, reason 
                     FROM jobs 
                     WHERE meeting_id = ?1 
                     ORDER BY created_at ASC",
                )
                .map_err(|e| e.to_string())?;

            let rows = stmt
                .query_map([&meeting_id], |row| {
                    Ok(PipelineStatusDto {
                        job_id: row.get(0)?,
                        kind: row.get(1)?,
                        state: row.get(2)?,
                        progress: row.get(3)?,
                        reason: row.get(4)?,
                    })
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;

            Ok(rows)
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn retry_pipeline_step(state: State<'_, AppState>, job_id: String) -> Result<(), String> {
    state.pipeline.retry(&job_id)
}

#[tauri::command]
pub fn get_slides(state: State<'_, AppState>, meeting_id: String) -> Result<Vec<SlideDto>, String> {
    state
        .database
        .run(move |connection| {
            let mut stmt = connection
                .prepare(
                    "SELECT 
                        s.id,
                        so.ordinal,
                        so.timestamp_seconds,
                        s.relative_path,
                        COALESCE(s.ocr_text, '')
                     FROM slides s
                     JOIN slide_occurrences so ON so.slide_id = s.id AND so.meeting_id = s.meeting_id
                     WHERE s.meeting_id = ?1
                     ORDER BY so.ordinal ASC",
                )
                .map_err(|e| e.to_string())?;

            let rows = stmt
                .query_map([&meeting_id], |row| {
                    let rel_path: String = row.get(3)?;
                    let image_url = if rel_path.starts_with('/') || rel_path.starts_with("http") {
                        rel_path
                    } else {
                        format!("/{}", rel_path)
                    };

                    Ok(SlideDto {
                        id: row.get(0)?,
                        ordinal: row.get(1)?,
                        timestamp: row.get(2)?,
                        image_url,
                        ocr_text: row.get(4)?,
                        repeated_timestamps: None,
                    })
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;

            Ok(rows)
        })
        .map_err(|e| e.to_string())
}
