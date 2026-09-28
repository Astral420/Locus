use std::time::{Duration, Instant};

pub const THREE_HOUR_WARNING_SECONDS: f64 = 3.0 * 60.0 * 60.0;

#[derive(Debug, Clone)]
pub struct MediaTimeline {
    started_at: Instant,
    paused_at: Option<Instant>,
    paused_total: Duration,
}

impl MediaTimeline {
    pub fn start() -> Self {
        Self {
            started_at: Instant::now(),
            paused_at: None,
            paused_total: Duration::ZERO,
        }
    }
    pub fn pause(&mut self) {
        if self.paused_at.is_none() {
            self.paused_at = Some(Instant::now());
        }
    }
    pub fn resume(&mut self) {
        if let Some(paused_at) = self.paused_at.take() {
            self.paused_total += paused_at.elapsed();
        }
    }
    pub fn elapsed(&self) -> Duration {
        let now = self.paused_at.unwrap_or_else(Instant::now);
        now.duration_since(self.started_at)
            .saturating_sub(self.paused_total)
    }
    pub fn warning_due(&self) -> bool {
        self.elapsed().as_secs_f64() >= THREE_HOUR_WARNING_SECONDS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paused_time_is_not_media_time() {
        let mut timeline = MediaTimeline::start();
        timeline.pause();
        std::thread::sleep(Duration::from_millis(5));
        let during_pause = timeline.elapsed();
        timeline.resume();
        assert!(timeline.elapsed() >= during_pause);
        assert!(timeline.elapsed() < Duration::from_secs(2));
        assert!(!timeline.warning_due());
    }
}
