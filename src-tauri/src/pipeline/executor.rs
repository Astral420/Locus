//! Background worker that runs pipeline jobs.
//!
//! `PipelineOrchestrator` is the durable queue (what exists, what is allowed to
//! run). This executor is the missing half: a single worker thread that picks
//! runnable jobs, runs them through a [`StepRunner`], and records the result.
//!
//! The scheduling loop ([`drain`]) knows nothing about Tauri or Whisper, so it
//! is unit-tested with a fake runner.

use super::{
    orchestrator::{PipelineJob, PipelineMode, PipelineOrchestrator},
    steps::{self, StepOutcome},
};
use crate::AppState;
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};

/// How often the worker re-checks for work when nobody woke it explicitly
/// (e.g. a recording that was still finalizing, or a new recording ending).
const POLL_INTERVAL: Duration = Duration::from_secs(3);

/// Event emitted to the UI whenever a job changes state. Payload: the meeting id.
pub const PIPELINE_UPDATED_EVENT: &str = "pipeline-updated";

pub trait StepRunner {
    fn run(&self, job: &PipelineJob) -> StepOutcome;
    fn capture_active(&self) -> bool;
}

/// Runs queued work until nothing can make progress. Returns how many jobs
/// were executed. `on_change` is told the meeting id after every state change.
pub fn drain(
    orchestrator: &PipelineOrchestrator,
    runner: &dyn StepRunner,
    on_change: &dyn Fn(&str),
) -> usize {
    let mut executed = 0;
    loop {
        let Ok(meetings) = orchestrator.meetings_with_pending_work() else {
            return executed;
        };
        let mut progressed = false;
        for meeting_id in meetings {
            let mode = orchestrator
                .mode(&meeting_id)
                .unwrap_or(PipelineMode::Balanced);
            let Ok(runnable) = orchestrator.runnable(&meeting_id, mode, runner.capture_active())
            else {
                continue;
            };
            for job in runnable {
                if orchestrator.set_state(&job.id, "running", None).is_err() {
                    continue;
                }
                on_change(&meeting_id);
                let outcome = catch_unwind(AssertUnwindSafe(|| runner.run(&job)))
                    .unwrap_or_else(|panic| StepOutcome::Failed(panic_message(panic)));
                let still_pending = apply_outcome(orchestrator, &job, outcome);
                executed += 1;
                progressed |= !still_pending;
                on_change(&meeting_id);
            }
        }
        if !progressed {
            return executed;
        }
    }
}

/// Records the outcome. Returns true if the job went back to `pending`.
fn apply_outcome(
    orchestrator: &PipelineOrchestrator,
    job: &PipelineJob,
    outcome: StepOutcome,
) -> bool {
    let (state, reason): (&str, Option<String>) = match outcome {
        StepOutcome::Done(note) => ("done", note),
        StepOutcome::Skipped(reason) => ("skipped", Some(reason)),
        StepOutcome::Blocked(reason) => ("blocked", Some(reason)),
        StepOutcome::Failed(reason) => ("error", Some(reason)),
        StepOutcome::Canceled => ("canceled", Some("Canceled".into())),
        StepOutcome::Deferred => ("pending", None),
    };
    let _ = orchestrator.set_state(&job.id, state, reason.as_deref());
    state == "pending"
}

fn panic_message(panic: Box<dyn std::any::Any + Send>) -> String {
    let detail = panic
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown error".into());
    format!("The step crashed unexpectedly: {detail}")
}

struct AppRunner<'a> {
    state: &'a AppState,
}

impl StepRunner for AppRunner<'_> {
    fn run(&self, job: &PipelineJob) -> StepOutcome {
        steps::run_step(self.state, job)
    }
    fn capture_active(&self) -> bool {
        !matches!(
            self.state.capture.state().state,
            crate::contracts::CaptureLifecycle::Idle
        )
    }
}

pub struct PipelineExecutor {
    wake: Sender<()>,
    receiver: Mutex<Option<Receiver<()>>>,
    busy: Arc<AtomicBool>,
}

