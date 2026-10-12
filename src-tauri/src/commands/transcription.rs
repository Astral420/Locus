use crate::{contracts::PipelineStatusDto, AppState};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegmentDto {
    pub id: String,
    /// Empty when the segment has no speaker attribution (diarization did not
    /// run). The UI shows no speaker rather than an invented one.
    pub speaker_id: String,
    pub speaker_label: String,
    pub start_time: f32,
    pub end_time: f32,
    pub text: String,
    pub language: Option<String>,
    pub confidence: Option<f32>,
}

/// `SPEAKER_00` is a machine label; show people-friendly numbering. A name the
/// user typed (display_name) is used as-is and never reaches this function.
pub fn friendly_speaker_label(anonymous_label: &str) -> String {
    anonymous_label
        .strip_prefix("SPEAKER_")
        .and_then(|number| number.parse::<u32>().ok())
        .map(|number| format!("Speaker {}", number + 1))
        .unwrap_or_else(|| anonymous_label.to_owned())
}

/// Segments of the meeting's *current* transcript revision, each with the
/// speaker the diarization step aligned it to (if any).
pub fn load_transcript_segments(
    db: &crate::db::Database,
    meeting_id: &str,
) -> Result<Vec<TranscriptSegmentDto>, String> {
    let meeting_id = meeting_id.to_owned();
    db.run(move |connection| {
        let mut statement = connection
            .prepare(
                "SELECT s.id, s.start_seconds, s.end_seconds, s.text, s.language, s.confidence, \
                        sp.id, sp.display_name, sp.anonymous_label \
                 FROM transcript_segments s \
                 JOIN meetings m ON m.id = s.meeting_id \
                 LEFT JOIN speaker_alignments a ON a.id = ( \
                     SELECT id FROM speaker_alignments WHERE transcript_segment_id = s.id \
                     ORDER BY confidence DESC, id LIMIT 1) \
                 LEFT JOIN speakers sp ON sp.id = a.speaker_id \
                 WHERE s.meeting_id = ?1 \
                   AND (m.current_transcript_revision_id IS NULL \
                        OR s.transcript_revision_id = m.current_transcript_revision_id) \
                 ORDER BY s.ordinal",
            )
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([&meeting_id], |row| {
                let speaker_id: Option<String> = row.get(6)?;
                let display_name: Option<String> = row.get(7)?;
                let anonymous: Option<String> = row.get(8)?;
                Ok(TranscriptSegmentDto {
                    id: row.get(0)?,
                    start_time: row.get(1)?,
                    end_time: row.get(2)?,
                    text: row.get(3)?,
                    language: row.get(4)?,
                    confidence: row.get(5)?,
                    speaker_id: speaker_id.unwrap_or_default(),
                    speaker_label: display_name
                        .filter(|name| !name.trim().is_empty())
                        .or_else(|| anonymous.map(|label| friendly_speaker_label(&label)))
                        .unwrap_or_default(),
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string());
        rows
    })
    .map_err(|e| e.to_string())
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
    load_transcript_segments(&state.database, &meeting_id)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        db::Database,
        pipeline::diarization::{persist_alignments, DiarizationSegment},
        transcription::{TranscriptSegment, TranscriptionService},
    };

    fn segment(text: &str, start: f32, end: f32) -> TranscriptSegment {
        TranscriptSegment {
            start_seconds: start,
            end_seconds: end,
            text: text.into(),
            language: Some("en".into()),
            confidence: Some(0.9),
        }
    }

    fn setup() -> (Database, TranscriptionService) {
        let db = Database::in_memory().unwrap();
        db.run(|c| {
            c.execute("INSERT INTO meetings(id,title,title_origin,recorded_at,timezone) VALUES ('m1','T','placeholder','now','UTC')", [])
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .unwrap();
        let service = TranscriptionService::new(db.clone());
        (db, service)
    }

    #[test]
    fn friendly_labels() {
        assert_eq!(friendly_speaker_label("SPEAKER_00"), "Speaker 1");
        assert_eq!(friendly_speaker_label("SPEAKER_03"), "Speaker 4");
        assert_eq!(friendly_speaker_label("User"), "User");
    }

    #[test]
    fn undiarized_segments_have_no_speaker_not_an_invented_one() {
        let (db, service) = setup();
        service
            .persist_segments("m1", "whisper.cpp:test", Some("en"), &[segment("Hello", 0.0, 1.0)])
            .unwrap();
        let segments = load_transcript_segments(&db, "m1").unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "Hello");
        assert_eq!((segments[0].start_time, segments[0].end_time), (0.0, 1.0));
        assert_eq!(segments[0].speaker_id, "");
        assert_eq!(segments[0].speaker_label, "");
    }

    #[test]
    fn aligned_segments_carry_the_speaker_and_renames_show_up() {
        let (db, service) = setup();
        service
            .persist_segments(
                "m1",
                "whisper.cpp:test",
                Some("en"),
                &[segment("Hi there", 0.0, 2.0), segment("Hello back", 2.0, 4.0)],
            )
            .unwrap();
        persist_alignments(
            &db,
            "m1",
            &[
                DiarizationSegment { speaker: "SPEAKER_00".into(), start: 0.0, end: 2.0, source: "pyannote".into() },
                DiarizationSegment { speaker: "SPEAKER_01".into(), start: 2.0, end: 4.0, source: "pyannote".into() },
            ],
        )
        .unwrap();
        let segments = load_transcript_segments(&db, "m1").unwrap();
        assert_eq!(segments[0].speaker_label, "Speaker 1");
        assert_eq!(segments[1].speaker_label, "Speaker 2");
        assert_ne!(segments[0].speaker_id, segments[1].speaker_id);
        service.rename_speaker(&segments[0].speaker_id, "Dana").unwrap();
        assert_eq!(load_transcript_segments(&db, "m1").unwrap()[0].speaker_label, "Dana");
    }

    #[test]
    fn only_the_current_transcript_revision_is_listed() {
        let (db, service) = setup();
        service
            .persist_segments("m1", "whisper.cpp:a", Some("en"), &[segment("old take", 0.0, 1.0)])
            .unwrap();
        service
            .persist_segments("m1", "whisper.cpp:b", Some("en"), &[segment("new take", 0.0, 1.0)])
            .unwrap();
        let texts: Vec<_> = load_transcript_segments(&db, "m1")
            .unwrap()
            .into_iter()
            .map(|s| s.text)
            .collect();
        assert_eq!(texts, ["new take"]);
    }
}
