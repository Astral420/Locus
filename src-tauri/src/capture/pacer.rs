//! Constant-frame-rate pacing (NATIVE_CAPTURE_PLAN §4.1 `FramePacer`).
//!
//! Capture APIs deliver frames only when the screen changes (ScreenCaptureKit
//! even reports idle frames), so the encoder cannot be fed "whatever arrives".
//! The pacer lays fixed slots `anchor + i / fps` on the media timeline and, for
//! each slot, emits the newest frame captured at or before it: repeating the
//! last frame over an idle screen and dropping extras when frames arrive faster
//! than the target rate. Slot times are computed from the index (not by adding
//! a rounded interval), so a long run cannot drift.

use super::frame::VideoFrame;
use std::{collections::VecDeque, time::Duration};

/// Frames that arrive faster than the pacer ticks are bounded: the oldest is
/// discarded first.
const MAX_PENDING: usize = 64;

#[derive(Debug)]
pub struct FramePacer {
    fps: u32,
    anchor: Option<Duration>,
    next_slot: u64,
    pending: VecDeque<VideoFrame>,
    current: Option<VideoFrame>,
}

impl FramePacer {
    pub fn new(fps: u32) -> Self {
        assert!(fps > 0, "fps must be positive");
        Self {
            fps,
            anchor: None,
            next_slot: 0,
            pending: VecDeque::new(),
            current: None,
        }
    }

    pub fn fps(&self) -> u32 {
        self.fps
    }

    fn slot_time(&self, slot: u64) -> Duration {
        let offset_ns = u128::from(slot) * 1_000_000_000 / u128::from(self.fps);
        self.anchor.unwrap_or_default() + Duration::from_nanos(offset_ns as u64)
    }

    /// Queues a captured frame. The first frame ever pushed anchors slot 0.
    pub fn push(&mut self, frame: VideoFrame) {
        if self.anchor.is_none() {
            self.anchor = Some(frame.pts);
        }
        if self.pending.len() >= MAX_PENDING {
            // Keep the discarded picture as the "current" one so early slots
            // still have something to show.
            self.current = self.pending.pop_front();
        }
        self.pending.push_back(frame);
    }

    /// Starts a new run of slots at `anchor` (after a pause). The last picture
    /// is kept so slots before the first post-resume frame repeat it.
    pub fn restart(&mut self, anchor: Duration) {
        self.pending.clear();
        self.anchor = Some(anchor);
        self.next_slot = 0;
    }

    /// Emits every slot strictly before `end`, each stamped with its slot time.
    pub fn emit_until(&mut self, end: Duration) -> Vec<VideoFrame> {
        let mut out = Vec::new();
        if self.anchor.is_none() {
            return out;
        }
        loop {
            let slot_time = self.slot_time(self.next_slot);
            if slot_time >= end {
                break;
            }
            while self
                .pending
                .front()
                .is_some_and(|frame| frame.pts <= slot_time)
            {
                self.current = self.pending.pop_front();
            }
            let Some(current) = &self.current else {
                break; // nothing captured yet; try again on the next tick
            };
            out.push(current.with_pts(slot_time));
            self.next_slot += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::frame::PixelFormat;

    fn frame(tag: u8, pts_ms: u64) -> VideoFrame {
        VideoFrame::packed(
            vec![tag; 24],
            4,
            4,
            PixelFormat::Nv12,
            Duration::from_millis(pts_ms),
        )
    }

    fn tag(frame: &VideoFrame) -> u8 {
        frame.data[0]
    }

    #[test]
    fn idle_screen_repeats_the_last_frame_to_fill_slots() {
        let mut pacer = FramePacer::new(30);
        pacer.push(frame(7, 0)); // one frame, then the screen never changes
        let out = pacer.emit_until(Duration::from_secs(10));
        assert_eq!(out.len(), 300);
        assert!(out.iter().all(|f| tag(f) == 7));
    }

    #[test]
    fn fast_source_is_decimated_to_the_target_rate() {
        let mut pacer = FramePacer::new(30);
        // 120 fps source for 2 s.
        for i in 0..240u64 {
            pacer.push(frame((i % 250) as u8, i * 1000 / 120));
        }
        let out = pacer.emit_until(Duration::from_secs(2));
        assert_eq!(out.len(), 60);
    }

    #[test]
    fn long_run_has_no_drift() {
        let mut pacer = FramePacer::new(30);
        pacer.push(frame(1, 0));
        let mut total = 0;
        // Tick every 7 ms (not a multiple of the slot interval) for 3 hours.
        let mut t = 0u64;
        while t < 3 * 3600 * 1000 {
            t += 7;
            total += pacer.emit_until(Duration::from_millis(t)).len();
        }
        let expected = (t as f64 / 1000.0 * 30.0).ceil() as i64;
        assert!(
            (total as i64 - expected).abs() <= 1,
            "{total} vs {expected}"
        );
    }

    #[test]
    fn slots_are_stamped_on_the_grid_and_use_the_newest_frame_at_or_before_them() {
        let mut pacer = FramePacer::new(10); // 100 ms slots
        pacer.push(frame(1, 0));
        pacer.push(frame(2, 120));
        let out = pacer.emit_until(Duration::from_millis(300));
        let got: Vec<(u8, u128)> = out.iter().map(|f| (tag(f), f.pts.as_millis())).collect();
        assert_eq!(got, vec![(1, 0), (1, 100), (2, 200)]);
    }

    #[test]
    fn nothing_is_emitted_before_the_first_frame() {
        let mut pacer = FramePacer::new(30);
        assert!(pacer.emit_until(Duration::from_secs(1)).is_empty());
    }

    #[test]
    fn restart_after_pause_continues_without_a_gap() {
        let mut pacer = FramePacer::new(10);
        pacer.push(frame(1, 0));
        assert_eq!(pacer.emit_until(Duration::from_millis(1000)).len(), 10);
        // Paused; media time stays at 1000 ms. Resume re-anchors there.
        pacer.restart(Duration::from_millis(1000));
        // First post-resume frame is late (50 ms): the old picture covers slot 0.
        pacer.push(frame(2, 1050));
        let out = pacer.emit_until(Duration::from_millis(1300));
        let got: Vec<(u8, u128)> = out.iter().map(|f| (tag(f), f.pts.as_millis())).collect();
        assert_eq!(got, vec![(1, 1000), (2, 1100), (2, 1200)]);
    }

    #[test]
    fn pending_queue_is_bounded() {
        let mut pacer = FramePacer::new(30);
        for i in 0..10_000u64 {
            pacer.push(frame(1, i));
        }
        assert!(pacer.pending.len() <= MAX_PENDING);
    }
}
