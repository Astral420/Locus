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

/// Returns the speaker with the greatest *total* overlap for each transcript segment, that overlap
/// in seconds, and whether it stays uncertain (covering under half of the segment).
///
/// Overlaps are summed per speaker instead of comparing single intervals: two short turns by the
/// same person inside one Whisper segment are more evidence than a brief interjection by someone
/// else, and taking only the longest interval made a segment jump to whoever had the longest single
/// slice. Weak totals remain explicitly uncertain instead of inventing identity.
pub fn align_transcript_segment(
    start: f32,
    end: f32,
    candidates: &[DiarizationSegment],
) -> Option<(&DiarizationSegment, f32, bool)> {
    let duration = (end - start).max(0.001);
    // (representative interval, total overlap, largest single overlap, earliest start)
    let mut totals: Vec<(&DiarizationSegment, f32, f32, f32)> = Vec::new();
    for candidate in candidates {
        let overlap = (end.min(candidate.end) - start.max(candidate.start)).max(0.0);
        if overlap <= 0.0 {
            continue;
        }
        match totals
            .iter_mut()
            .find(|(seen, ..)| seen.speaker == candidate.speaker)
        {
            Some(entry) => {
                entry.1 += overlap;
                if overlap > entry.2 {
                    entry.0 = candidate;
                    entry.2 = overlap;
                }
                entry.3 = entry.3.min(candidate.start);
            }
            None => totals.push((candidate, overlap, overlap, candidate.start)),
        }
    }
    totals
        .into_iter()
        // Ties go to whoever started speaking first, which keeps re-runs byte-identical.
        .max_by(|a, b| a.1.total_cmp(&b.1).then_with(|| b.3.total_cmp(&a.3)))
        .map(|(speaker, overlap, _, _)| (speaker, overlap, overlap / duration < 0.5))
}

