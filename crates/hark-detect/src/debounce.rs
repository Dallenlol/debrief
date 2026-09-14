use crate::Detected;
use std::time::{Duration, Instant};

/// Emits a detection once, and again only after the same app has been gone
/// for at least `reemit_after`. A different app is a new candidate.
pub struct Debouncer {
    reemit_after: Duration,
    /// App we last emitted for, and when we last saw it.
    current: Option<(String, Instant)>,
}

impl Debouncer {
    pub fn new(reemit_after: Duration) -> Self {
        Debouncer { reemit_after, current: None }
    }

    /// Feed the current observation. Returns `Some` when the caller should show the popup.
    pub fn observe(&mut self, now: Instant, det: Option<&Detected>) -> Option<Detected> {
        match det {
            None => {
                // Keep `current` so a brief disappearance doesn't re-trigger; it
                // expires naturally once `reemit_after` has elapsed since last_seen.
                None
            }
            Some(d) => {
                let same_recent = match &self.current {
                    Some((app, last_seen)) => {
                        app == &d.app && now.saturating_duration_since(*last_seen) < self.reemit_after
                    }
                    None => false,
                };
                self.current = Some((d.app.clone(), now));
                if same_recent {
                    None
                } else {
                    Some(d.clone())
                }
            }
        }
    }

    /// Forget everything (e.g. after a recording ends) so the next sighting emits again.
    pub fn reset(&mut self) {
        self.current = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det(app: &str) -> Detected {
        Detected { app: app.into(), label: app.into(), title: "t".into(), confidence: 1.0 }
    }

    #[test]
    fn debouncer_emits_once_until_gone_for_reemit_window() {
        let mut d = Debouncer::new(Duration::from_secs(60));
        let t0 = Instant::now();
        let z = det("zoom");
        assert!(d.observe(t0, Some(&z)).is_some());
        assert!(d.observe(t0 + Duration::from_secs(2), Some(&z)).is_none());
        assert!(d.observe(t0 + Duration::from_secs(10), None).is_none());
        // Came back within 60 s of last sighting: no re-emit.
        assert!(d.observe(t0 + Duration::from_secs(30), Some(&z)).is_none());
        assert!(d.observe(t0 + Duration::from_secs(40), None).is_none());
        // Gone >= 60 s (last seen at 30 s), back at 110 s: emit again.
        assert!(d.observe(t0 + Duration::from_secs(110), Some(&z)).is_some());
    }

    #[test]
    fn different_app_emits_immediately_and_reset_works() {
        let mut d = Debouncer::new(Duration::from_secs(60));
        let t0 = Instant::now();
        assert!(d.observe(t0, Some(&det("zoom"))).is_some());
        assert!(d.observe(t0 + Duration::from_secs(1), Some(&det("teams"))).is_some());
        assert!(d.observe(t0 + Duration::from_secs(2), Some(&det("teams"))).is_none());
        d.reset();
        assert!(d.observe(t0 + Duration::from_secs(3), Some(&det("teams"))).is_some());
    }
}
