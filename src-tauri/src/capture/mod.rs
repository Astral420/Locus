pub mod audio;
pub mod clock;
pub mod container;
pub mod encoder;
pub mod frame;
pub mod linux;
pub mod macos;
#[cfg(target_os = "macos")]
pub mod macos_sck;
pub mod native_screen;
pub mod pacer;
pub mod preflight;
pub mod recovery;
pub mod screen;
pub mod source;
pub mod timeline;
pub mod video_encoder;
pub mod video_pipeline;
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

/// Which implementation records the screen. `LOCUS_CAPTURE_BACKEND` selects it;
/// `ffmpeg` (the interim FFmpeg-driven capture) stays the default until a
/// native backend is proven on each platform (NATIVE_CAPTURE_PLAN NC-0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureBackend {
    Ffmpeg,
    Native,
}

impl CaptureBackend {
    pub const ENV_VAR: &'static str = "LOCUS_CAPTURE_BACKEND";

    /// Parses a switch value. Unset or empty means the default (`ffmpeg`).
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            None | Some("") | Some("ffmpeg") => Ok(Self::Ffmpeg),
            Some("native") => Ok(Self::Native),
            Some(other) => Err(format!(
                "unknown {} value \"{other}\" (expected \"ffmpeg\" or \"native\")",
                Self::ENV_VAR
            )),
        }
    }

    pub fn from_env() -> Result<Self, String> {
        Self::parse(std::env::var(Self::ENV_VAR).ok().as_deref())
    }
}

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
    /// Which display or window to record (FR1.3). `None` means the primary
    /// display. Only the native backend honours it; the FFmpeg fallback always
    /// records the primary display.
    #[serde(default)]
    pub screen_target: Option<source::ScreenTarget>,
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
    screen: Option<native_screen::ScreenRecorder>,
    /// When the audio streams were really recording (t=0 of the audio file).
    audio_started_at: Option<std::time::Instant>,
    /// Set once the video+audio MP4 has been produced.
    muxed_video: bool,
    /// Verified length of the finished recording file, once known.
    final_duration: Option<f64>,
    /// Durable record of how this capture was set up and which segments are
    /// complete; what crash recovery rebuilds from.
    manifest: Option<encoder::Encoder>,
    registered_segments: usize,
    registered_seconds: f64,
}

fn source_name(source: &CaptureSource) -> String {
    serde_json::to_string(source)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string()
}

/// Seconds the audio began after the first video frame (negative: the audio
/// was already recording before frame 0).
fn audio_offset_secs(video: Option<std::time::Instant>, audio: Option<std::time::Instant>) -> f64 {
    match (video, audio) {
        (Some(video), Some(audio)) if audio >= video => audio.duration_since(video).as_secs_f64(),
        (Some(video), Some(audio)) => -video.duration_since(audio).as_secs_f64(),
        _ => 0.0,
    }
}

