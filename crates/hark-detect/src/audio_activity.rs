use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Fallback detector: "someone is talking both ways". Returns true once both
/// mic and system audio have been above `threshold_db` continuously (per
/// sample) for at least `min_active`, looking back at most `window`.
pub struct AudioActivity {
    threshold_db: f32,
    min_active: Duration,
    window: Duration,
    samples: VecDeque<(Instant, bool)>,
}

impl AudioActivity {
    pub fn new(threshold_db: f32, min_active: Duration, window: Duration) -> Self {
        AudioActivity { threshold_db, min_active, window, samples: VecDeque::new() }
    }

    pub fn push(&mut self, now: Instant, mic_db: f32, sys_db: f32) -> bool {
        let active = mic_db > self.threshold_db && sys_db > self.threshold_db;
        self.samples.push_back((now, active));
        while let Some((t, _)) = self.samples.front() {
            if now.saturating_duration_since(*t) > self.window {
                self.samples.pop_front();
            } else {
                break;
            }
        }
        // Longest trailing run of active samples must span >= min_active.
        let mut run_start: Option<Instant> = None;
        for (t, a) in self.samples.iter().rev() {
            if *a {
                run_start = Some(*t);
            } else {
                break;
            }
        }
        match run_start {
            Some(start) => now.saturating_duration_since(start) >= self.min_active,
            None => false,
        }
    }

    pub fn reset(&mut self) {
        self.samples.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_activity_requires_both_sides_for_min_duration() {
        let mut a = AudioActivity::new(-45.0, Duration::from_secs(10), Duration::from_secs(20));
        let t0 = Instant::now();
        for i in 0..5 {
            assert!(!a.push(t0 + Duration::from_secs(i * 2), -20.0, -20.0)); // 0..8 s
        }
        assert!(a.push(t0 + Duration::from_secs(10), -20.0, -20.0));

        let mut b = AudioActivity::new(-45.0, Duration::from_secs(10), Duration::from_secs(20));
        for i in 0..8 {
            assert!(!b.push(t0 + Duration::from_secs(i * 2), -20.0, -80.0)); // mic only
        }
    }

    #[test]
    fn silence_breaks_the_run() {
        let mut a = AudioActivity::new(-45.0, Duration::from_secs(10), Duration::from_secs(20));
        let t0 = Instant::now();
        for i in 0..4 {
            a.push(t0 + Duration::from_secs(i * 2), -20.0, -20.0);
        }
        a.push(t0 + Duration::from_secs(8), -90.0, -90.0);
        assert!(!a.push(t0 + Duration::from_secs(10), -20.0, -20.0));
    }
}
