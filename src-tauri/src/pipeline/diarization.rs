use crate::{
    db::Database,
    sidecar::client::{SidecarClient, SidecarError},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiarizationSegment {
    pub start: f32,
    pub end: f32,
    pub speaker: String,
    #[serde(default = "default_source")]
    pub source: String,
}

fn default_source() -> String {
    "mixed".into()
}

#[derive(Debug, Deserialize)]
struct DiarizationResponse {
    segments: Vec<DiarizationSegment>,
    /// Set by the sidecar when no real diarization model is available. The host
    /// must skip the step; it must never substitute guessed speaker labels.
    #[serde(default)]
    unavailable: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiarizationOutcome {
    Segments(Vec<DiarizationSegment>),
    Unavailable(String),
}

pub fn diarize(
    client: &mut SidecarClient,
    audio_path: &str,
    single_person_mic: bool,
) -> Result<DiarizationOutcome, SidecarError> {
    let response: DiarizationResponse = client.call(
        "diarize",
        serde_json::json!({
            "audio_path": audio_path,
            "single_person": single_person_mic,
        }),
    )?;
    Ok(match response.unavailable {
        Some(reason) => DiarizationOutcome::Unavailable(reason),
        None => DiarizationOutcome::Segments(response.segments),
    })
}

/// Returns the speaker with the greatest overlap for each transcript segment.
/// Equal/weak overlaps remain explicitly uncertain instead of inventing identity.
pub fn align_transcript_segment(
    start: f32,
    end: f32,
    candidates: &[DiarizationSegment],
) -> Option<(&DiarizationSegment, f32, bool)> {
    let duration = (end - start).max(0.001);
    candidates
        .iter()
        .filter_map(|candidate| {
            let overlap = (end.min(candidate.end) - start.max(candidate.start)).max(0.0);
            (overlap > 0.0).then_some((candidate, overlap, overlap / duration))
        })
        .max_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(segment, overlap, coverage)| (segment, overlap, coverage < 0.5))
}

pub fn persist_alignments(
    db: &Database,
    meeting_id: &str,
    segments: &[DiarizationSegment],
) -> Result<(), String> {
    let meeting_id = meeting_id.to_owned();
    let segments = segments.to_vec();
    db.run(move |connection| {
        let transcript_segments: Vec<(String, f32, f32)> = {
            let mut statement = connection.prepare("SELECT id, start_seconds, end_seconds FROM transcript_segments WHERE transcript_revision_id = (SELECT current_transcript_revision_id FROM meetings WHERE id = ?1) ORDER BY ordinal").map_err(|e| e.to_string())?;
            let rows = statement
                .query_map([&meeting_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .map_err(|e| e.to_string())?
                .collect::<Result<_, _>>()
                .map_err(|e: rusqlite::Error| e.to_string())?;
            rows
        };
        let tx = connection.transaction().map_err(|e| e.to_string())?;
        // Re-running diarization replaces earlier alignments instead of stacking them.
        tx.execute("DELETE FROM speaker_alignments WHERE transcript_segment_id IN (SELECT id FROM transcript_segments WHERE meeting_id = ?1)", [&meeting_id]).map_err(|e| e.to_string())?;
        for segment in &segments {
            tx.execute("INSERT OR IGNORE INTO speakers(id, meeting_id, anonymous_label, display_name) VALUES (?1, ?2, ?3, NULL)", (Uuid::new_v4().to_string(), &meeting_id, &segment.speaker)).map_err(|e| e.to_string())?;
        }
        for (transcript_id, start, end) in transcript_segments {
            let Some((speaker, overlap, uncertain)) = align_transcript_segment(start, end, &segments) else { continue };
            let speaker_id: String = tx.query_row("SELECT id FROM speakers WHERE meeting_id = ?1 AND anonymous_label = ?2", (&meeting_id, &speaker.speaker), |row| row.get(0)).map_err(|e| e.to_string())?;
            tx.execute("INSERT INTO speaker_alignments(id, transcript_segment_id, speaker_id, start_seconds, end_seconds, confidence, source, uncertainty) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)", (Uuid::new_v4().to_string(), &transcript_id, &speaker_id, speaker.start.max(start), speaker.end.min(end), overlap, &speaker.source, uncertain.then_some("partial_overlap"))).map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_overlap_is_marked_uncertain() {
        let segment = DiarizationSegment {
            start: 1.0,
            end: 2.0,
            speaker: "SPEAKER_00".into(),
            source: "mixed".into(),
        };
        let (_, _, uncertain) = align_transcript_segment(0.0, 3.0, &[segment]).unwrap();
        assert!(uncertain);
    }
}
