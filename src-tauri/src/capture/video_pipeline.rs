//! Source -> pacer -> encoder, with one segment per pause
//! (NATIVE_CAPTURE_PLAN NC-1). Pause and resume run on the shared media clock,
//! so pauses never appear in the output and resume returns only once the pacer
//! has re-anchored (the "resume waits for video" ordering).

use super::{
    clock::MediaClock,
    frame::VideoFrame,
    pacer::FramePacer,
    source::{ScreenSource, ScreenTarget},
    video_encoder::{EncoderConfig, SegmentEncoder, SegmentReport, Submit},
    CaptureError,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{channel, sync_channel, Receiver, RecvTimeoutError, Sender, TryRecvError},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const TICK: Duration = Duration::from_millis(5);
/// Frames the source may have in flight before it starts dropping.
const SOURCE_QUEUE: usize = 8;

fn unavailable(message: impl Into<String>) -> CaptureError {
    CaptureError::SourceUnavailable(format!("Screen: {}", message.into()))
}

pub fn segment_path(dir: &std::path::Path, ordinal: usize) -> PathBuf {
    dir.join(format!("screen-{ordinal:03}.mp4"))
}

#[derive(Debug)]
pub struct VideoReport {
    pub segments: Vec<SegmentReport>,
    /// Media time at stop (pauses excluded).
    pub media_duration: Duration,
    pub first_frame_at: Instant,
}

impl VideoReport {
    pub fn frames_written(&self) -> u64 {
        self.segments.iter().map(|s| s.stats.frames_written).sum()
    }
    pub fn frames_dropped(&self) -> u64 {
        self.segments.iter().map(|s| s.stats.frames_dropped).sum()
    }
}

enum Command {
    Pause(Sender<()>),
    Resume(Sender<()>),
    Stop(Duration, Sender<()>),
}

struct Outcome {
    segments: Vec<SegmentReport>,
    error: Option<CaptureError>,
}

struct Worker {
    closed: Arc<std::sync::Mutex<Vec<(PathBuf, f64)>>>,
    failed: Arc<AtomicBool>,
    config: EncoderConfig,
    dir: PathBuf,
    pacer: FramePacer,
    encoder: Option<SegmentEncoder>,
    slots: u64,
    ordinal: usize,
    segments: Vec<SegmentReport>,
    error: Option<CaptureError>,
}

impl Worker {
    fn fail(&mut self, error: CaptureError) {
        self.failed.store(true, Ordering::SeqCst);
        self.error = Some(error);
    }

    fn emit(&mut self, frames: Vec<VideoFrame>) {
        for frame in frames {
            if self.error.is_some() {
                return;
            }
            if self.encoder.is_none() {
                let path = segment_path(&self.dir, self.ordinal);
                match SegmentEncoder::start(
                    &self.config,
                    path,
                    frame.width,
                    frame.height,
                    frame.pixel_format,
                ) {
                    Ok(encoder) => {
                        self.ordinal += 1;
                        self.slots = 0;
                        self.encoder = Some(encoder);
                    }
                    Err(e) => {
                        self.fail(e);
                        return;
                    }
                }
            }
            if let Some(encoder) = &mut self.encoder {
                match encoder.submit(frame) {
                    Submit::Closed => {
                        self.fail(unavailable("the video encoder stopped unexpectedly"));
                        return;
                    }
                    Submit::WrongShape => {
                        // Display change mid-segment: policy belongs to the platform
                        // adapters (plan §4.2); never silently drop video.
                        self.fail(unavailable("the screen size changed during recording"));
                        return;
                    }
                    Submit::Queued | Submit::Dropped => {}
                }
                self.slots += 1;
            }
        }
    }

    fn close_segment(&mut self) {
        if let Some(encoder) = self.encoder.take() {
            match encoder.finish(self.slots) {
                Ok(report) => {
                    let seconds = report.stats.frames_written as f64 / f64::from(self.config.fps);
                    if let Ok(mut closed) = self.closed.lock() {
                        closed.push((report.path.clone(), seconds));
                    }
                    self.segments.push(report)
                }
                Err(e) => self.error = Some(self.error.take().unwrap_or(e)),
            }
        }
        self.slots = 0;
    }

    fn drain(&mut self, frames: &Receiver<VideoFrame>) {
        while let Ok(frame) = frames.try_recv() {
            self.pacer.push(frame);
        }
    }

    fn run(
        mut self,
        commands: Receiver<Command>,
        frames: Receiver<VideoFrame>,
        clock: Arc<MediaClock>,
    ) -> Outcome {
        let mut paused = false;
        loop {
            match commands.try_recv() {
                Ok(Command::Pause(ack)) => {
                    self.drain(&frames);
                    let end = clock.now(); // already frozen by the caller
                    let due = self.pacer.emit_until(end);
                    self.emit(due);
                    self.close_segment();
                    paused = true;
                    let _ = ack.send(());
                }
                Ok(Command::Resume(ack)) => {
                    self.drain(&frames);
                    self.pacer.restart(clock.now());
                    paused = false;
                    let _ = ack.send(());
                }
                Ok(Command::Stop(end, ack)) => {
                    self.drain(&frames);
                    if !paused {
                        let due = self.pacer.emit_until(end);
                        self.emit(due);
                    }
                    self.close_segment();
                    let _ = ack.send(());
                    break;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.close_segment();
                    break;
                }
            }
            match frames.recv_timeout(TICK) {
                Ok(frame) => self.pacer.push(frame),
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {}
            }
            if !paused {
                self.drain(&frames);
                let due = self.pacer.emit_until(clock.now());
                self.emit(due);
            }
        }
        Outcome {
            segments: self.segments,
            error: self.error,
        }
    }
}

pub struct VideoPipeline {
    source: Box<dyn ScreenSource>,
    clock: Arc<MediaClock>,
    commands: Sender<Command>,
    handle: Option<JoinHandle<Outcome>>,
    first_frame_at: Instant,
    failed: Arc<AtomicBool>,
    closed: Arc<std::sync::Mutex<Vec<(PathBuf, f64)>>>,
}

impl VideoPipeline {
    /// Starts the source and blocks until its first frame exists.
    pub fn start(
        mut source: Box<dyn ScreenSource>,
        target: &ScreenTarget,
        clock: Arc<MediaClock>,
        config: EncoderConfig,
        dir: PathBuf,
    ) -> Result<Self, CaptureError> {
        let (frame_tx, frame_rx) = sync_channel::<VideoFrame>(SOURCE_QUEUE);
        let first_frame_at = source.start(target, clock.clone(), frame_tx)?;
        let (commands, command_rx) = channel();
        let failed = Arc::new(AtomicBool::new(false));
        let closed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let worker = Worker {
            closed: closed.clone(),
            failed: failed.clone(),
            pacer: FramePacer::new(config.fps),
            config,
            dir,
            encoder: None,
            slots: 0,
            ordinal: 0,
            segments: Vec::new(),
            error: None,
        };
        let worker_clock = clock.clone();
        let handle = thread::spawn(move || worker.run(command_rx, frame_rx, worker_clock));
        Ok(Self {
            source,
            clock,
            commands,
            handle: Some(handle),
            first_frame_at,
            failed,
            closed,
        })
    }

    /// Segments closed so far (one per pause) with their length in seconds.
    pub fn completed_segments(&self) -> Vec<(PathBuf, f64)> {
        self.closed.lock().map(|c| c.clone()).unwrap_or_default()
    }

    /// False once the encoder died or the OS ended the capture. A paused
    /// pipeline is alive.
    pub fn is_alive(&self) -> bool {
        !self.failed.load(Ordering::SeqCst) && self.source.failure().is_none()
    }

    /// Why the capture died, if it did.
    pub fn failure(&self) -> Option<String> {
        self.source.failure().or_else(|| {
            self.failed
                .load(Ordering::SeqCst)
                .then(|| "the video encoder stopped".to_string())
        })
    }

    pub fn first_frame_at(&self) -> Instant {
        self.first_frame_at
    }

    fn send(&self, make: impl FnOnce(Sender<()>) -> Command) {
        let (ack_tx, ack_rx) = channel();
        if self.commands.send(make(ack_tx)).is_ok() {
            let _ = ack_rx.recv_timeout(Duration::from_secs(15));
        }
    }

    /// Freezes the clock, stops the source, closes the current segment.
    pub fn pause(&mut self) {
        self.source.pause();
        self.clock.pause();
        self.send(Command::Pause);
    }

    pub fn resume(&mut self) -> Result<(), CaptureError> {
        self.clock.resume();
        self.send(Command::Resume);
        self.source.resume()
    }

    pub fn stop(mut self) -> Result<VideoReport, CaptureError> {
        let end = self.clock.now();
        self.source.stop();
        let (ack_tx, ack_rx) = channel();
        if self.commands.send(Command::Stop(end, ack_tx)).is_ok() {
            let _ = ack_rx.recv_timeout(Duration::from_secs(15));
        }
        let outcome = self
            .handle
            .take()
            .and_then(|h| h.join().ok())
            .ok_or_else(|| unavailable("video pipeline thread crashed"))?;
        if let Some(error) = outcome.error {
            return Err(error);
        }
        Ok(VideoReport {
            segments: outcome.segments,
            media_duration: end,
            first_frame_at: self.first_frame_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{screen, source::synthetic::SyntheticSource};
    use std::process::Command as Process;

    fn ffmpeg_for_test() -> Option<PathBuf> {
        let found = screen::locate_ffmpeg();
        if found.is_none() && std::env::var("LOCUS_REQUIRE_FFMPEG").ok().as_deref() == Some("1") {
            panic!("LOCUS_REQUIRE_FFMPEG=1 but no FFmpeg was found");
        }
        found
    }

    /// Video frames in a 320x240 MP4, counted exactly by decoding every frame
    /// to 8-bit gray and dividing the byte count by one frame's size.
    fn count_frames(ffmpeg: &std::path::Path, file: &std::path::Path) -> u64 {
        let out = Process::new(ffmpeg)
            .args(["-hide_banner", "-v", "error", "-nostats", "-i"])
            .arg(file)
            .args(["-map", "0:v:0", "-pix_fmt", "gray", "-f", "rawvideo", "-"])
            .output()
            .expect("run ffmpeg");
        assert!(out.status.success(), "ffmpeg could not decode {file:?}");
        out.stdout.len() as u64 / (320 * 240)
    }

    fn has_audio_or_extra_video(ffmpeg: &std::path::Path, file: &std::path::Path) -> bool {
        let out = Process::new(ffmpeg)
            .args(["-hide_banner", "-i"])
            .arg(file)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stderr).contains("Audio:")
    }

    fn run_synthetic(
        emit_fps: u32,
        seconds: f64,
    ) -> Option<(VideoReport, PathBuf, tempfile::TempDir)> {
        let ffmpeg = ffmpeg_for_test()?;
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(MediaClock::new());
        let pipeline = VideoPipeline::start(
            Box::new(SyntheticSource::new(320, 240, emit_fps)),
            &ScreenTarget::PrimaryDisplay,
            clock,
            EncoderConfig::new(ffmpeg.clone()),
            dir.path().to_path_buf(),
        )
        .unwrap();
        thread::sleep(Duration::from_secs_f64(seconds));
        let report = pipeline.stop().unwrap();
        Some((report, ffmpeg, dir))
    }

    #[test]
    fn moving_screen_yields_duration_times_fps_frames() {
        let Some((report, ffmpeg, _dir)) = run_synthetic(30, 3.0) else {
            return;
        };
        assert_eq!(report.segments.len(), 1);
        let secs = report.media_duration.as_secs_f64();
        assert!((secs - 3.0).abs() < 0.25, "media duration {secs}");
        let frames = count_frames(&ffmpeg, &report.segments[0].path);
        let expected = (secs * 30.0).ceil() as i64;
        assert!(
            (frames as i64 - expected).abs() <= 1,
            "{frames} vs {expected}"
        );
        assert_eq!(frames, report.frames_written());
        assert!(!has_audio_or_extra_video(&ffmpeg, &report.segments[0].path));
    }

    #[test]
    fn static_screen_still_yields_a_full_length_video() {
        // The "OS" reports one frame and then nothing, like an idle desktop.
        let Some((report, ffmpeg, _dir)) = run_synthetic(0, 2.0) else {
            return;
        };
        let secs = report.media_duration.as_secs_f64();
        let frames = count_frames(&ffmpeg, &report.segments[0].path);
        let expected = (secs * 30.0).ceil() as i64;
        assert!(
            (frames as i64 - expected).abs() <= 1,
            "{frames} vs {expected}"
        );
        assert!(frames >= 55, "{frames} frames for a 2 s static screen");
    }

    #[test]
    fn pause_creates_a_segment_and_leaves_no_gap_or_pause_time() {
        let Some(ffmpeg) = ffmpeg_for_test() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(MediaClock::new());
        let mut pipeline = VideoPipeline::start(
            Box::new(SyntheticSource::new(320, 240, 30)),
            &ScreenTarget::PrimaryDisplay,
            clock,
            EncoderConfig::new(ffmpeg.clone()),
            dir.path().to_path_buf(),
        )
        .unwrap();
        thread::sleep(Duration::from_secs(1));
        pipeline.pause();
        thread::sleep(Duration::from_millis(1500)); // must not appear in the output
        let resumed_at = Instant::now();
        pipeline.resume().unwrap();
        assert!(
            resumed_at.elapsed() < Duration::from_millis(500),
            "resume waited {:?}",
            resumed_at.elapsed()
        );
        thread::sleep(Duration::from_secs(1));
        let report = pipeline.stop().unwrap();
        assert_eq!(report.segments.len(), 2, "one segment per pause");
        let secs = report.media_duration.as_secs_f64();
        assert!(
            (secs - 2.0).abs() < 0.3,
            "media duration {secs} includes the pause"
        );
        let frames: u64 = report
            .segments
            .iter()
            .map(|s| count_frames(&ffmpeg, &s.path))
            .sum();
        let expected = (secs * 30.0).ceil() as i64;
        assert!(
            (frames as i64 - expected).abs() <= 2,
            "{frames} vs {expected}"
        );
    }

    #[test]
    fn media_clock_records_the_video_offset() {
        let Some(ffmpeg) = ffmpeg_for_test() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(MediaClock::new());
        let pipeline = VideoPipeline::start(
            Box::new(SyntheticSource::new(64, 48, 30)),
            &ScreenTarget::PrimaryDisplay,
            clock.clone(),
            EncoderConfig::new(ffmpeg),
            dir.path().to_path_buf(),
        )
        .unwrap();
        thread::sleep(Duration::from_millis(300));
        pipeline.stop().unwrap();
        let offsets = clock.source_offsets();
        assert_eq!(offsets[0], ("video".to_string(), Duration::ZERO));
    }
}
