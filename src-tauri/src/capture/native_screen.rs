//! The native screen backend as the manager sees it
//! (`LOCUS_CAPTURE_BACKEND=native`, NATIVE_CAPTURE_PLAN NC-2).
//!
//! `NativeScreen` gives a [`VideoPipeline`] the same control surface as the
//! FFmpeg-driven `screen::ScreenCapture`, and `ScreenRecorder` lets a session
//! hold either one, so the pause / stop / finalise code in `mod.rs` does not
//! care which backend recorded.

use super::{
    clock::MediaClock,
    screen::{self, FinishedScreen},
    source::{ScreenSource, ScreenTarget},
    video_encoder::EncoderConfig,
    video_pipeline::VideoPipeline,
    CaptureError,
};
use std::{path::Path, sync::Arc, time::Instant};

/// The platform's real display source, or why there is none yet.
pub fn platform_source() -> Result<Box<dyn ScreenSource>, CaptureError> {
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(super::macos_sck::MacScreenSource::new()))
    }
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(super::windows_wgc::WinScreenSource::new()))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err(CaptureError::SourceUnavailable(
            "Screen: the native capture backend is only implemented on macOS and Windows so far; \
             unset LOCUS_CAPTURE_BACKEND or set it to \"ffmpeg\""
                .into(),
        ))
    }
}

/// Displays and windows that can be recorded, for the frontend picker.
pub fn list_sources() -> Result<Vec<super::source::ScreenSourceInfo>, CaptureError> {
    #[cfg(target_os = "macos")]
    {
        super::macos_sck::list_sources()
    }
    #[cfg(target_os = "windows")]
    {
        super::windows_wgc::list_sources()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err(CaptureError::SourceUnavailable(
            "Choosing a screen or window is only available on macOS and Windows so far.".into(),
        ))
    }
}

pub struct NativeScreen {
    /// `None` once finished or aborted.
    pipeline: Option<VideoPipeline>,
    dir: std::path::PathBuf,
    video_started_at: Instant,
    /// Segments produced so far (one per pause).
    segments: Vec<std::path::PathBuf>,
}

impl NativeScreen {
    /// Starts capture and blocks until the first frame exists.
    pub fn start(
        source: Box<dyn ScreenSource>,
        target: &ScreenTarget,
        dir: &Path,
        ffmpeg: std::path::PathBuf,
    ) -> Result<Self, CaptureError> {
        let clock = Arc::new(MediaClock::new());
        let pipeline = VideoPipeline::start(
            source,
            target,
            clock,
            EncoderConfig::new(ffmpeg),
            dir.to_path_buf(),
        )?;
        let video_started_at = pipeline.first_frame_at();
        Ok(Self {
            pipeline: Some(pipeline),
            dir: dir.to_path_buf(),
            video_started_at,
            segments: Vec::new(),
        })
    }

    pub fn started_at(&self) -> Instant {
        self.video_started_at
    }

    /// Native pause freezes the media clock and closes the segment in one step,
    /// so there is nothing to wait for separately in `complete_stop`.
    pub fn request_stop(&mut self) {
        if let Some(pipeline) = &mut self.pipeline {
            pipeline.pause();
        }
    }

    pub fn complete_stop(&mut self) {}

    pub fn completed_segments(&self) -> Vec<(std::path::PathBuf, f64)> {
        self.pipeline
            .as_ref()
            .map(|p| p.completed_segments())
            .unwrap_or_default()
    }

    pub fn resume(&mut self) -> Result<(), CaptureError> {
        match &mut self.pipeline {
            Some(pipeline) => pipeline.resume(),
            None => Ok(()),
        }
    }

    pub fn is_alive(&mut self) -> bool {
        self.pipeline.as_ref().map_or(true, |p| p.is_alive())
    }

    /// Why the capture died, when `is_alive` is false.
    pub fn failure(&self) -> Option<String> {
        self.pipeline.as_ref().and_then(|p| p.failure())
    }

    pub fn finish(mut self) -> FinishedScreen {
        if let Some(pipeline) = self.pipeline.take() {
            match pipeline.stop() {
                Ok(report) => {
                    self.segments = report.segments.into_iter().map(|s| s.path).collect();
                }
                Err(error) => {
                    // Keep whatever segments reached disk so the take is not lost.
                    eprintln!("locus: native screen finish: {error}");
                    self.segments = existing_segments(&self.dir);
                }
            }
        }
        FinishedScreen {
            segments: std::mem::take(&mut self.segments),
            video_started_at: self.video_started_at,
        }
    }

    pub fn abort(mut self) {
        if let Some(pipeline) = self.pipeline.take() {
            let _ = pipeline.stop();
        }
        for segment in existing_segments(&self.dir) {
            let _ = std::fs::remove_file(segment);
        }
    }
}

fn existing_segments(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut found: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("screen-") && n.ends_with(".mp4"))
        })
        .collect();
    found.sort();
    found
}

/// Either backend, behind the methods the manager uses.
pub enum ScreenRecorder {
    Ffmpeg(screen::ScreenCapture),
    Native(NativeScreen),
}

