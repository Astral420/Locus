//! The single media clock shared by video and every audio source
//! (SPEC l.229; NATIVE_CAPTURE_PLAN §6 item 1).
//!
//! Origin: the instant the first source delivers its first sample. Every later
//! timestamp is `instant - origin - paused time`, so pauses never appear in
//! output and all sources agree on one timeline. The clock takes explicit
//! `Instant`s so adapters can stamp a sample with the time it was captured, and
//! tests can drive it deterministically.

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

#[derive(Debug, Default)]
struct State {
    origin: Option<Instant>,
    paused_since: Option<Instant>,
    /// Closed pause intervals, oldest first.
    pauses: Vec<(Instant, Instant)>,
    /// Media-time offset of each source's first sample.
    offsets: Vec<(String, Duration)>,
}

impl State {
    fn media_time(&self, at: Instant) -> Duration {
        let Some(origin) = self.origin else {
            return Duration::ZERO;
        };
        let mut paused = Duration::ZERO;
        for &(start, end) in &self.pauses {
            if at > start {
                paused += at.min(end).saturating_duration_since(start);
            }
        }
        if let Some(start) = self.paused_since {
            if at > start {
                paused += at.saturating_duration_since(start);
            }
        }
        at.saturating_duration_since(origin).saturating_sub(paused)
    }
}

#[derive(Debug, Default)]
pub struct MediaClock {
    state: Mutex<State>,
}

impl MediaClock {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Media time of a sample captured at `at`. The first call anchors the
    /// origin, so the first sample of the first source is time zero. Samples
    /// captured before the origin clamp to zero.
    pub fn stamp(&self, at: Instant) -> Duration {
        let mut state = self.lock();
        if state.origin.is_none() {
            state.origin = Some(at);
        }
        state.media_time(at)
    }

    /// Current media time; zero until a sample has anchored the origin. Frozen
    /// while paused.
    pub fn now(&self) -> Duration {
        self.lock().media_time(Instant::now())
    }

    pub fn is_anchored(&self) -> bool {
        self.lock().origin.is_some()
    }

    pub fn pause(&self) {
        self.pause_at(Instant::now());
    }

    pub fn resume(&self) {
        self.resume_at(Instant::now());
    }

    pub fn pause_at(&self, at: Instant) {
        let mut state = self.lock();
        if state.origin.is_some() && state.paused_since.is_none() {
            state.paused_since = Some(at);
        }
    }

    pub fn resume_at(&self, at: Instant) {
        let mut state = self.lock();
        if let Some(start) = state.paused_since.take() {
            state.pauses.push((start, at.max(start)));
        }
    }

    pub fn is_paused(&self) -> bool {
        self.lock().paused_since.is_some()
    }

    /// Stamps a source's first sample and records its offset in the manifest
    /// data (`source_offsets`). Returns the offset.
    pub fn register_source(&self, name: &str, first_sample_at: Instant) -> Duration {
        let offset = self.stamp(first_sample_at);
        self.lock().offsets.push((name.to_string(), offset));
        offset
    }

    pub fn source_offsets(&self) -> Vec<(String, Duration)> {
        self.lock().offsets.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn first_sample_is_time_zero() {
        let clock = MediaClock::new();
        assert!(!clock.is_anchored());
        let t0 = Instant::now();
        assert_eq!(clock.stamp(t0), Duration::ZERO);
        assert_eq!(clock.stamp(t0 + ms(250)), ms(250));
    }

    #[test]
    fn pauses_are_removed_from_later_samples() {
        let clock = MediaClock::new();
        let t0 = Instant::now();
        clock.stamp(t0);
        clock.pause_at(t0 + ms(1000));
        clock.resume_at(t0 + ms(6000)); // 5 s pause
        assert_eq!(clock.stamp(t0 + ms(1000)), ms(1000));
        assert_eq!(clock.stamp(t0 + ms(6000)), ms(1000));
        assert_eq!(clock.stamp(t0 + ms(7000)), ms(2000));
        // A sample that falls inside the pause maps to the pause start.
        assert_eq!(clock.stamp(t0 + ms(3000)), ms(1000));
    }

    #[test]
    fn several_pauses_accumulate() {
        let clock = MediaClock::new();
        let t0 = Instant::now();
        clock.stamp(t0);
        clock.pause_at(t0 + ms(100));
        clock.resume_at(t0 + ms(200));
        clock.pause_at(t0 + ms(300));
        clock.resume_at(t0 + ms(500));
        assert_eq!(clock.stamp(t0 + ms(600)), ms(300));
    }

    #[test]
    fn open_pause_freezes_time_and_double_pause_is_ignored() {
        let clock = MediaClock::new();
        let t0 = Instant::now();
        clock.stamp(t0);
        clock.pause_at(t0 + ms(100));
        clock.pause_at(t0 + ms(400)); // ignored
        assert!(clock.is_paused());
        assert_eq!(clock.stamp(t0 + ms(900)), ms(100));
    }

    #[test]
    fn source_offsets_are_recorded_against_one_origin() {
        let clock = MediaClock::new();
        let t0 = Instant::now();
        assert_eq!(clock.register_source("video", t0), Duration::ZERO);
        assert_eq!(clock.register_source("microphone", t0 + ms(40)), ms(40));
        let offsets = clock.source_offsets();
        assert_eq!(offsets.len(), 2);
        assert_eq!(offsets[1], ("microphone".to_string(), ms(40)));
    }

    #[test]
    fn pause_before_any_sample_is_a_no_op() {
        let clock = MediaClock::new();
        clock.pause();
        assert!(!clock.is_paused());
        assert_eq!(clock.now(), Duration::ZERO);
    }
}
