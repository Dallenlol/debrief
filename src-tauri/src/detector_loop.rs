//! Background loop: every 2 s, look for a meeting app; on a new sighting emit
//! `detection` and show the popup. Never records on its own.

use crate::events::{self, DetectionPayload};
use crate::state::AppState;
use crate::windows;
use hark_detect::{AudioActivity, Debouncer, Detected, PatternSet};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const POLL: Duration = Duration::from_secs(2);
const AUDIO_PROBE_EVERY: Duration = Duration::from_secs(10);
const AUDIO_PROBE_LEN: Duration = Duration::from_millis(600);
const REEMIT_AFTER: Duration = Duration::from_secs(60);

pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("hark-detector".into())
        .spawn(move || run(app))
        .expect("spawn detector");
}

fn run(app: AppHandle) {
    let patterns = PatternSet::builtin();
    let mut debouncer = Debouncer::new(REEMIT_AFTER);
    let mut activity = AudioActivity::new(-45.0, Duration::from_secs(10), Duration::from_secs(30));
    let mut last_probe = Instant::now() - AUDIO_PROBE_EVERY;

    loop {
        std::thread::sleep(POLL);
        let state = app.state::<AppState>();
        let settings = state.settings.read().clone();
        if !settings.detection_enabled || state.detection_paused.load(Ordering::SeqCst) {
            debouncer.reset();
            activity.reset();
            continue;
        }
        if let Some(until) = *state.snooze_until.lock() {
            if Instant::now() < until {
                continue;
            }
            *state.snooze_until.lock() = None;
        }

        let now = Instant::now();
        let windows_now = hark_detect::list_visible_windows();
        let mut det = patterns.match_windows(&windows_now).filter(|d| !settings.never_apps.contains(&d.app));

        // Audio-activity fallback: only when no app matched, probed sparsely.
        if det.is_none() && settings.audio_activity_enabled && now.duration_since(last_probe) >= AUDIO_PROBE_EVERY {
            last_probe = now;
            let (mic, sys) = hark_capture::probe::sample_levels(
                settings.mic_device.as_deref(),
                settings.loopback_device.as_deref(),
                AUDIO_PROBE_LEN,
            );
            if activity.push(now, mic, sys) && !settings.never_apps.contains(&"unknown".to_string()) {
                det = Some(Detected {
                    app: "unknown".into(),
                    label: "Possible call".into(),
                    title: "Microphone and speakers are both active".into(),
                    confidence: 0.5,
                });
            }
        } else if det.is_some() {
            activity.reset();
        }

        if let Some(d) = debouncer.observe(now, det.as_ref()) {
            log::info!("meeting detected: {} ({})", d.label, d.title);
            let _ = app.emit(
                events::DETECTION,
                DetectionPayload { app: d.app.clone(), label: d.label.clone(), title: d.title.clone(), confidence: d.confidence },
            );
            windows::show_popup(&app);
        }
    }
}