/// Union of the chosen speaker's overlapping intervals, clipped to the transcript segment.
/// A speaker's two turns inside one segment are one span, not the longest of the two.
pub fn aligned_span(
    start: f32,
    end: f32,
    speaker: &str,
    candidates: &[DiarizationSegment],
) -> Option<(f32, f32)> {
    let mut span: Option<(f32, f32)> = None;
    for candidate in candidates.iter().filter(|item| item.speaker == speaker) {
        if (end.min(candidate.end) - start.max(candidate.start)).max(0.0) <= 0.0 {
            continue;
        }
        let clipped = (start.max(candidate.start), end.min(candidate.end));
        span = Some(match span {
            Some((low, high)) => (low.min(clipped.0), high.max(clipped.1)),
            None => clipped,
        });
    }
    span
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
            // The stored span covers every interval of that speaker inside this segment, so the
            // UI does not hide the second half of a sentence they spoke.
            let (span_start, span_end) = aligned_span(start, end, &speaker.speaker, &segments)
                .unwrap_or((speaker.start.max(start), speaker.end.min(end)));
            tx.execute("INSERT INTO speaker_alignments(id, transcript_segment_id, speaker_id, start_seconds, end_seconds, confidence, source, uncertainty) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)", (Uuid::new_v4().to_string(), &transcript_id, &speaker_id, span_start, span_end, overlap, &speaker.source, uncertain.then_some("partial_overlap"))).map_err(|e| e.to_string())?;
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

    #[test]
    fn a_sidecar_response_with_diagnostics_still_parses() {
        // The host reads only segments/unavailable; the added diagnostics field is additive, and
        // the response must not start rejecting payloads because of it.
        let payload = r#"{
            "segments": [{"start": 0.0, "end": 1.5, "speaker": "SPEAKER_00", "source": "mixed"}],
            "diagnostics": {
                "engine": "sherpa-onnx",
                "audio_seconds": 12.5,
                "settings": {"threshold": 0.5, "min_speaker_seconds": 0.0},
                "speakers": [{"speaker": "SPEAKER_00", "seconds": 1.5, "segments": 1}]
            }
        }"#;
        let response: DiarizationResponse = serde_json::from_str(payload).unwrap();
        assert_eq!(response.segments.len(), 1);
        assert_eq!(response.segments[0].speaker, "SPEAKER_00");
        assert!(response.unavailable.is_none());
    }

    #[test]
    fn a_segment_without_a_source_still_defaults_to_the_mix() {
        let payload = r#"{"segments": [{"start": 0.0, "end": 1.0, "speaker": "User"}]}"#;
        let response: DiarizationResponse = serde_json::from_str(payload).unwrap();
        assert_eq!(response.segments[0].source, "mixed");
    }

    fn turn(speaker: &str, start: f32, end: f32) -> DiarizationSegment {
        DiarizationSegment {
            start,
            end,
            speaker: speaker.into(),
            source: "mixed".into(),
        }
    }

    #[test]
    fn a_speakers_two_short_turns_beat_another_speakers_single_long_one() {
        let turns = vec![
            turn("SPEAKER_00", 0.0, 4.0),
            turn("SPEAKER_01", 4.0, 6.0),
            turn("SPEAKER_01", 6.0, 8.0),
        ];
        // 3.5..7.5: SPEAKER_00 covers 0.5 s, SPEAKER_01 covers 2.0 s + 1.5 s. The single longest
        // interval is SPEAKER_01's, but it is the sum that decides.
        let (chosen, overlap, uncertain) = align_transcript_segment(3.5, 7.5, &turns).unwrap();
        assert_eq!(chosen.speaker, "SPEAKER_01");
        assert!((overlap - 3.5).abs() < 1e-5, "{overlap}");
        assert!(!uncertain);
    }

    #[test]
    fn the_sum_only_wins_when_it_is_actually_larger() {
        let turns = vec![
            turn("SPEAKER_00", 0.0, 0.5),
            turn("SPEAKER_01", 0.5, 3.0),
            turn("SPEAKER_00", 3.0, 3.4),
        ];
        // SPEAKER_00: 0.5 + 0.4 = 0.9 s. SPEAKER_01: 2.5 s in one turn.
        let (chosen, overlap, _) = align_transcript_segment(0.0, 3.5, &turns).unwrap();
        assert_eq!(chosen.speaker, "SPEAKER_01");
        assert!((overlap - 2.5).abs() < 1e-5, "{overlap}");
    }

    #[test]
    fn equal_totals_keep_the_earliest_speaker_regardless_of_input_order() {
        let turns = vec![turn("SPEAKER_01", 2.0, 4.0), turn("SPEAKER_00", 0.0, 2.0)];
        let (chosen, overlap, _) = align_transcript_segment(0.0, 4.0, &turns).unwrap();
        assert_eq!(chosen.speaker, "SPEAKER_00");
        assert!((overlap - 2.0).abs() < 1e-5);
    }

    #[test]
    fn a_segment_outside_every_turn_gets_no_speaker() {
        let turns = vec![turn("SPEAKER_00", 5.0, 6.0)];
        assert!(align_transcript_segment(0.0, 4.0, &turns).is_none());
    }

    #[test]
    fn summed_overlap_decides_uncertainty_too() {
        let turns = vec![turn("SPEAKER_00", 0.0, 1.0), turn("SPEAKER_00", 2.0, 3.0)];
        // 2.0 s of a 4.0 s segment is exactly half: attributed, not flagged. Looking at one
        // interval alone it would have been 25% and marked uncertain.
        let (_, overlap, uncertain) = align_transcript_segment(0.0, 4.0, &turns).unwrap();
        assert!((overlap - 2.0).abs() < 1e-5);
        assert!(!uncertain);
    }

    #[test]
    fn the_stored_span_is_the_union_of_that_speakers_intervals() {
        let turns = vec![
            turn("SPEAKER_00", 0.0, 1.0),
            turn("SPEAKER_01", 1.0, 2.0),
            turn("SPEAKER_00", 2.0, 4.0),
        ];
        assert_eq!(aligned_span(0.5, 3.5, "SPEAKER_00", &turns), Some((0.5, 3.5)));
        // clipped to the segment, and only intervals that actually overlap it
        assert_eq!(aligned_span(1.5, 2.5, "SPEAKER_00", &turns), Some((2.0, 2.5)));
        assert_eq!(aligned_span(0.0, 1.0, "SPEAKER_09", &turns), None);
    }

    #[test]
    fn the_aligned_span_matches_the_alignment_across_a_mid_sentence_change() {
        let turns = vec![
            turn("SPEAKER_00", 0.0, 1.2),
            turn("SPEAKER_00", 1.6, 2.4),
            turn("SPEAKER_01", 2.4, 3.0),
        ];
        let (chosen, overlap, _) = align_transcript_segment(0.0, 6.0, &turns).unwrap();
        assert_eq!(chosen.speaker, "SPEAKER_00");
        assert!((overlap - 2.0).abs() < 1e-5);
        assert_eq!(aligned_span(0.0, 6.0, &chosen.speaker, &turns), Some((0.0, 2.4)));
    }
}
