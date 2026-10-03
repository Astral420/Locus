pub mod audio;
pub mod encoder;
pub mod linux;
pub mod macos;
pub mod preflight;
pub mod screen;
pub mod timeline;
pub mod windows;

use crate::{
    contracts::{CaptureLifecycle, CaptureSource, MeetingType, RecordingStateDto},
    db::Database,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CaptureError {
    #[error("a capture is already active")]
    AlreadyActive,
    #[error("invalid capture transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: CaptureLifecycle,
        to: CaptureLifecycle,
    },
    #[error("at least one audio source must be selected")]
    AudioSourceRequired,
    #[error("screen-only capture is not supported")]
    ScreenOnly,
    #[error("no active capture")]
    NoActiveCapture,
    #[error("capture source is unavailable: {0}")]
    SourceUnavailable(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureOptions {
    pub sources: Vec<CaptureSource>,
    pub meeting_type: MeetingType,
    pub single_person_mic: bool,
    pub title: String,
    pub output_root: PathBuf,
}

impl CaptureOptions {
    pub fn validate(&self) -> Result<(), CaptureError> {
        let has_audio = self.sources.iter().any(|source| {
            matches!(
                source,
                CaptureSource::SystemAudio | CaptureSource::Microphone
            )
        });
        if !has_audio {
            return Err(
                if self
                    .sources
                    .iter()
                    .any(|source| matches!(source, CaptureSource::Screen))
                {
                    CaptureError::ScreenOnly
                } else {
                    CaptureError::AudioSourceRequired
                },
            );
        }
        Ok(())
    }
}

struct Session {
    capture_id: String,
    meeting_id: String,
    generation: u64,
    state: CaptureLifecycle,
    sources: Vec<CaptureSource>,
    timeline: timeline::MediaTimeline,
    reason: Option<String>,
    audio: Option<audio::AudioCapture>,
    finished: Option<audio::FinishedAudio>,
    screen: Option<screen::ScreenCapture>,
    /// When the audio streams were really recording (t=0 of the audio file).
    audio_started_at: Option<std::time::Instant>,
    /// Set once the video+audio MP4 has been produced.
    muxed_video: bool,
}

#[derive(Clone)]
pub struct CaptureManager {
    active: Arc<Mutex<Option<Session>>>,
    #[allow(dead_code)]
    db: Option<Database>,
    /// When false (unit tests) no hardware is opened and levels read as silence.
    audio_enabled: bool,
}

impl CaptureManager {
    pub fn new(db: Option<Database>) -> Self {
        Self {
            active: Arc::new(Mutex::new(None)),
            db,
            audio_enabled: false,
        }
    }

    /// Production constructor: opens real audio devices while recording.
    pub fn with_audio(db: Option<Database>) -> Self {
        Self {
            audio_enabled: true,
            ..Self::new(db)
        }
    }

    pub fn start(&self, options: CaptureOptions) -> Result<RecordingStateDto, CaptureError> {
        options.validate()?;
        let mut active = self.active.lock().expect("capture mutex poisoned");
        if active.is_some() {
            return Err(CaptureError::AlreadyActive);
        }
        let now = Utc::now();
        let selected_sources = options.sources.clone();
        let session = Session {
            capture_id: Uuid::new_v4().to_string(),
            meeting_id: Uuid::new_v4().to_string(),
            generation: 0,
            state: CaptureLifecycle::Preparing,
            sources: selected_sources,
            timeline: timeline::MediaTimeline::start(),
            reason: None,
            audio: None,
            finished: None,
            screen: None,
            audio_started_at: None,
            muxed_video: false,
        };
        let mut session = session;
        if self.audio_enabled {
            let media_dir = options.output_root.join(&session.meeting_id);
            std::fs::create_dir_all(&media_dir).map_err(|e| {
                CaptureError::SourceUnavailable(format!("cannot create media folder: {e}"))
            })?;
            // Screen (FFmpeg, ~1s to the first frame) and the audio devices open
            // at the same time instead of one after the other. Each reports the
            // instant it really began, and the mux aligns the streams to that.
            let screen_job = options.sources.contains(&CaptureSource::Screen).then(|| {
                let dir = media_dir.clone();
                std::thread::spawn(move || screen::ScreenCapture::start(&dir))
            });
            let audio_result = audio::AudioCapture::start(&media_dir, &options.sources);
            let screen_result = screen_job.map(|job| {
                job.join().unwrap_or_else(|_| {
                    Err(CaptureError::SourceUnavailable(
                        "Screen: capture thread crashed".into(),
                    ))
                })
            });
            // Screen is a selected source: if either side cannot be captured,
            // fail the whole start instead of showing "recording" with gaps.
            match (audio_result, screen_result) {
                (Ok(audio), None) => {
                    session.audio_started_at = audio.started_at();
                    session.audio = Some(audio);
                }
                (Ok(audio), Some(Ok(screen))) => {
                    session.audio_started_at = audio.started_at();
                    session.audio = Some(audio);
                    session.screen = Some(screen);
                }
                (audio, screen) => {
                    let mut failure = None;
                    match audio {
                        Ok(audio) => audio.discard(),
                        Err(error) => failure = Some(error),
                    }
                    match screen {
                        Some(Ok(screen)) => screen.abort(),
                        Some(Err(error)) => failure = failure.or(Some(error)),
                        None => {}
                    }
                    let _ = std::fs::remove_dir_all(&media_dir);
                    return Err(failure.expect("a failed start carries an error"));
                }
            }
            // The on-screen timer starts when capture really has, not when the
            // button was pressed (device start-up used to be counted in it).
            session.timeline = timeline::MediaTimeline::start();
        }
        if let Some(database) = &self.db {
            let meeting_id = session.meeting_id.clone();
            let title = if options.title.trim().is_empty() {
                "Untitled Conversation".to_owned()
            } else {
                options.title.trim().to_owned()
            };
            let requested_type = match &options.meeting_type {
                MeetingType::Meeting => "meeting",
                MeetingType::Lecture => "lecture",
                MeetingType::Auto => "auto",
            };
            let sources = serde_json::to_string(&session.sources).unwrap_or_else(|_| "[]".into());
            let inserted = database
                .run(move |connection| {
                    connection
                        .execute(
                            "INSERT INTO meetings(id, title, title_origin, recorded_at, timezone, requested_type, capture_sources, single_person_mic, lifecycle) VALUES (?1, ?2, 'placeholder', ?3, ?4, ?5, ?6, ?7, 'recording')",
                            (
                                &meeting_id,
                                &title,
                                Utc::now().to_rfc3339(),
                                chrono::Local::now().format("%:z").to_string(),
                                requested_type,
                                &sources,
                                options.single_person_mic as i64,
                            ),
                        )
                        .map_err(|error| error.to_string())?;
                    connection
                        .execute(
                            "INSERT INTO capture_manifests(id, meeting_id, capture_generation, state, manifest_path, media_clock_origin_ns) VALUES (?1, ?2, 0, 'recording', ?3, ?4)",
                            (
                                format!("manifest:{meeting_id}"),
                                &meeting_id,
                                format!("{meeting_id}/capture-manifest.json"),
                                0_i64,
                            ),
                        )
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                })
                .map_err(|error| CaptureError::SourceUnavailable(error.to_string()));
            if let Err(error) = inserted {
                if let Some(screen) = session.screen.take() {
                    screen.abort();
                }
                if let Some(audio) = session.audio.take() {
                    audio.discard();
                }
                return Err(error);
            }
        }
        session.state = CaptureLifecycle::Recording;
        let state = to_dto(&session);
        let _ = (
            now,
            options.meeting_type,
            options.single_person_mic,
            options.output_root,
        );
        *active = Some(session);
        Ok(state)
    }

    pub fn transition(&self, target: CaptureLifecycle) -> Result<RecordingStateDto, CaptureError> {
        let mut active = self.active.lock().expect("capture mutex poisoned");
        let session = active.as_mut().ok_or(CaptureError::NoActiveCapture)?;
        let current = session.state.clone();
        if current == target {
            return Ok(to_dto(session));
        }
        let valid = matches!(
            (&current, &target),
            (
                CaptureLifecycle::Recording,
                CaptureLifecycle::Paused
                    | CaptureLifecycle::Finalizing
                    | CaptureLifecycle::Interrupted
            ) | (
                CaptureLifecycle::Paused,
                CaptureLifecycle::Recording
                    | CaptureLifecycle::Finalizing
                    | CaptureLifecycle::Interrupted
            ) | (
                CaptureLifecycle::Finalizing,
                CaptureLifecycle::Saved | CaptureLifecycle::Recoverable
            )
        );
        if !valid {
            return Err(CaptureError::InvalidTransition {
                from: current,
                to: target,
            });
        }
        if target == CaptureLifecycle::Paused {
            session.timeline.pause();
            // Stop video and audio at the same instant, then wait for the
            // video file to flush; otherwise one runs on and A/V drifts.
            if let Some(screen) = session.screen.as_mut() {
                screen.request_stop();
            }
            if let Some(audio) = &session.audio {
                audio.set_paused(true);
            }
            if let Some(screen) = session.screen.as_mut() {
                screen.complete_stop();
            }
        }
        if target == CaptureLifecycle::Recording {
            // Restart video first: if it cannot resume, stay paused and report.
            if let Some(screen) = session.screen.as_mut() {
                if let Err(error) = screen.resume() {
                    session.reason = Some(error.to_string());
                    return Err(error);
                }
            }
            session.timeline.resume();
            if let Some(audio) = &session.audio {
                audio.set_paused(false);
            }
        }
        if target == CaptureLifecycle::Finalizing {
            let mut screen = session.screen.take();
            // Video stops at the same moment the audio streams are closed.
            if let Some(screen) = screen.as_mut() {
                screen.request_stop();
            }
            if let Some(audio) = session.audio.take() {
                match audio.finish() {
                    Ok(finished) => session.finished = Some(finished),
                    Err(error) => {
                        if let Some(screen) = screen {
                            screen.abort();
                        }
                        session.reason = Some(error.to_string());
                        session.state = CaptureLifecycle::Recoverable;
                        return Err(error);
                    }
                }
            }
            if let Some(screen) = screen {
                let video = screen.finish();
                if let Some(finished) = &session.finished {
                    let output = finished.mixed_path.with_file_name(screen::VIDEO_FILE_NAME);
                    // Seconds the audio began after the first video frame
                    // (negative: audio was already recording before frame 0).
                    let offset = match session.audio_started_at {
                        Some(audio_at) if audio_at >= video.video_started_at => audio_at
                            .duration_since(video.video_started_at)
                            .as_secs_f64(),
                        Some(audio_at) => -video
                            .video_started_at
                            .duration_since(audio_at)
                            .as_secs_f64(),
                        None => 0.0,
                    };
                    let result = match screen::locate_ffmpeg() {
                        Some(ffmpeg) => screen::mux_recording(
                            &ffmpeg,
                            &video.segments,
                            Some((finished.mixed_path.as_path(), offset)),
                            &output,
                        ),
                        None => Err("FFmpeg disappeared before the recording was finalised".into()),
                    };
                    // Sync report next to the recording (cheap, and the first thing
                    // to look at if A/V ever drifts).
                    if let Some(ffmpeg) = screen::locate_ffmpeg() {
                        let streams = if result.is_ok() {
                            screen::describe_streams(&ffmpeg, &output)
                        } else {
                            String::new()
                        };
                        let report = format!(
                            "audio_offset_secs={offset:.3}\nmux={}\n{streams}\n",
                            if result.is_ok() { "ok" } else { "failed" }
                        );
                        let _ = std::fs::write(output.with_file_name("sync-report.txt"), report);
                    }
                    match result {
                        Ok(()) => session.muxed_video = true,
                        // Never lose the take: audio.wav stays playable and the
                        // raw video segments stay on disk for recovery.
                        Err(error) => {
                            session.reason = Some(format!(
                                "Video could not be finalised ({error}); audio was saved."
                            ))
                        }
                    }
                }
            }
        }
        if matches!(
            target,
            CaptureLifecycle::Saved | CaptureLifecycle::Recoverable
        ) {
            session.generation += 1;
        }
        session.state = target;
        let state = to_dto(session);
        if let Some(database) = &self.db {
            let meeting_id = session.meeting_id.clone();
            let lifecycle = match session.state {
                CaptureLifecycle::Finalizing => "finalizing",
                CaptureLifecycle::Saved => "saved",
                CaptureLifecycle::Recoverable | CaptureLifecycle::Interrupted => "interrupted",
                CaptureLifecycle::Paused => "paused",
                _ => "recording",
            };
            let duration = state.elapsed_seconds;
            let _ = database.run(move |connection| {
                connection
                    .execute(
                        "UPDATE meetings SET lifecycle=?1, duration_seconds=?2 WHERE id=?3",
                        (lifecycle, duration, &meeting_id),
                    )
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            });
        }
        if matches!(session.state, CaptureLifecycle::Saved) {
            if let (Some(database), Some(finished)) = (&self.db, session.finished.clone()) {
                let meeting_id = session.meeting_id.clone();
                let file = if session.muxed_video {
                    screen::VIDEO_FILE_NAME
                } else {
                    audio::MIXED_FILE_NAME
                };
                let relative_path = format!("{meeting_id}/{file}");
                let duration = finished.duration_seconds;
                let _ = database.run(move |connection| {
                    connection
                        .execute(
                            "INSERT OR REPLACE INTO media_segments(id, manifest_id, stream_id, source, ordinal, relative_path, start_seconds, duration_seconds, committed_at) VALUES (?1, ?2, 'mixed', 'mixed', 0, ?3, 0, ?4, ?5)",
                            (
                                format!("segment:{meeting_id}:0"),
                                format!("manifest:{meeting_id}"),
                                &relative_path,
                                duration,
                                Utc::now().to_rfc3339(),
                            ),
                        )
                        .map_err(|error| error.to_string())?;
                    connection
                        .execute(
                            "UPDATE capture_manifests SET state='saved' WHERE id=?1",
                            [format!("manifest:{meeting_id}")],
                        )
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                });
            }
            *active = None;
        }
        Ok(state)
    }

    pub fn mark_interrupted(
        &self,
        reason: impl Into<String>,
    ) -> Result<RecordingStateDto, CaptureError> {
        let mut active = self.active.lock().expect("capture mutex poisoned");
        let session = active.as_mut().ok_or(CaptureError::NoActiveCapture)?;
        session.reason = Some(reason.into());
        session.state = CaptureLifecycle::Recoverable;
        Ok(to_dto(session))
    }

    pub fn state(&self) -> RecordingStateDto {
        self.active
            .lock()
            .expect("capture mutex poisoned")
            .as_ref()
            .map(to_dto)
            .unwrap_or_else(|| RecordingStateDto {
                capture_id: None,
                meeting_id: None,
                state: CaptureLifecycle::Idle,
                generation: 0,
                elapsed_seconds: 0.0,
                selected_sources: vec![],
                recoverable: false,
                reason: None,
                warning: false,
                system_audio_level: None,
                mic_level: None,
            })
    }
}

/// Real dBFS reading for a selected source; silence when no audio is flowing.
fn live_level(session: &Session, source: &CaptureSource) -> Option<f32> {
    if !session.sources.contains(source) {
        return None;
    }
    Some(
        session
            .audio
            .as_ref()
            .and_then(|audio| audio.level(source))
            .unwrap_or(audio::SILENCE_DBFS),
    )
}

fn to_dto(session: &Session) -> RecordingStateDto {
    let elapsed = session.timeline.elapsed().as_secs_f64();
    RecordingStateDto {
        capture_id: Some(session.capture_id.clone()),
        meeting_id: Some(session.meeting_id.clone()),
        state: session.state.clone(),
        generation: session.generation,
        elapsed_seconds: elapsed,
        selected_sources: session.sources.clone(),
        recoverable: matches!(
            session.state,
            CaptureLifecycle::Recoverable | CaptureLifecycle::Interrupted
        ),
        reason: session.reason.clone(),
        warning: session.timeline.warning_due(),
        system_audio_level: live_level(session, &CaptureSource::SystemAudio),
        mic_level: live_level(session, &CaptureSource::Microphone),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options(sources: Vec<CaptureSource>) -> CaptureOptions {
        CaptureOptions {
            sources,
            meeting_type: MeetingType::Auto,
            single_person_mic: false,
            output_root: PathBuf::from("media"),
            title: "Test capture".into(),
        }
    }

    #[test]
    fn validates_audio_and_single_active_capture() {
        assert_eq!(
            options(vec![CaptureSource::Screen]).validate(),
            Err(CaptureError::ScreenOnly)
        );
        let manager = CaptureManager::new(None);
        manager
            .start(options(vec![CaptureSource::Microphone]))
            .unwrap();
        assert_eq!(
            manager
                .start(options(vec![CaptureSource::Microphone]))
                .unwrap_err(),
            CaptureError::AlreadyActive
        );
    }

    #[test]
    fn transitions_are_idempotent_and_invalid_edges_are_rejected() {
        let manager = CaptureManager::new(None);
        manager
            .start(options(vec![CaptureSource::SystemAudio]))
            .unwrap();
        manager.transition(CaptureLifecycle::Paused).unwrap();
        manager.transition(CaptureLifecycle::Paused).unwrap();
        manager.transition(CaptureLifecycle::Recording).unwrap();
        assert!(matches!(
            manager.transition(CaptureLifecycle::Saved),
            Err(CaptureError::InvalidTransition { .. })
        ));
        manager.transition(CaptureLifecycle::Finalizing).unwrap();
        manager.transition(CaptureLifecycle::Saved).unwrap();
        assert_eq!(manager.state().state, CaptureLifecycle::Idle);
    }

    #[test]
    fn interruption_is_recoverable_and_preserves_reason() {
        let manager = CaptureManager::new(None);
        manager
            .start(options(vec![CaptureSource::SystemAudio]))
            .unwrap();
        let state = manager.mark_interrupted("source disconnected").unwrap();
        assert!(state.recoverable);
        assert_eq!(state.reason.as_deref(), Some("source disconnected"));
    }
}
