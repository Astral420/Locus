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
    pub kind: String,
    pub state: String,
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
            let mut statement = connection.prepare("SELECT id, kind, state FROM jobs WHERE meeting_id=?1 ORDER BY created_at, kind").map_err(|e| e.to_string())?;
            let jobs = statement.query_map([&meeting_id], |row| Ok(PipelineJob { id: row.get(0)?, kind: row.get(1)?, state: row.get(2)? }))
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
        let done: HashSet<String> = jobs
            .iter()
            .filter(|job| job.state == "done")
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
                .all(|dependency| done.contains(*dependency))
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

    pub fn retry(&self, job_id: &str) -> Result<(), String> {
        self.database.run({
            let job_id = job_id.to_owned();
            move |connection| {
                connection.execute("UPDATE jobs SET state='pending', progress=0, reason=NULL, cancellation_generation=cancellation_generation+1, updated_at=?1 WHERE id=?2 AND state IN ('error','canceled','blocked')", (Utc::now().to_rfc3339(), &job_id)).map(|_| ()).map_err(|e| e.to_string())
            }
        }).map_err(|e| e.to_string())
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
}
