//! Pipes paced raw frames into a bundled FFmpeg that only *encodes*
//! (NATIVE_CAPTURE_PLAN §4.1 `VideoEncoder`, DESIGN l.118): H.264 into a
//! fragmented MP4 segment that stays readable after a kill.
//!
//! The capture callback must never block, so frames cross a small bounded queue
//! with `try_send`. If FFmpeg falls behind, frames are dropped at the queue (and
//! counted) rather than growing memory; the writer thread then repeats the last
//! written frame over any gap so the segment keeps its real-time length.

use super::{
    frame::{PixelFormat, VideoFrame},
    CaptureError,
};
use std::{
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{sync_channel, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const STOP_TIMEOUT: Duration = Duration::from_secs(10);
const STDERR_TAIL_BYTES: usize = 2048;

fn unavailable(message: impl Into<String>) -> CaptureError {
    CaptureError::SourceUnavailable(format!("Screen: {}", message.into()))
}

#[derive(Debug, Clone)]
pub struct EncoderConfig {
    pub ffmpeg: PathBuf,
    pub fps: u32,
    pub crf: u32,
    pub preset: String,
    /// Frames buffered between the pacer and FFmpeg before new ones are dropped.
    pub queue_frames: usize,
}

impl EncoderConfig {
    /// Defaults follow the plan's proposal for D4 (30 fps, CRF-based). The
    /// encoder itself (`libx264`, as in the interim path) is decision D1.
    pub fn new(ffmpeg: PathBuf) -> Self {
        Self {
            ffmpeg,
            fps: 30,
            crf: 26,
            preset: "veryfast".into(),
            queue_frames: 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Submit {
    Queued,
    /// The queue was full; the frame was dropped without blocking.
    Dropped,
    /// Size or pixel format differs from the segment's (display change).
    WrongShape,
    /// The writer has stopped (FFmpeg exited or the pipe broke).
    Closed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WriterStats {
    pub frames_written: u64,
    /// Frames dropped because the queue was full.
    pub frames_dropped: u64,
    /// Repeats written to cover dropped frames.
    pub frames_filled: u64,
}

fn slot_of(offset: Duration, fps: u32) -> u64 {
    let nanos = offset.as_nanos();
    ((nanos * u128::from(fps) + 500_000_000) / 1_000_000_000) as u64
}

/// Bounded queue plus writer thread in front of any `Write` sink.
pub struct FrameWriter {
    tx: Option<SyncSender<VideoFrame>>,
    handle: Option<JoinHandle<io::Result<WriterStats>>>,
    dropped: Arc<AtomicU64>,
    total_slots: Arc<AtomicU64>,
    shape: Option<(u32, u32, PixelFormat)>,
}

impl FrameWriter {
    pub fn spawn(mut sink: Box<dyn Write + Send>, fps: u32, queue_frames: usize) -> Self {
        let (tx, rx) = sync_channel::<VideoFrame>(queue_frames.max(1));
        let dropped = Arc::new(AtomicU64::new(0));
        let total_slots = Arc::new(AtomicU64::new(0));
        let thread_dropped = dropped.clone();
        let thread_total = total_slots.clone();
        let handle = thread::spawn(move || {
            let mut stats = WriterStats::default();
            let mut base: Option<Duration> = None;
            let mut last: Option<VideoFrame> = None;
            for frame in rx.iter() {
                let base = *base.get_or_insert(frame.pts);
                let slot = slot_of(frame.pts.saturating_sub(base), fps);
                // Cover slots whose frames were dropped under backpressure.
                while stats.frames_written < slot {
                    let Some(previous) = &last else { break };
                    sink.write_all(&previous.packed_bytes())?;
                    stats.frames_written += 1;
                    stats.frames_filled += 1;
                }
                if stats.frames_written > slot {
                    continue;
                }
                sink.write_all(&frame.packed_bytes())?;
                stats.frames_written += 1;
                last = Some(frame);
            }
            // Cover dropped frames at the very end of the segment.
            let total = thread_total.load(Ordering::Acquire);
            while stats.frames_written < total {
                let Some(previous) = &last else { break };
                sink.write_all(&previous.packed_bytes())?;
                stats.frames_written += 1;
                stats.frames_filled += 1;
            }
            sink.flush()?;
            stats.frames_dropped = thread_dropped.load(Ordering::Acquire);
            Ok(stats)
        });
        Self {
            tx: Some(tx),
            handle: Some(handle),
            dropped,
            total_slots,
            shape: None,
        }
    }

    /// Never blocks.
    pub fn submit(&mut self, frame: VideoFrame) -> Submit {
        let shape = (frame.width, frame.height, frame.pixel_format);
        match self.shape {
            Some(expected) if expected != shape => return Submit::WrongShape,
            None => self.shape = Some(shape),
            _ => {}
        }
        let Some(tx) = &self.tx else {
            return Submit::Closed;
        };
        match tx.try_send(frame) {
            Ok(()) => Submit::Queued,
            Err(TrySendError::Full(_)) => {
                self.dropped.fetch_add(1, Ordering::AcqRel);
                Submit::Dropped
            }
            Err(TrySendError::Disconnected(_)) => Submit::Closed,
        }
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Acquire)
    }

    /// Ends the segment. `total_slots` is how many frames the pacer emitted for
    /// it; any shortfall at the tail (frames dropped last) is filled.
    pub fn finish(mut self, total_slots: u64) -> io::Result<WriterStats> {
        self.total_slots.store(total_slots, Ordering::Release);
        self.tx = None; // closes the queue; the thread drains and exits, dropping the sink
        match self.handle.take().map(JoinHandle::join) {
            Some(Ok(result)) => result,
            _ => Err(io::Error::other("frame writer thread crashed")),
        }
    }
}

fn drain_stderr(mut stderr: impl Read + Send + 'static) -> Arc<Mutex<String>> {
    let tail = Arc::new(Mutex::new(String::new()));
    let shared = tail.clone();
    thread::spawn(move || {
        let mut buf = [0u8; 512];
        while let Ok(n) = stderr.read(&mut buf) {
            if n == 0 {
                break;
            }
            if let Ok(mut text) = shared.lock() {
                text.push_str(&String::from_utf8_lossy(&buf[..n]));
                let len = text.len();
                if len > STDERR_TAIL_BYTES {
                    let mut cut = len - STDERR_TAIL_BYTES;
                    while !text.is_char_boundary(cut) {
                        cut += 1;
                    }
                    text.drain(..cut);
                }
            }
        }
    });
    tail
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentReport {
    pub path: PathBuf,
    pub stats: WriterStats,
}

/// One FFmpeg process writing one fragmented-MP4 segment.
pub struct SegmentEncoder {
    path: PathBuf,
    child: Child,
    writer: FrameWriter,
    stderr_tail: Arc<Mutex<String>>,
}

impl SegmentEncoder {
    pub fn start(
        config: &EncoderConfig,
        path: PathBuf,
        width: u32,
        height: u32,
        pixel_format: PixelFormat,
    ) -> Result<Self, CaptureError> {
        let mut child = Command::new(&config.ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-nostats"])
            .args(["-f", "rawvideo", "-pix_fmt", pixel_format.ffmpeg_name()])
            .args(["-s", &format!("{width}x{height}")])
            .args(["-framerate", &config.fps.to_string(), "-i", "pipe:0", "-an"])
            .args(["-vf", "format=yuv420p", "-c:v", "libx264"])
            .args(["-preset", &config.preset])
            // No lookahead/B-frame buffering: frames leave the encoder at once.
            .args(["-tune", "zerolatency", "-crf", &config.crf.to_string()])
            .args(["-r", &config.fps.to_string(), "-g", &config.fps.to_string()])
            .args([
                "-movflags",
                "frag_keyframe+empty_moov+default_base_moof",
                "-y",
            ])
            .arg(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| unavailable(format!("cannot start FFmpeg: {e}")))?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stderr_tail = drain_stderr(child.stderr.take().expect("piped stderr"));
        let writer = FrameWriter::spawn(Box::new(stdin), config.fps, config.queue_frames);
        Ok(Self {
            path,
            child,
            writer,
            stderr_tail,
        })
    }

    pub fn submit(&mut self, frame: VideoFrame) -> Submit {
        self.writer.submit(frame)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Flushes the queue, closes FFmpeg's input and waits for the MP4 to be
    /// finalised.
    pub fn finish(mut self, total_slots: u64) -> Result<SegmentReport, CaptureError> {
        let written = self.writer.finish(total_slots);
        let deadline = Instant::now() + STOP_TIMEOUT;
        let status = loop {
            match self.child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                _ => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break None;
                }
            }
        };
        // Let the stderr reader catch the process's last words.
        thread::sleep(Duration::from_millis(20));
        let tail = self
            .stderr_tail
            .lock()
            .map(|t| t.clone())
            .unwrap_or_default();
        let ffmpeg_failed = status.map_or(true, |s| !s.success());
        if ffmpeg_failed {
            return Err(unavailable(format!(
                "FFmpeg failed while encoding video: {}",
                tail.trim()
            )));
        }
        let stats = written.map_err(|e| unavailable(format!("cannot write video frames: {e}")))?;
        Ok(SegmentReport {
            path: self.path,
            stats,
        })
    }

    /// Stops FFmpeg without waiting; used when capture is abandoned.
    pub fn abort(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn frame(pts_ms: u64) -> VideoFrame {
        VideoFrame::packed(
            vec![pts_ms as u8; 24],
            4,
            4,
            PixelFormat::Nv12,
            Duration::from_millis(pts_ms),
        )
    }

    /// Collects what the writer wrote, optionally slowly.
    struct SlowSink {
        delay: Duration,
        writes: mpsc::Sender<usize>,
    }
    impl Write for SlowSink {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            thread::sleep(self.delay);
            let _ = self.writes.send(buf.len());
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn slot_of_rounds_to_the_nearest_slot() {
        assert_eq!(slot_of(Duration::from_millis(0), 30), 0);
        assert_eq!(slot_of(Duration::from_millis(33), 30), 1);
        assert_eq!(slot_of(Duration::from_millis(1000), 30), 30);
    }

    #[test]
    fn backpressure_drops_without_blocking_and_keeps_the_timeline() {
        let (tx, rx) = mpsc::channel();
        let sink = SlowSink {
            delay: Duration::from_millis(15),
            writes: tx,
        };
        let mut writer = FrameWriter::spawn(Box::new(sink), 30, 4);
        let mut slowest_submit = Duration::ZERO;
        let mut dropped_seen = 0;
        // 90 frames offered back to back, far faster than the sink drains.
        for i in 0..90u64 {
            let started = Instant::now();
            if writer.submit(frame(i * 1000 / 30)) == Submit::Dropped {
                dropped_seen += 1;
            }
            slowest_submit = slowest_submit.max(started.elapsed());
        }
        assert!(dropped_seen > 0, "a slow sink must cause drops");
        assert!(
            slowest_submit < Duration::from_millis(10),
            "submit blocked for {slowest_submit:?}"
        );
        let stats = writer.finish(90).unwrap();
        assert_eq!(stats.frames_dropped, dropped_seen);
        // Dropped frames are repeated, so the segment still has every slot.
        assert_eq!(stats.frames_written, 90);
        assert!(stats.frames_filled > 0);
        assert_eq!(rx.try_iter().count(), 90);
    }

    #[test]
    fn queue_is_bounded_while_the_sink_is_stalled() {
        let (tx, _rx) = mpsc::channel();
        let sink = SlowSink {
            delay: Duration::from_millis(200),
            writes: tx,
        };
        let mut writer = FrameWriter::spawn(Box::new(sink), 30, 3);
        let queued = (0..1000u64)
            .filter(|i| writer.submit(frame(i * 33)) == Submit::Queued)
            .count();
        // At most the queue plus the frame being written can be accepted.
        assert!(queued <= 3 + 2, "{queued} frames were buffered");
        assert!(writer.dropped() >= 990);
    }

    #[test]
    fn frames_of_a_different_shape_are_rejected() {
        let (tx, _rx) = mpsc::channel();
        let sink = SlowSink {
            delay: Duration::ZERO,
            writes: tx,
        };
        let mut writer = FrameWriter::spawn(Box::new(sink), 30, 4);
        assert_eq!(writer.submit(frame(0)), Submit::Queued);
        let bigger = VideoFrame::packed(vec![0; 54], 6, 6, PixelFormat::Nv12, Duration::ZERO);
        assert_eq!(writer.submit(bigger), Submit::WrongShape);
        writer.finish(1).unwrap();
    }

    #[test]
    fn broken_sink_reports_closed_and_an_error() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::from(io::ErrorKind::BrokenPipe))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut writer = FrameWriter::spawn(Box::new(Broken), 30, 2);
        let _ = writer.submit(frame(0));
        thread::sleep(Duration::from_millis(50));
        assert_eq!(writer.submit(frame(33)), Submit::Closed);
        assert!(writer.finish(2).is_err());
    }
}
