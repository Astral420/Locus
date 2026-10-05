//! The boundary every platform video adapter implements
//! (NATIVE_CAPTURE_PLAN §4.1 `ScreenSource`).

use super::{clock::MediaClock, frame::VideoFrame, CaptureError};
use serde::{Deserialize, Serialize};
use std::{
    sync::{mpsc::SyncSender, Arc},
    time::Instant,
};

/// What to capture (FR1.3: full screen or a specific window). Chosen in the
/// frontend picker and sent with the start request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum ScreenTarget {
    PrimaryDisplay,
    Display(u32),
    Window(u64),
}

/// One entry of the source list the picker shows (Tauri command contract).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenSourceInfo {
    pub target: ScreenTarget,
    /// Display name, or "<app> - <window title>" for windows.
    pub title: String,
    pub width: u32,
    pub height: u32,
}

/// Implemented by the ScreenCaptureKit, Windows Graphics Capture and PipeWire
/// adapters and by the synthetic test source. Each adapter owns its OS threads.
///
/// Contract:
/// - Frames are stamped with `clock.stamp(<instant the frame was captured>)`.
/// - Frames are sent with `try_send` and dropped when the queue is full; the OS
///   capture callback must never block.
/// - While paused, no frames are sent. A source that only reports changes
///   (idle screens) may send none after `resume`; the pacer repeats the last one.
pub trait ScreenSource: Send {
    /// Starts capture and returns once the first frame exists, giving the
    /// instant that frame was captured.
    fn start(
        &mut self,
        target: &ScreenTarget,
        clock: Arc<MediaClock>,
        frames: SyncSender<VideoFrame>,
    ) -> Result<Instant, CaptureError>;
    fn pause(&mut self);
    fn resume(&mut self) -> Result<(), CaptureError>;
    fn stop(&mut self);
    /// Set once the OS ended the capture on its own (permission revoked,
    /// display unplugged, window closed). The pipeline then reports the
    /// recording as dead instead of silently producing a short video.
    fn failure(&self) -> Option<String> {
        None
    }
}

#[cfg(test)]
pub mod synthetic {
    //! Deterministic stand-in for a real display: a moving gradient at a chosen
    //! rate, or a static screen that reports only its first frame.

    use super::*;
    use crate::capture::frame::PixelFormat;
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        thread::{self, JoinHandle},
        time::Duration,
    };

    pub struct SyntheticSource {
        pub width: u32,
        pub height: u32,
        /// Frames per second the "OS" reports; 0 means only the first frame.
        pub emit_fps: u32,
        paused: Arc<AtomicBool>,
        running: Arc<AtomicBool>,
        handle: Option<JoinHandle<()>>,
    }

    impl SyntheticSource {
        pub fn new(width: u32, height: u32, emit_fps: u32) -> Self {
            Self {
                width,
                height,
                emit_fps,
                paused: Arc::new(AtomicBool::new(false)),
                running: Arc::new(AtomicBool::new(false)),
                handle: None,
            }
        }
    }

    fn make_frame(width: u32, height: u32, n: u64, pts: Duration) -> VideoFrame {
        let len = PixelFormat::Nv12.packed_len(width, height);
        VideoFrame::packed(
            (0..len).map(|i| ((i as u64 + n * 3) % 251) as u8).collect(),
            width,
            height,
            PixelFormat::Nv12,
            pts,
        )
    }

    impl ScreenSource for SyntheticSource {
        fn start(
            &mut self,
            _target: &ScreenTarget,
            clock: Arc<MediaClock>,
            frames: SyncSender<VideoFrame>,
        ) -> Result<Instant, CaptureError> {
            let first_at = Instant::now();
            let pts = clock.register_source("video", first_at);
            let _ = frames.try_send(make_frame(self.width, self.height, 0, pts));
            self.running.store(true, Ordering::SeqCst);
            if self.emit_fps > 0 {
                let (running, paused) = (self.running.clone(), self.paused.clone());
                let (w, h, fps) = (self.width, self.height, self.emit_fps);
                self.handle = Some(thread::spawn(move || {
                    let mut n = 1u64;
                    while running.load(Ordering::SeqCst) {
                        thread::sleep(Duration::from_micros(1_000_000 / u64::from(fps)));
                        if paused.load(Ordering::SeqCst) {
                            continue;
                        }
                        let pts = clock.stamp(Instant::now());
                        let _ = frames.try_send(make_frame(w, h, n, pts));
                        n += 1;
                    }
                }));
            }
            Ok(first_at)
        }

        fn pause(&mut self) {
            self.paused.store(true, Ordering::SeqCst);
        }

        fn resume(&mut self) -> Result<(), CaptureError> {
            self.paused.store(false, Ordering::SeqCst);
            Ok(())
        }

        fn stop(&mut self) {
            self.running.store(false, Ordering::SeqCst);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }
}

#[cfg(test)]
mod serde_tests {
    use super::*;

    #[test]
    fn screen_target_json_matches_the_frontend_contract() {
        let cases = [
            (
                ScreenTarget::PrimaryDisplay,
                r#"{"kind":"primary_display"}"#,
            ),
            (ScreenTarget::Display(3), r#"{"kind":"display","id":3}"#),
            (ScreenTarget::Window(42), r#"{"kind":"window","id":42}"#),
        ];
        for (target, json) in cases {
            assert_eq!(serde_json::to_string(&target).unwrap(), json);
            assert_eq!(serde_json::from_str::<ScreenTarget>(json).unwrap(), target);
        }
    }

    #[test]
    fn source_list_entries_serialize_with_their_target() {
        let info = ScreenSourceInfo {
            target: ScreenTarget::Display(1),
            title: "Display 1".into(),
            width: 1440,
            height: 900,
        };
        let value = serde_json::to_value(&info).unwrap();
        assert_eq!(value["target"]["kind"], "display");
        assert_eq!(value["width"], 1440);
    }
}
