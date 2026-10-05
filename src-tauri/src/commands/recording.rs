use crate::{
    capture::{preflight, CaptureOptions},
    contracts::{CaptureLifecycle, CaptureSource, MeetingType, RecordingStateDto},
    AppState,
};
use tauri::State;

#[tauri::command]
pub fn show_recording_window(window: tauri::Window) -> Result<(), String> {
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

/// Capture commands run on the blocking pool: starting opens audio devices and
/// FFmpeg (and can sit behind an OS permission prompt) and stopping mixes and
/// muxes the recording. Doing that on the main thread froze the whole UI.
async fn blocking<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|e| e.to_string())?
}

/// Warms up screen capture (locates/loads FFmpeg, lists displays) while the
/// Record page is open, so pressing Record does not pay that cost. Best-effort.
#[tauri::command]
pub async fn prewarm_capture() -> Result<(), String> {
    blocking(|| {
        crate::capture::screen::prewarm();
        Ok(())
    })
    .await
}

/// Captures that were interrupted (crash, forced quit, power loss) and still
/// have recordable material on disk. The app only offers them; nothing is
/// restarted or rebuilt until `recover_capture` is called.
#[tauri::command]
pub async fn list_recoverable_captures(
    state: State<'_, AppState>,
) -> Result<Vec<crate::capture::recovery::RecoverableCapture>, String> {
    let root = state.media_root.clone();
    let database = state.database.clone();
    let active = state.capture.state().meeting_id;
    blocking(move || {
        Ok(crate::capture::recovery::list_recoverable(
            &root,
            active.as_deref(),
            Some(&database),
        ))
    })
    .await
}

/// Rebuilds the recording of an interrupted capture from what survived, then
/// saves it like a normal stop (including queueing processing).
#[tauri::command]
pub async fn recover_capture(
    state: State<'_, AppState>,
    meeting_id: String,
) -> Result<crate::capture::recovery::RecoveryOutcome, String> {
    let root = state.media_root.clone();
    let database = state.database.clone();
    let pipeline = state.pipeline.clone();
    blocking(move || {
        let ffmpeg = crate::capture::screen::locate_ffmpeg().ok_or_else(|| {
            "FFmpeg was not found, so the recording cannot be rebuilt.".to_string()
        })?;
        let outcome =
            crate::capture::recovery::recover_and_commit(&database, &root, &meeting_id, &ffmpeg)?;
        pipeline.enqueue(
            &meeting_id,
            crate::pipeline::orchestrator::PipelineMode::Balanced,
        )?;
        Ok(outcome)
    })
    .await
}

/// Displays and windows the native backend can record, for the source picker
/// (FR1.3). Asking triggers the macOS screen-recording permission prompt the
/// first time. Fails on platforms without a native backend yet.
#[tauri::command]
pub async fn list_screen_sources() -> Result<Vec<crate::capture::source::ScreenSourceInfo>, String>
{
    blocking(|| crate::capture::native_screen::list_sources().map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn start_recording(
    state: State<'_, AppState>,
    sources: Vec<CaptureSource>,
    meeting_type: MeetingType,
    title: Option<String>,
    single_person_mic: Option<bool>,
    screen_target: Option<crate::capture::source::ScreenTarget>,
) -> Result<RecordingStateDto, String> {
    let capture = state.capture.clone();
    let media_root = state.media_root.clone();
    blocking(move || {
        // Recordings are played back through the asset protocol, which needs absolute paths.
        std::fs::create_dir_all(&media_root).map_err(|e| e.to_string())?;
        let output_root = media_root.canonicalize().map_err(|e| e.to_string())?;
        let options = CaptureOptions {
            sources,
            meeting_type,
            single_person_mic: single_person_mic.unwrap_or(false),
            title: title.unwrap_or_default(),
            output_root,
            screen_target,
        };
        preflight::validate_storage(&options).map_err(|e| e.to_string())?;
        #[cfg(target_os = "linux")]
        crate::capture::linux::preflight(&options).map_err(|e| e.to_string())?;
        #[cfg(target_os = "macos")]
        crate::capture::macos::preflight(&options).map_err(|e| e.to_string())?;
        #[cfg(target_os = "windows")]
        crate::capture::windows::preflight(&options).map_err(|e| e.to_string())?;
        capture.start(options).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn pause_recording(state: State<'_, AppState>) -> Result<RecordingStateDto, String> {
    let capture = state.capture.clone();
    blocking(move || {
        capture
            .transition(CaptureLifecycle::Paused)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn resume_recording(state: State<'_, AppState>) -> Result<RecordingStateDto, String> {
    let capture = state.capture.clone();
    blocking(move || {
        capture
            .transition(CaptureLifecycle::Recording)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn stop_recording(state: State<'_, AppState>) -> Result<RecordingStateDto, String> {
    let capture = state.capture.clone();
    let pipeline = state.pipeline.clone();
    blocking(move || {
        capture
            .transition(CaptureLifecycle::Finalizing)
            .map_err(|e| e.to_string())?;
        let result = capture
            .transition(CaptureLifecycle::Saved)
            .map_err(|e| e.to_string())?;
        if let Some(meeting_id) = result.meeting_id.as_deref() {
            pipeline.enqueue(
                meeting_id,
                crate::pipeline::orchestrator::PipelineMode::Balanced,
            )?;
        }
        Ok(result)
    })
    .await
}