impl ScreenRecorder {
    pub fn started_at(&self) -> Instant {
        match self {
            Self::Ffmpeg(s) => s.started_at(),
            Self::Native(s) => s.started_at(),
        }
    }

    /// Segments closed so far with their length in seconds (0.0 when the
    /// backend does not know it). Call after a pause.
    pub fn completed_segments(&self) -> Vec<(std::path::PathBuf, f64)> {
        match self {
            Self::Ffmpeg(s) => s
                .completed_segments()
                .into_iter()
                .map(|p| (p, 0.0))
                .collect(),
            Self::Native(s) => s.completed_segments(),
        }
    }

    /// Why the capture died, when `is_alive` is false.
    pub fn failure(&self) -> Option<String> {
        match self {
            Self::Ffmpeg(_) => Some("the screen recorder stopped unexpectedly".into()),
            Self::Native(s) => s.failure(),
        }
    }

    pub fn request_stop(&mut self) {
        match self {
            Self::Ffmpeg(s) => s.request_stop(),
            Self::Native(s) => s.request_stop(),
        }
    }

    pub fn complete_stop(&mut self) {
        match self {
            Self::Ffmpeg(s) => s.complete_stop(),
            Self::Native(s) => s.complete_stop(),
        }
    }

    pub fn resume(&mut self) -> Result<(), CaptureError> {
        match self {
            Self::Ffmpeg(s) => s.resume(),
            Self::Native(s) => s.resume(),
        }
    }

    pub fn is_alive(&mut self) -> bool {
        match self {
            Self::Ffmpeg(s) => s.is_alive(),
            Self::Native(s) => s.is_alive(),
        }
    }

    pub fn finish(self) -> FinishedScreen {
        match self {
            Self::Ffmpeg(s) => s.finish(),
            Self::Native(s) => s.finish(),
        }
    }

    pub fn abort(self) {
        match self {
            Self::Ffmpeg(s) => s.abort(),
            Self::Native(s) => s.abort(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::source::synthetic::SyntheticSource;
    use std::{thread, time::Duration};

    fn ffmpeg_for_test() -> Option<std::path::PathBuf> {
        let found = screen::locate_ffmpeg();
        if found.is_none() && std::env::var("LOCUS_REQUIRE_FFMPEG").ok().as_deref() == Some("1") {
            panic!("LOCUS_REQUIRE_FFMPEG=1 but no FFmpeg was found");
        }
        found
    }

    #[test]
    fn manager_style_lifecycle_produces_one_segment_per_pause() {
        let Some(ffmpeg) = ffmpeg_for_test() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let mut screen = NativeScreen::start(
            Box::new(SyntheticSource::new(320, 240, 30)),
            &ScreenTarget::PrimaryDisplay,
            dir.path(),
            ffmpeg,
        )
        .unwrap();
        let started = screen.started_at();
        thread::sleep(Duration::from_millis(600));
        // Exactly the calls the manager makes on pause, resume and stop.
        screen.request_stop();
        screen.complete_stop();
        assert!(screen.is_alive(), "a paused recording is alive");
        screen.resume().unwrap();
        thread::sleep(Duration::from_millis(600));
        let mut recorder = ScreenRecorder::Native(screen);
        recorder.request_stop();
        let finished = recorder.finish();
        assert_eq!(finished.segments.len(), 2);
        assert!(finished.segments.iter().all(|p| p.exists()));
        assert_eq!(finished.video_started_at, started);
    }

    #[test]
    fn abort_removes_the_segments() {
        let Some(ffmpeg) = ffmpeg_for_test() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let screen = NativeScreen::start(
            Box::new(SyntheticSource::new(320, 240, 30)),
            &ScreenTarget::PrimaryDisplay,
            dir.path(),
            ffmpeg,
        )
        .unwrap();
        thread::sleep(Duration::from_millis(400));
        screen.abort();
        assert!(existing_segments(dir.path()).is_empty());
    }

    #[test]
    fn a_source_the_os_ended_is_reported_dead() {
        struct Dying(
            SyntheticSource,
            std::sync::Arc<std::sync::atomic::AtomicBool>,
        );
        impl ScreenSource for Dying {
            fn start(
                &mut self,
                t: &ScreenTarget,
                c: Arc<MediaClock>,
                f: std::sync::mpsc::SyncSender<crate::capture::frame::VideoFrame>,
            ) -> Result<Instant, CaptureError> {
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
                    .then(|| "display disconnected".to_string())
            }
        }
        let Some(ffmpeg) = ffmpeg_for_test() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut screen = NativeScreen::start(
            Box::new(Dying(SyntheticSource::new(320, 240, 30), flag.clone())),
            &ScreenTarget::PrimaryDisplay,
            dir.path(),
            ffmpeg,
        )
        .unwrap();
        assert!(screen.is_alive());
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(!screen.is_alive());
        assert_eq!(screen.failure().as_deref(), Some("display disconnected"));
        // The take so far is still finalised, never lost.
        let finished = screen.finish();
        assert!(!finished.segments.is_empty());
    }
}