/// Records video segments closed since the last call in the manifest file and
/// the database, so a crash later never loses track of them.
fn register_segments(db: Option<&Database>, session: &mut Session) {
    let Some(screen) = session.screen.as_ref() else {
        return;
    };
    let completed = screen.completed_segments();
    for (path, seconds) in completed.into_iter().skip(session.registered_segments) {
        let ordinal = session.registered_segments as i64;
        let file = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let start = session.registered_seconds;
        if let Some(manifest) = session.manifest.as_mut() {
            let _ = manifest.add_segment(encoder::Segment {
                stream_id: "screen".into(),
                source: "screen".into(),
                ordinal: ordinal as u64,
                relative_path: file.clone(),
                start_seconds: start,
                duration_seconds: seconds,
                checksum: None,
            });
        }
        if let Some(db) = db {
            let meeting_id = session.meeting_id.clone();
            let relative = format!("{meeting_id}/{file}");
            let _ = db.run(move |connection| {
                connection
                    .execute(
                        "INSERT OR REPLACE INTO media_segments(id, manifest_id, stream_id, source, ordinal, relative_path, start_seconds, duration_seconds, committed_at) VALUES (?1, ?2, 'screen', 'screen', ?3, ?4, ?5, ?6, ?7)",
                        (
                            format!("segment:{meeting_id}:screen:{ordinal}"),
                            format!("manifest:{meeting_id}"),
                            ordinal,
                            &relative,
                            start,
                            seconds,
                            Utc::now().to_rfc3339(),
                        ),
                    )
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            });
        }
        session.registered_segments += 1;
        session.registered_seconds += seconds;
    }
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
        // A capture that was stopped and preserved after a failure no longer
        // blocks a new one; its files stay on disk and are offered for recovery.
        if active.as_ref().is_some_and(|session| {
            !matches!(
                session.state,
                CaptureLifecycle::Recoverable | CaptureLifecycle::Interrupted
            )
        }) {
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
            final_duration: None,
            manifest: None,
            registered_segments: 0,
            registered_seconds: 0.0,
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
                let target = options
                    .screen_target
                    .clone()
                    .unwrap_or(source::ScreenTarget::PrimaryDisplay);
                std::thread::spawn(move || match CaptureBackend::from_env() {
                    Ok(CaptureBackend::Ffmpeg) => screen::ScreenCapture::start(&dir)
                        .map(native_screen::ScreenRecorder::Ffmpeg),
                    Ok(CaptureBackend::Native) => {
                        let ffmpeg = screen::locate_ffmpeg().ok_or_else(|| {
                            CaptureError::SourceUnavailable(
                                "Screen: FFmpeg was not found (it encodes the native capture)"
                                    .into(),
                            )
                        });
                        ffmpeg.and_then(|ffmpeg| {
                            native_screen::NativeScreen::start(
                                native_screen::platform_source()?,
                                &target,
                                &dir,
                                ffmpeg,
                            )
                            .map(native_screen::ScreenRecorder::Native)
                        })
                    }
                    Err(message) => Err(CaptureError::SourceUnavailable(format!(
                        "Screen: {message}"
                    ))),
                })
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
            // Durable record of the setup, so a crash can be rebuilt (NC-5).
            let video_started_at = session.screen.as_ref().map(|s| s.started_at());
            let track_leads = session
                .audio
                .as_ref()
                .map(|a| a.track_leads())
                .unwrap_or_default();
            if let Ok(mut manifest) =
                encoder::Encoder::create(&media_dir, &session.capture_id, &session.meeting_id)
            {
                let _ = manifest.set_session(
                    session.sources.iter().map(source_name).collect(),
                    session.screen.is_some(),
                    audio_offset_secs(video_started_at, session.audio_started_at),
                    track_leads
                        .iter()
                        .map(|(source, lead)| encoder::TrackLead {
                            source: source_name(source),
                            lead_seconds: *lead,
                        })
                        .collect(),
                );
                session.manifest = Some(manifest);
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
            register_segments(self.db.as_ref(), session);
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
            let video = screen.map(|screen| screen.finish());
            if let Some(finished) = session.finished.clone() {
                let output = finished.mixed_path.with_file_name(screen::VIDEO_FILE_NAME);
                let offset = audio_offset_secs(
                    video.as_ref().map(|v| v.video_started_at),
                    session.audio_started_at,
                );
                let segments: Vec<std::path::PathBuf> = video
                    .as_ref()
                    .map(|v| v.segments.clone())
                    .unwrap_or_default();
                let wav_for = |source: &CaptureSource| {
                    finished
                        .tracks
                        .iter()
                        .find(|(s, _)| s == source)
                        .map(|(_, path)| path.as_path())
                };
                let inputs = container::plan_streams(
                    &finished.mixed_path,
                    wav_for(&CaptureSource::SystemAudio),
                    wav_for(&CaptureSource::Microphone),
                );
                // Separate audio streams (FR1.13); if that mux fails, fall back
                // to the proven single mixed-track MP4 so the take is not lost.
                let result = match screen::locate_ffmpeg() {
                    Some(ffmpeg) => {
                        // Mux, then read the result back before anything raw is deleted.
                        let primary =
                            container::mux_container(&ffmpeg, &segments, &inputs, offset, &output)
                                .and_then(|streams| {
                                    container::verify_recording(
                                        &ffmpeg,
                                        &output,
                                        !segments.is_empty(),
                                        inputs.len(),
                                    )
                                    .map(|verified| (streams, verified.duration_seconds))
                                });
                        match primary {
                            Ok((streams, verified_duration)) => {
                                // The muxed file is what plays back; its length (after
                                // sync trimming) is the true duration, not wall-clock.
                                session.final_duration = Some(verified_duration);
                                if let Ok(json) = serde_json::to_string_pretty(&streams) {
                                    let _ = std::fs::write(
                                        output.with_file_name(container::STREAMS_FILE_NAME),
                                        json,
                                    );
                                }
                                // Saved first: after this, recovery never rebuilds over it.
                                if let Some(manifest) = session.manifest.as_mut() {
                                    let _ = manifest.mark_state("saved");
                                }
                                if std::env::var("LOCUS_KEEP_SEGMENTS").ok().as_deref() != Some("1")
                                {
                                    for segment in &segments {
                                        let _ = std::fs::remove_file(segment);
                                    }
                                }
                                for (_, aligned) in &finished.tracks {
                                    let _ = std::fs::remove_file(aligned);
                                }
                                Ok(())
                            }
                            Err(error) => {
                                let _ = std::fs::remove_file(&output);
                                if segments.is_empty() {
                                    Err(error)
                                } else {
                                    screen::mux_recording(
                                        &ffmpeg,
                                        &segments,
                                        Some((finished.mixed_path.as_path(), offset)),
                                        &output,
                                    )
                                }
                            }
                        }
                    }
                    None if segments.is_empty() => Err("FFmpeg is unavailable".into()),
                    None => Err("FFmpeg disappeared before the recording was finalised".into()),
                };
                if !segments.is_empty() {
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
                }
                match result {
                    // `recording.mp4` exists: video + audio streams, or an
                    // audio-only MP4 with no video stream for audio-only sessions.
                    Ok(()) => session.muxed_video = true,
                    // Never lose the take: audio.wav stays playable and the raw
                    // video segments stay on disk for recovery.
                    Err(error) if !segments.is_empty() => {
                        session.reason = Some(format!(
                            "Video could not be finalised ({error}); audio was saved."
                        ))
                    }
                    // Audio-only: audio.wav is already the saved recording.
                    Err(_) => {}
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
        if matches!(
            session.state,
            CaptureLifecycle::Recoverable | CaptureLifecycle::Interrupted
        ) {
            if let Some(manifest) = session.manifest.as_mut() {
                let _ = manifest.set_reason(session.reason.clone());
                // A recording that was finalised and verified is complete and must
                // never be rebuilt; anything else stays recoverable from raw files.
                let _ = manifest.mark_state(if session.muxed_video {
                    "saved"
                } else {
                    "interrupted"
                });
            }
        }
        if let Some(database) = &self.db {
            let meeting_id = session.meeting_id.clone();
            let lifecycle = match session.state {
                CaptureLifecycle::Finalizing => "finalizing",
                CaptureLifecycle::Saved => "saved",
                CaptureLifecycle::Recoverable | CaptureLifecycle::Interrupted => "interrupted",
                CaptureLifecycle::Paused => "paused",
                _ => "recording",
            };
            let duration = session.final_duration.unwrap_or(state.elapsed_seconds);
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
        let preserved_complete =
            matches!(session.state, CaptureLifecycle::Recoverable) && session.muxed_video;
        if matches!(session.state, CaptureLifecycle::Saved) || preserved_complete {
            if let (Some(database), Some(finished)) = (&self.db, session.finished.clone()) {
                let meeting_id = session.meeting_id.clone();
                let file = if session.muxed_video {
                    screen::VIDEO_FILE_NAME
                } else {
                    audio::MIXED_FILE_NAME
                };
                let relative_path = format!("{meeting_id}/{file}");
                let duration = session.final_duration.unwrap_or(finished.duration_seconds);
                let _ = database.run(move |connection| {
                    // The raw video segments were replaced by the final file.
                    connection
                        .execute(
                            "DELETE FROM media_segments WHERE manifest_id=?1 AND source='screen'",
                            [format!("manifest:{meeting_id}")],
                        )
                        .map_err(|error| error.to_string())?;
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
        }
        if matches!(session.state, CaptureLifecycle::Saved) {
            *active = None;
        }
        Ok(state)
    }

    /// Stops a capture that can no longer continue (disk full, display or
    /// device lost, permission revoked), finalising what was recorded so the
    /// take is kept. The meeting is left interrupted with the reason.
    pub fn stop_and_preserve(
        &self,
        reason: impl Into<String>,
    ) -> Result<RecordingStateDto, CaptureError> {
        {
            let mut active = self.active.lock().expect("capture mutex poisoned");
            let session = active.as_mut().ok_or(CaptureError::NoActiveCapture)?;
            session.reason = Some(reason.into());
        }
        self.transition(CaptureLifecycle::Finalizing)?;
        self.transition(CaptureLifecycle::Recoverable)
    }

    /// One watchdog pass: if the running capture has failed, stop and keep it.
    /// Returns the reason when it did.
    pub fn check_health(&self) -> Option<String> {
        let reason = {
            let mut active = self.active.lock().expect("capture mutex poisoned");
            let session = active.as_mut()?;
            if session.state != CaptureLifecycle::Recording {
                return None;
            }
            let mut screen_failure = None;
            if let Some(screen) = session.screen.as_mut() {
                if !screen.is_alive() {
                    screen_failure = Some(
                        screen
                            .failure()
                            .unwrap_or_else(|| "the screen recorder stopped".into()),
                    );
                }
            }
            screen_failure.or_else(|| session.audio.as_ref().and_then(|audio| audio.health()))?
        };
        let _ = self.stop_and_preserve(reason.clone());
        Some(reason)
    }

    /// Runs `check_health` every two seconds for the life of the app.
    pub fn start_watchdog(&self) {
        let manager = self.clone();
        let _ = std::thread::Builder::new()
            .name("locus-capture-watchdog".into())
            .spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                manager.check_health();
            });
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

    #[test]
    fn capture_backend_defaults_to_ffmpeg() {
        assert_eq!(CaptureBackend::parse(None), Ok(CaptureBackend::Ffmpeg));
        assert_eq!(CaptureBackend::parse(Some("")), Ok(CaptureBackend::Ffmpeg));
        assert_eq!(
            CaptureBackend::parse(Some(" FFmpeg ")),
            Ok(CaptureBackend::Ffmpeg)
        );
    }

    #[test]
    fn capture_backend_parses_native_and_rejects_unknown() {
        assert_eq!(
            CaptureBackend::parse(Some("native")),
            Ok(CaptureBackend::Native)
        );
        assert!(CaptureBackend::parse(Some("gstreamer")).is_err());
    }
    fn options(sources: Vec<CaptureSource>) -> CaptureOptions {
        CaptureOptions {
            sources,
            meeting_type: MeetingType::Auto,
            single_person_mic: false,
            output_root: PathBuf::from("media"),
            title: "Test capture".into(),
            screen_target: None,
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

    fn empty_session(state: CaptureLifecycle) -> Session {
        Session {
            capture_id: "capture-1".into(),
            meeting_id: "meeting-1".into(),
            generation: 0,
            state,
            sources: vec![CaptureSource::Microphone],
            timeline: timeline::MediaTimeline::start(),
            reason: None,
            audio: None,
            finished: None,
            screen: None,
            audio_started_at: None,
            muxed_video: false,
            manifest: None,
            registered_segments: 0,
            registered_seconds: 0.0,
        }
    }

    #[test]
    fn a_healthy_capture_is_left_alone_by_the_watchdog() {
        let manager = CaptureManager::new(None);
        *manager.active.lock().unwrap() = Some(empty_session(CaptureLifecycle::Recording));
        assert_eq!(manager.check_health(), None);
        assert_eq!(manager.state().state, CaptureLifecycle::Recording);
        let idle = CaptureManager::new(None);
        assert_eq!(idle.check_health(), None);
    }

    #[test]
    fn a_dead_screen_source_stops_and_preserves_the_capture() {
        use crate::capture::source::{synthetic::SyntheticSource, ScreenSource, ScreenTarget};
        let Some(ffmpeg) = screen::locate_ffmpeg() else {
            if std::env::var("LOCUS_REQUIRE_FFMPEG").ok().as_deref() == Some("1") {
                panic!("LOCUS_REQUIRE_FFMPEG=1 but no FFmpeg was found");
            }
            return;
        };
        // A display that is unplugged mid-recording: the OS ends the stream.
        struct Unplugged(
            SyntheticSource,
            std::sync::Arc<std::sync::atomic::AtomicBool>,
        );
        impl ScreenSource for Unplugged {
            fn start(
                &mut self,
                t: &ScreenTarget,
                c: std::sync::Arc<clock::MediaClock>,
                f: std::sync::mpsc::SyncSender<frame::VideoFrame>,
            ) -> Result<std::time::Instant, CaptureError> {
                self.0.start(t, c, f)
            }
            fn pause(&mut self) {
                self.0.pause()
            }
            fn resume(&mut self) -> Result<(), CaptureError> {
                self.0.resume()
            }
            fn stop(&mut self) {
                self.0.stop()
            }
            fn failure(&self) -> Option<String> {
                self.1
                    .load(std::sync::atomic::Ordering::SeqCst)
                    .then(|| "the display was disconnected".to_string())
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let unplugged = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let screen = native_screen::NativeScreen::start(
            Box::new(Unplugged(
                SyntheticSource::new(320, 240, 30),
                unplugged.clone(),
            )),
            &ScreenTarget::PrimaryDisplay,
            dir.path(),
            ffmpeg,
        )
        .unwrap();
        let manager = CaptureManager::new(None);
        let mut session = empty_session(CaptureLifecycle::Recording);
        session.sources = vec![CaptureSource::Screen];
        session.screen = Some(native_screen::ScreenRecorder::Native(screen));
        *manager.active.lock().unwrap() = Some(session);
        std::thread::sleep(std::time::Duration::from_millis(1500));
        assert_eq!(manager.check_health(), None, "alive until the OS ends it");

        unplugged.store(true, std::sync::atomic::Ordering::SeqCst);
        let reason = manager.check_health().expect("the dead source is noticed");
        assert!(reason.contains("disconnected"), "{reason}");
        let state = manager.state();
        assert_eq!(state.state, CaptureLifecycle::Recoverable);
        assert!(state.recoverable);
        assert!(state.reason.unwrap().contains("disconnected"));
        // What was recorded before the loss is still on disk.
        let kept = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with("screen-"));
        assert!(kept, "the video recorded so far must be preserved");
        // A preserved capture does not block the next one.
        assert!(manager.check_health().is_none(), "no repeat stop");
    }

    #[test]
    fn a_disk_write_failure_stops_and_keeps_a_verified_recording() {
        let Some(_) = screen::locate_ffmpeg() else {
            if std::env::var("LOCUS_REQUIRE_FFMPEG").ok().as_deref() == Some("1") {
                panic!("LOCUS_REQUIRE_FFMPEG=1 but no FFmpeg was found");
            }
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let audio = audio::failed_capture_for_test(
            dir.path(),
            "cannot write audio to disk (is the disk full?): no space left",
        );
        let mut manifest = encoder::Encoder::create(dir.path(), "capture-1", "meeting-1").unwrap();
        manifest
            .set_session(vec!["microphone".into()], false, 0.0, vec![])
            .unwrap();
        let manager = CaptureManager::new(None);
        let mut session = empty_session(CaptureLifecycle::Recording);
        session.audio_started_at = audio.started_at();
        session.audio = Some(audio);
        session.manifest = Some(manifest);
        *manager.active.lock().unwrap() = Some(session);

        let reason = manager.check_health().expect("the failed write is noticed");
        assert!(reason.contains("disk"), "{reason}");
        let state = manager.state();
        assert_eq!(state.state, CaptureLifecycle::Recoverable);
        assert!(state.reason.unwrap().contains("disk"));
        // The one second that reached the disk was finalised into a recording.
        assert!(dir.path().join(screen::VIDEO_FILE_NAME).exists());
        assert!(
            dir.path().join("microphone.wav").exists(),
            "raw audio is kept"
        );
        let manifest = encoder::Encoder::open(dir.path()).unwrap();
        assert_eq!(
            manifest.manifest().state,
            "saved",
            "verified, so never rebuilt"
        );
        assert!(manifest
            .manifest()
            .reason
            .as_deref()
            .unwrap_or("")
            .contains("disk"));
        // A new capture may start while the preserved one waits on disk.
        assert!(manager.check_health().is_none());
    }
}