impl Default for PipelineExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl PipelineExecutor {
    pub fn new() -> Self {
        let (wake, receiver) = mpsc::channel();
        Self {
            wake,
            receiver: Mutex::new(Some(receiver)),
            busy: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Ask the worker to look for work now.
    pub fn notify(&self) {
        let _ = self.wake.send(());
    }

    /// True while a drain pass is running jobs (storage migration checks this).
    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    /// Spawns the worker. Safe to call once; later calls are no-ops.
    pub fn start(&self, app: AppHandle) {
        let Some(receiver) = self.receiver.lock().ok().and_then(|mut slot| slot.take()) else {
            return;
        };
        let busy = Arc::clone(&self.busy);
        let spawned = std::thread::Builder::new()
            .name("locus-pipeline".into())
            .spawn(move || worker_loop(app, receiver, busy));
        if let Err(error) = spawned {
            eprintln!("locus: could not start the pipeline worker: {error}");
        }
    }
}

fn worker_loop(app: AppHandle, receiver: Receiver<()>, busy: Arc<AtomicBool>) {
    let state = app.state::<AppState>();
    let _ = state.pipeline.reset_interrupted();
    loop {
        run_pass(&app, &state, &busy);
        match receiver.recv_timeout(POLL_INTERVAL) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {
                while receiver.try_recv().is_ok() {}
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn run_pass(app: &AppHandle, state: &AppState, busy: &AtomicBool) {
    busy.store(true, Ordering::SeqCst);
    let runner = AppRunner { state };
    let emit = |meeting_id: &str| {
        let _ = app.emit(PIPELINE_UPDATED_EVENT, meeting_id);
    };
    drain(&state.pipeline, &runner, &emit);
    busy.store(false, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use std::{cell::RefCell, collections::HashMap};

    struct FakeRunner {
        capture_active: bool,
        outcomes: HashMap<&'static str, StepOutcome>,
        ran: RefCell<Vec<String>>,
    }

    impl FakeRunner {
        fn new(outcomes: &[(&'static str, StepOutcome)]) -> Self {
            Self {
                capture_active: false,
                outcomes: outcomes.iter().cloned().collect(),
                ran: RefCell::new(Vec::new()),
            }
        }
    }

    impl StepRunner for FakeRunner {
        fn run(&self, job: &PipelineJob) -> StepOutcome {
            self.ran.borrow_mut().push(job.kind.clone());
            if job.kind == "ocr" && !self.outcomes.contains_key("ocr") {
                panic!("ocr exploded");
            }
            self.outcomes
                .get(job.kind.as_str())
                .cloned()
                .unwrap_or(StepOutcome::Done(None))
        }
        fn capture_active(&self) -> bool {
            self.capture_active
        }
    }

    fn setup(mode: PipelineMode) -> (PipelineOrchestrator, Database) {
        let db = Database::in_memory().unwrap();
        db.run(|c| {
            c.execute("INSERT INTO meetings(id,title,title_origin,recorded_at,timezone,lifecycle) VALUES ('m1','T','placeholder','2026-01-01','UTC','saved')", [])
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .unwrap();
        let orchestrator = PipelineOrchestrator::new(db.clone());
        orchestrator.enqueue("m1", mode).unwrap();
        (orchestrator, db)
    }

    fn states(orchestrator: &PipelineOrchestrator) -> HashMap<String, String> {
        orchestrator
            .jobs("m1")
            .unwrap()
            .into_iter()
            .map(|job| (job.kind, job.state))
            .collect()
    }

    #[test]
    fn a_full_pipeline_runs_in_dependency_order() {
        let (orchestrator, _) = setup(PipelineMode::Balanced);
        let runner = FakeRunner::new(&[]);
        let executed = drain(&orchestrator, &runner, &|_| {});
        assert_eq!(executed, 7);
        let ran = runner.ran.borrow();
        let position = |kind: &str| ran.iter().position(|k| k == kind).unwrap();
        assert!(position("encoding") < position("transcription"));
        assert!(position("transcription") < position("diarization"));
        assert!(position("slide_extraction") < position("ocr"));
        assert!(position("ocr") < position("summarization"));
        assert!(position("summarization") < position("indexing"));
        // the fake panics in `ocr`: that must become an error, not kill the loop
        let states = states(&orchestrator);
        assert_eq!(states["ocr"], "error");
        assert_eq!(states["summarization"], "done");
    }

    #[test]
    fn missing_optional_capabilities_do_not_stop_the_summary() {
        let (orchestrator, _) = setup(PipelineMode::Balanced);
        let runner = FakeRunner::new(&[
            ("diarization", StepOutcome::Skipped("no pyannote".into())),
            ("slide_extraction", StepOutcome::Skipped("no screen".into())),
            ("ocr", StepOutcome::Skipped("no slides".into())),
        ]);
        drain(&orchestrator, &runner, &|_| {});
        let states = states(&orchestrator);
        assert_eq!(states["summarization"], "done");
        assert_eq!(states["indexing"], "done");
        assert_eq!(states["diarization"], "skipped");
    }

    #[test]
    fn a_blocked_summary_holds_back_indexing_until_retried() {
        let (orchestrator, _) = setup(PipelineMode::Balanced);
        let blocked = FakeRunner::new(&[
            ("ocr", StepOutcome::Skipped("x".into())),
            ("summarization", StepOutcome::Blocked("select a provider".into())),
        ]);
        drain(&orchestrator, &blocked, &|_| {});
        let first = states(&orchestrator);
        assert_eq!(first["summarization"], "blocked");
        assert_eq!(first["indexing"], "pending");

        let job = orchestrator
            .jobs("m1")
            .unwrap()
            .into_iter()
            .find(|j| j.kind == "summarization")
            .unwrap();
        orchestrator.retry(&job.id).unwrap();
        let fixed = FakeRunner::new(&[("ocr", StepOutcome::Skipped("x".into()))]);
        drain(&orchestrator, &fixed, &|_| {});
        let second = states(&orchestrator);
        assert_eq!(second["summarization"], "done");
        assert_eq!(second["indexing"], "done");
    }

    #[test]
    fn a_failed_transcription_stops_dependents_and_reports_the_reason() {
        let (orchestrator, _) = setup(PipelineMode::Balanced);
        let runner = FakeRunner::new(&[(
            "transcription",
            StepOutcome::Failed("model failed to load".into()),
        )]);
        drain(&orchestrator, &runner, &|_| {});
        let states = states(&orchestrator);
        assert_eq!(states["transcription"], "error");
        assert_eq!(states["diarization"], "pending");
        assert_eq!(states["summarization"], "pending");
        assert!(!runner.ran.borrow().contains(&"summarization".to_string()));
    }

    #[test]
    fn a_deferred_step_stays_pending_and_does_not_spin() {
        let (orchestrator, _) = setup(PipelineMode::Balanced);
        let runner = FakeRunner::new(&[("encoding", StepOutcome::Deferred)]);
        let executed = drain(&orchestrator, &runner, &|_| {});
        assert_eq!(executed, 1);
        assert_eq!(states(&orchestrator)["encoding"], "pending");
    }

    #[test]
    fn active_capture_only_allows_encoding() {
        let (orchestrator, _) = setup(PipelineMode::Balanced);
        let mut runner = FakeRunner::new(&[("encoding", StepOutcome::Deferred)]);
        runner.capture_active = true;
        drain(&orchestrator, &runner, &|_| {});
        assert_eq!(*runner.ran.borrow(), vec!["encoding".to_string()]);
        let states = states(&orchestrator);
        assert_eq!(states["transcription"], "pending");
    }

    #[test]
    fn change_notifications_name_the_meeting() {
        let (orchestrator, _) = setup(PipelineMode::Balanced);
        let runner = FakeRunner::new(&[("encoding", StepOutcome::Deferred)]);
        let seen = RefCell::new(Vec::new());
        drain(&orchestrator, &runner, &|id| seen.borrow_mut().push(id.to_owned()));
        assert!(seen.borrow().iter().all(|id| id == "m1") && !seen.borrow().is_empty());
    }
}
