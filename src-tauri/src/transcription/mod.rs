//! Post-recording transcription pipeline primitives.

mod chunking;
mod engine;
mod models;

pub use chunking::{chunk_speech_regions, SpeechRegion, TranscriptionChunk, MAX_CHUNK_SECONDS};
pub use engine::{
    detect_backend, merge_transcript_segments, parse_backend, parse_macos_amd_discrete,
    vulkan_environment, vulkan_runtime_ready, BackendKind, TranscriptSegment, TranscriptionError,
    WhisperEngine,
};
pub use models::{ModelAsset, ModelCatalogEntry, ModelManager, ModelOrigin, ModelVariant};

use crate::db::Database;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionStatus {
    pub meeting_id: String,
    pub state: String,
    pub progress: f32,
    pub language: Option<String>,
    pub error: Option<String>,
}

/// Small orchestration seam used by Tauri commands and later durable job workers.
pub struct TranscriptionService {
    database: Database,
}

impl TranscriptionService {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub fn status(&self, meeting_id: &str) -> Result<TranscriptionStatus, String> {
        let meeting_id = meeting_id.to_owned();
        self.database
            .run(move |connection| {
                connection
                    .query_row(
                        "SELECT language_selected, language_detected FROM meetings WHERE id = ?1",
                        [&meeting_id],
                        |row| {
                            let selected: Option<String> = row.get(0)?;
                            let detected: Option<String> = row.get(1)?;
                            Ok(TranscriptionStatus {
                                meeting_id: meeting_id.clone(),
                                state: if selected.is_some() || detected.is_some() {
                                    "ready".into()
                                } else {
                                    "pending".into()
                                },
                                progress: if selected.is_some() || detected.is_some() {
                                    1.0
                                } else {
                                    0.0
                                },
                                language: selected.or(detected),
                                error: None,
                            })
                        },
                    )
                    .map_err(|e| e.to_string())
            })
            .map_err(|e| e.to_string())
    }

    pub fn set_language(&self, meeting_id: &str, language: Option<&str>) -> Result<(), String> {
        let meeting_id = meeting_id.to_owned();
        let language = language.map(str::to_owned);
        self.database
            .run(move |connection| {
                connection
                    .execute(
                        "UPDATE meetings SET language_selected = ?1 WHERE id = ?2",
                        (&language, &meeting_id),
                    )
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })
            .map_err(|e| e.to_string())
    }

    pub fn rename_speaker(&self, speaker_id: &str, display_name: &str) -> Result<(), String> {
        let speaker_id = speaker_id.to_owned();
        let display_name = display_name.trim().to_owned();
        if display_name.is_empty() {
            return Err("speaker name cannot be empty".into());
        }
        self.database
            .run(move |connection| {
                connection
                    .execute(
                        "UPDATE speakers SET display_name = ?1 WHERE id = ?2",
                        (&display_name, &speaker_id),
                    )
                    .map_err(|e| e.to_string())
                    .and_then(|changed| {
                        if changed == 0 {
                            Err("speaker not found".into())
                        } else {
                            Ok(())
                        }
                    })
            })
            .map_err(|e| e.to_string())
    }

    pub fn persist_segments(
        &self,
        meeting_id: &str,
        model_identity: &str,
        language: Option<&str>,
        segments: &[TranscriptSegment],
    ) -> Result<String, String> {
        let meeting_id = meeting_id.to_owned();
        let model_identity = model_identity.to_owned();
        let language = language.map(str::to_owned);
        let segments = segments.to_vec();
        self.database
            .run(move |connection| {
                let revision: i64 = connection
                    .query_row(
                        "SELECT COALESCE(MAX(revision), 0) + 1 FROM artifact_revisions WHERE meeting_id = ?1 AND kind = 'transcript'",
                        [&meeting_id],
                        |row| row.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                let revision_id = Uuid::new_v4().to_string();
                let tx = connection.transaction().map_err(|e| e.to_string())?;
                tx.execute(
                    "INSERT INTO artifact_revisions(id, meeting_id, kind, revision, model_identity, freshness, published_at) VALUES (?1, ?2, 'transcript', ?3, ?4, 'current', ?5)",
                    (&revision_id, &meeting_id, revision, &model_identity, Utc::now().to_rfc3339()),
                ).map_err(|e| e.to_string())?;
                for (ordinal, segment) in segments.iter().enumerate() {
                    tx.execute(
                        "INSERT INTO transcript_segments(id, transcript_revision_id, meeting_id, ordinal, start_seconds, end_seconds, text, language, confidence, source_provenance) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        (Uuid::new_v4().to_string(), &revision_id, &meeting_id, ordinal as i64, segment.start_seconds, segment.end_seconds, &segment.text, &language, segment.confidence, "whisper-rs"),
                    ).map_err(|e| e.to_string())?;
                }
                tx.execute("UPDATE meetings SET current_transcript_revision_id = ?1, language_detected = COALESCE(?2, language_detected) WHERE id = ?3", (&revision_id, &language, &meeting_id)).map_err(|e| e.to_string())?;
                tx.commit().map_err(|e| e.to_string())?;
                Ok(revision_id)
            })
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_override_is_persisted_without_creating_a_transcript() {
        let db = Database::in_memory().unwrap();
        let service = TranscriptionService::new(db.clone());
        db.run(|connection| connection.execute("INSERT INTO meetings(id,title,title_origin,recorded_at,timezone) VALUES ('m','Test','placeholder','now','UTC')", []).map(|_| ()).map_err(|e| e.to_string())).unwrap();
        service.set_language("m", Some("en")).unwrap();
        let language: String = db
            .run(|connection| {
                connection
                    .query_row(
                        "SELECT language_selected FROM meetings WHERE id='m'",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|e| e.to_string())
            })
            .unwrap();
        assert_eq!(language, "en");
    }
}
