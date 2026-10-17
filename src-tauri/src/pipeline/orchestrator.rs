//! Durable pipeline scheduling seam used by capture completion and retries.

use crate::db::Database;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PipelineMode {
    #[default]
    Balanced,
    Maximum,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PipelineStepSpec {
    pub kind: &'static str,
    pub dependencies: &'static [&'static str],
    pub resource_class: &'static str,
    pub optional: bool,
}

pub const PIPELINE_STEPS: &[PipelineStepSpec] = &[
    PipelineStepSpec {
        kind: "encoding",
        dependencies: &[],
        resource_class: "capture",
        optional: false,
    },
    PipelineStepSpec {
        kind: "transcription",
        dependencies: &["encoding"],
        resource_class: "inference",
        optional: false,
    },
    PipelineStepSpec {
        kind: "diarization",
        dependencies: &["transcription"],
        resource_class: "inference",
        optional: true,
    },
    PipelineStepSpec {
        kind: "slide_extraction",
        dependencies: &["encoding"],
        resource_class: "inference",
        optional: true,
    },
    PipelineStepSpec {
        kind: "ocr",
        dependencies: &["slide_extraction"],
        resource_class: "inference",
        optional: true,
    },
    PipelineStepSpec {
        kind: "summarization",
        dependencies: &["transcription", "diarization", "ocr"],
        resource_class: "inference",
        optional: false,
    },
    PipelineStepSpec {
        kind: "indexing",
        dependencies: &["transcription", "summarization"],
        resource_class: "background",
        optional: true,
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineJob {
    pub id: String,
    pub meeting_id: String,
    pub kind: String,
    pub state: String,
    pub cancellation_generation: i64,
}

/// Every step that depends, directly or through other steps, on `kind`.
pub fn downstream_kinds(kind: &str) -> Vec<&'static str> {
    let mut found: Vec<&'static str> = Vec::new();
    let mut frontier = vec![kind.to_owned()];
    while let Some(current) = frontier.pop() {
        for spec in PIPELINE_STEPS {
            if spec.dependencies.iter().any(|dependency| *dependency == current)
                && !found.contains(&spec.kind)
            {
                found.push(spec.kind);
                frontier.push(spec.kind.to_owned());
            }
        }
    }
    found
}

#[derive(Clone)]
pub struct PipelineOrchestrator {
    database: Database,
}

impl PipelineOrchestrator {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub fn enqueue(
        &self,
        meeting_id: &str,
        mode: PipelineMode,
    ) -> Result<Vec<PipelineJob>, String> {
        let meeting_id = meeting_id.to_owned();
        let meeting_id_for_insert = meeting_id.clone();
        let config = serde_json::json!({ "mode": mode, "steps": PIPELINE_STEPS }).to_string();
        self.database.run(move |connection| {
            for spec in PIPELINE_STEPS {
                let id = format!("pipeline:{meeting_id_for_insert}:{}", spec.kind);
                connection.execute(
                    "INSERT OR IGNORE INTO jobs(id, meeting_id, kind, state, config_snapshot, resource_class, created_at, updated_at) VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?6, ?6)",
                    (&id, &meeting_id_for_insert, spec.kind, &config, spec.resource_class, Utc::now().to_rfc3339()),
                ).map_err(|e| e.to_string())?;
            }
            Ok(())
        }).map_err(|error| error.to_string())?;
        self.jobs(&meeting_id)
    }

    pub fn jobs(&self, meeting_id: &str) -> Result<Vec<PipelineJob>, String> {
        let meeting_id = meeting_id.to_owned();
        self.database.run(move |connection| {
            let mut statement = connection.prepare("SELECT id, meeting_id, kind, state, cancellation_generation FROM jobs WHERE meeting_id=?1 AND id LIKE 'pipeline:%' ORDER BY created_at, kind").map_err(|e| e.to_string())?;
            let jobs = statement.query_map([&meeting_id], |row| Ok(PipelineJob { id: row.get(0)?, meeting_id: row.get(1)?, kind: row.get(2)?, state: row.get(3)?, cancellation_generation: row.get(4)? }))
                .map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
                ;
            jobs
        }).map_err(|e| e.to_string())
    }

    /// Returns work that may start now.  Balanced mode never starts inference
    /// or indexing while capture is active; Maximum still defers capture-bound
    /// work and lets independent inference run after capture is idle.
    pub fn runnable(
        &self,
        meeting_id: &str,
        mode: PipelineMode,
        capture_active: bool,
    ) -> Result<Vec<PipelineJob>, String> {
        let jobs = self.jobs(meeting_id)?;
        if capture_active {
            return Ok(jobs
                .into_iter()
                .filter(|job| job.kind == "encoding" && job.state == "pending")
                .collect());
        }
        // A dependency is satisfied when it is done, or when it is an optional
        // step that ended without producing output (skipped, failed or
        // canceled). Otherwise one missing optional capability (no slides, no
        // diarization model, no text recognition) would block the summary forever.
        let satisfied: HashSet<String> = jobs
            .iter()
            .filter(|job| {
                job.state == "done"
                    || (matches!(job.state.as_str(), "skipped" | "error" | "canceled")
                        && PIPELINE_STEPS
                            .iter()
                            .any(|spec| spec.kind == job.kind && spec.optional))
            })
            .map(|job| job.kind.clone())
            .collect();
        let mut runnable = Vec::new();
        for job in jobs.into_iter().filter(|job| job.state == "pending") {
            let Some(spec) = PIPELINE_STEPS.iter().find(|spec| spec.kind == job.kind) else {
                continue;
            };
            if spec
                .dependencies
                .iter()
                .all(|dependency| satisfied.contains(*dependency))
            {
                runnable.push(job);
                if mode == PipelineMode::Balanced && !runnable.is_empty() {
                    break;
                }
            }
        }
        Ok(runnable)
    }

    pub fn set_state(&self, job_id: &str, state: &str, reason: Option<&str>) -> Result<(), String> {
        if !matches!(
            state,
            "pending" | "running" | "done" | "error" | "blocked" | "skipped" | "canceled"
        ) {
            return Err(format!("invalid pipeline state: {state}"));
        }
        let job_id = job_id.to_owned();
        let state = state.to_owned();
        let reason = reason.map(str::to_owned);
        self.database.run(move |connection| {
            connection.execute("UPDATE jobs SET state=?1, reason=?2, progress=?3, updated_at=?4 WHERE id=?5", (&state, reason, if state == "done" { 1.0 } else { 0.0 }, Utc::now().to_rfc3339(), &job_id)).map(|_| ()).map_err(|e| e.to_string())
        }).map_err(|e| e.to_string())
    }

    /// The mode stored when the pipeline was enqueued (defaults to Balanced).
    pub fn mode(&self, meeting_id: &str) -> Result<PipelineMode, String> {
        let meeting_id = meeting_id.to_owned();
        let config: Option<String> = self
            .database
            .run(move |connection| {
                Ok(connection
                    .query_row(
                        "SELECT config_snapshot FROM jobs WHERE meeting_id=?1 AND id LIKE 'pipeline:%' LIMIT 1",
                        [&meeting_id],
                        |row| row.get::<_, String>(0),
                    )
                    .ok())
            })
            .map_err(|e| e.to_string())?;
        Ok(config
            .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
            .and_then(|value| serde_json::from_value(value.get("mode")?.clone()).ok())
            .unwrap_or_default())
    }

    /// Records fractional progress for a running job (0.0..=1.0).
    pub fn set_progress(&self, job_id: &str, progress: f32) -> Result<(), String> {
        let job_id = job_id.to_owned();
        let progress = f64::from(progress.clamp(0.0, 1.0));
        self.database
            .run(move |connection| {
                connection
                    .execute(
                        "UPDATE jobs SET progress=?1, updated_at=?2 WHERE id=?3 AND state='running'",
                        (progress, Utc::now().to_rfc3339(), &job_id),
                    )
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })
            .map_err(|e| e.to_string())
    }

    /// Jobs left `running` by a process that exited are interrupted work, not
    /// work in progress; they go back to `pending` so they resume on start.
    pub fn reset_interrupted(&self) -> Result<usize, String> {
        self.database
            .run(|connection| {
                connection
                    .execute(
                        "UPDATE jobs SET state='pending', progress=0, reason='Interrupted; resumed after restart', updated_at=?1 WHERE state='running' AND id LIKE 'pipeline:%'",
                        [Utc::now().to_rfc3339()],
                    )
                    .map_err(|e| e.to_string())
            })
            .map_err(|e| e.to_string())
    }

    /// Meetings that have pipeline work left and are still alive. Meetings that
    /// were deleted have their remaining jobs canceled instead of run.
    pub fn meetings_with_pending_work(&self) -> Result<Vec<String>, String> {
        self.database
            .run(|connection| {
                connection
                    .execute(
                        "UPDATE jobs SET state='canceled', reason='Meeting deleted', updated_at=?1 WHERE id LIKE 'pipeline:%' AND state IN ('pending','running') AND meeting_id IN (SELECT id FROM meetings WHERE deleted_at IS NOT NULL OR lifecycle='deleted')",
                        [Utc::now().to_rfc3339()],
                    )
                    .map_err(|e| e.to_string())?;
                let mut statement = connection
                    .prepare(
                        "SELECT DISTINCT j.meeting_id FROM jobs j JOIN meetings m ON m.id=j.meeting_id WHERE j.id LIKE 'pipeline:%' AND j.state='pending' AND m.deleted_at IS NULL AND m.lifecycle='saved' ORDER BY m.recorded_at",
                    )
                    .map_err(|e| e.to_string())?;
                let ids = statement
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|e| e.to_string())?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| e.to_string());
                ids
            })
            .map_err(|e| e.to_string())
    }

    /// True while the job is still the running attempt this worker started and
    /// its meeting has not been deleted. Long steps poll this between chunks.
    pub fn is_current(&self, job: &PipelineJob) -> bool {
        let job_id = job.id.clone();
        let generation = job.cancellation_generation;
        self.database
            .run(move |connection| {
                connection
                    .query_row(
                        "SELECT COUNT(*) FROM jobs j JOIN meetings m ON m.id=j.meeting_id WHERE j.id=?1 AND j.state='running' AND j.cancellation_generation=?2 AND m.deleted_at IS NULL AND m.lifecycle<>'deleted'",
                        (&job_id, generation),
                        |row| row.get::<_, i64>(0),
                    )
                    .map_err(|e| e.to_string())
            })
            .map(|count| count > 0)
            .unwrap_or(false)
    }

    /// Re-runs a step. Besides failed/blocked/skipped steps this also accepts a
    /// finished one (re-transcribe with another language, say), and in every
    /// case the steps that depend on it are put back to pending so stale
    /// diarization, summaries and indexes are rebuilt from the new output.
    pub fn retry(&self, job_id: &str) -> Result<(), String> {
        let job_id = job_id.to_owned();
        self.database
            .run(move |connection| {
                let row: Option<(String, String, String)> = connection
                    .query_row(
                        "SELECT meeting_id, kind, state FROM jobs WHERE id=?1 AND id LIKE 'pipeline:%'",
                        [&job_id],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .ok();
                let Some((meeting_id, kind, state)) = row else {
                    return Ok(());
                };
                let allowed = matches!(state.as_str(), "error" | "canceled" | "blocked" | "skipped")
                    || (state == "done" && kind != "encoding");
                if !allowed {
                    return Ok(());
                }
                let now = Utc::now().to_rfc3339();
                let tx = connection.transaction().map_err(|e| e.to_string())?;
                for target in std::iter::once(kind.as_str()).chain(downstream_kinds(&kind)) {
                    tx.execute(
                        "UPDATE jobs SET state='pending', progress=0, reason=NULL, cancellation_generation=cancellation_generation+1, updated_at=?1 WHERE meeting_id=?2 AND kind=?3 AND id LIKE 'pipeline:%' AND state<>'running'",
                        (&now, &meeting_id, target),
                    )
                    .map_err(|e| e.to_string())?;
                }
                tx.commit().map_err(|e| e.to_string())
            })
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meeting(db: &Database) {
        db.run(|connection| connection.execute("INSERT INTO meetings(id,title,title_origin,recorded_at,timezone) VALUES ('m1','Test','placeholder','now','+00:00')", []).map(|_| ()).map_err(|e| e.to_string())).unwrap();
    }

    #[test]
    fn balanced_pipeline_is_durable_and_capture_has_priority() {
        let db = Database::in_memory().unwrap();
        meeting(&db);
        let orchestrator = PipelineOrchestrator::new(db);
        let jobs = orchestrator.enqueue("m1", PipelineMode::Balanced).unwrap();
        assert_eq!(jobs.len(), PIPELINE_STEPS.len());
        assert_eq!(
            orchestrator
                .enqueue("m1", PipelineMode::Balanced)
                .unwrap()
                .len(),
            PIPELINE_STEPS.len()
        );
        assert_eq!(orchestrator.jobs("m1").unwrap().len(), PIPELINE_STEPS.len());
        assert_eq!(
            orchestrator
                .runnable("m1", PipelineMode::Balanced, true)
                .unwrap()[0]
                .kind,
            "encoding"
        );
        orchestrator.set_state(&jobs[0].id, "done", None).unwrap();
        assert_eq!(
            orchestrator
                .runnable("m1", PipelineMode::Balanced, false)
                .unwrap()[0]
                .kind,
            "transcription"
        );
    }

    #[test]
    fn retry_advances_cancellation_generation() {
        let db = Database::in_memory().unwrap();
        meeting(&db);
        let orchestrator = PipelineOrchestrator::new(db);
        let job = orchestrator
            .enqueue("m1", PipelineMode::Balanced)
            .unwrap()
            .remove(0);
        orchestrator
            .set_state(&job.id, "error", Some("fixture failure"))
            .unwrap();
        orchestrator.retry(&job.id).unwrap();
        assert_eq!(orchestrator.jobs("m1").unwrap()[0].state, "pending");
    }

    fn finish_all(orchestrator: &PipelineOrchestrator, meeting: &str, kinds: &[(&str, &str)]) {
        for job in orchestrator.jobs(meeting).unwrap() {
            if let Some((_, state)) = kinds.iter().find(|(kind, _)| *kind == job.kind) {
                orchestrator.set_state(&job.id, state, None).unwrap();
            }
        }
    }

    #[test]
    fn skipped_or_failed_optional_steps_do_not_block_the_summary() {
        let db = Database::in_memory().unwrap();
        meeting(&db);
        let orchestrator = PipelineOrchestrator::new(db);
        orchestrator.enqueue("m1", PipelineMode::Maximum).unwrap();
        finish_all(
            &orchestrator,
            "m1",
            &[
                ("encoding", "done"),
                ("transcription", "done"),
                ("diarization", "error"),
                ("slide_extraction", "skipped"),
                ("ocr", "skipped"),
            ],
        );
        let runnable = orchestrator
            .runnable("m1", PipelineMode::Balanced, false)
            .unwrap();
        assert_eq!(runnable[0].kind, "summarization");
    }

    #[test]
    fn a_failed_required_step_blocks_its_dependents() {
        let db = Database::in_memory().unwrap();
        meeting(&db);
        let orchestrator = PipelineOrchestrator::new(db);
        orchestrator.enqueue("m1", PipelineMode::Maximum).unwrap();
        finish_all(
            &orchestrator,
            "m1",
            &[("encoding", "done"), ("transcription", "error")],
        );
        let kinds: Vec<String> = orchestrator
            .runnable("m1", PipelineMode::Maximum, false)
            .unwrap()
            .into_iter()
            .map(|job| job.kind)
            .collect();
        assert!(!kinds.contains(&"diarization".to_string()));
        assert!(!kinds.contains(&"summarization".to_string()));
        assert!(kinds.contains(&"slide_extraction".to_string()));
    }

    #[test]
    fn restart_resumes_interrupted_work_and_cancels_deleted_meetings() {
        let db = Database::in_memory().unwrap();
        meeting(&db);
        db.run(|c| {
            c.execute("UPDATE meetings SET lifecycle='saved' WHERE id='m1'", [])
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .unwrap();
        let orchestrator = PipelineOrchestrator::new(db.clone());
        let jobs = orchestrator.enqueue("m1", PipelineMode::Balanced).unwrap();
        orchestrator.set_state(&jobs[0].id, "running", None).unwrap();
        assert_eq!(orchestrator.reset_interrupted().unwrap(), 1);
        assert_eq!(orchestrator.jobs("m1").unwrap()[0].state, "pending");
        assert_eq!(
            orchestrator.meetings_with_pending_work().unwrap(),
            vec!["m1".to_string()]
        );
        db.run(|c| {
            c.execute("UPDATE meetings SET lifecycle='deleted', deleted_at='now' WHERE id='m1'", [])
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .unwrap();
        assert!(orchestrator.meetings_with_pending_work().unwrap().is_empty());
        assert!(orchestrator
            .jobs("m1")
            .unwrap()
            .iter()
            .all(|job| job.state == "canceled"));
    }

    #[test]
    fn is_current_tracks_the_running_attempt() {
        let db = Database::in_memory().unwrap();
        meeting(&db);
        let orchestrator = PipelineOrchestrator::new(db);
        let job = orchestrator
            .enqueue("m1", PipelineMode::Balanced)
            .unwrap()
            .remove(0);
        assert!(!orchestrator.is_current(&job)); // pending, not running
        orchestrator.set_state(&job.id, "running", None).unwrap();
        assert!(orchestrator.is_current(&job));
        orchestrator.set_progress(&job.id, 0.5).unwrap();
        orchestrator.set_state(&job.id, "error", Some("x")).unwrap();
        orchestrator.retry(&job.id).unwrap();
        orchestrator.set_state(&job.id, "running", None).unwrap();
        // the retry advanced the generation, so the stale handle is not current
        assert!(!orchestrator.is_current(&job));
    }

    #[test]
    fn downstream_of_transcription_is_everything_that_reads_it() {
        let mut found = downstream_kinds("transcription");
        found.sort_unstable();
        assert_eq!(found, vec!["diarization", "indexing", "summarization"]);
        let mut slides = downstream_kinds("slide_extraction");
        slides.sort_unstable();
        assert_eq!(slides, vec!["indexing", "ocr", "summarization"]);
        assert!(downstream_kinds("indexing").is_empty());
    }

    #[test]
    fn rerunning_a_finished_step_resets_what_depends_on_it() {
        let db = Database::in_memory().unwrap();
        meeting(&db);
        let orchestrator = PipelineOrchestrator::new(db);
        orchestrator.enqueue("m1", PipelineMode::Maximum).unwrap();
        let all: Vec<_> = orchestrator.jobs("m1").unwrap();
        for job in &all {
            orchestrator.set_state(&job.id, "done", None).unwrap();
        }
        let transcription = all.iter().find(|j| j.kind == "transcription").unwrap();
        orchestrator.retry(&transcription.id).unwrap();
        let states: std::collections::HashMap<_, _> = orchestrator
            .jobs("m1")
            .unwrap()
            .into_iter()
            .map(|j| (j.kind, j.state))
            .collect();
        for rerun in ["transcription", "diarization", "summarization", "indexing"] {
            assert_eq!(states[rerun], "pending", "{rerun}");
        }
        for untouched in ["encoding", "slide_extraction", "ocr"] {
            assert_eq!(states[untouched], "done", "{untouched}");
        }
    }

    #[test]
    fn encoding_cannot_be_rerun_and_running_jobs_are_left_alone() {
        let db = Database::in_memory().unwrap();
        meeting(&db);
        let orchestrator = PipelineOrchestrator::new(db);
        orchestrator.enqueue("m1", PipelineMode::Maximum).unwrap();
        let jobs = orchestrator.jobs("m1").unwrap();
        let encoding = jobs.iter().find(|j| j.kind == "encoding").unwrap();
        orchestrator.set_state(&encoding.id, "done", None).unwrap();
        orchestrator.retry(&encoding.id).unwrap();
        assert_eq!(
            orchestrator.jobs("m1").unwrap().iter().find(|j| j.kind == "encoding").unwrap().state,
            "done"
        );
        let transcription = jobs.iter().find(|j| j.kind == "transcription").unwrap();
        let summary = jobs.iter().find(|j| j.kind == "summarization").unwrap();
        orchestrator.set_state(&transcription.id, "done", None).unwrap();
        orchestrator.set_state(&summary.id, "running", None).unwrap();
        orchestrator.retry(&transcription.id).unwrap();
        let after: std::collections::HashMap<_, _> = orchestrator
            .jobs("m1").unwrap().into_iter().map(|j| (j.kind, j.state)).collect();
        assert_eq!(after["summarization"], "running");
    }
}
