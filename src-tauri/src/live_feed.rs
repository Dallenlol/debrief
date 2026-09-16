//! What the Meeting page shows while a recording is still running: the
//! captions so far and running notes the language model keeps updating.

use crate::events;
use crate::recorder::notice;
use crate::state::AppState;
use hark_asr::Caption;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// Update the notes this soon after a new committed line when something changed...
const SOON: Duration = Duration::from_secs(45);
/// ...and no later than this while new lines keep arriving.
const AT_MOST: Duration = Duration::from_secs(120);
/// A pass slower than this doubles the wait (CPU is busy with captions too).
const SLOW_PASS: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct LiveFeed {
    pub meeting_id: String,
    pub finals: Vec<Caption>,
    pub partial: Option<Caption>,
    pub notes: String,
    pub notes_updated_ms: Option<u64>,
    /// Index into `finals` the notes were last computed from.
    notes_through: usize,
    started: Option<Instant>,
}

#[derive(Serialize, Clone)]
pub struct LiveSnapshot {
    pub meeting_id: String,
    pub finals: Vec<Caption>,
    pub partial: Option<Caption>,
    pub notes: String,
    pub notes_updated_ms: Option<u64>,
}

#[derive(Serialize, Clone)]
pub struct LiveNotesPayload {
    pub meeting_id: String,
    pub notes: String,
    pub updated_ms: u64,
}

impl LiveFeed {
    pub fn begin(state: &AppState, meeting_id: &str) {
        *state.live_feed.lock() = Some(LiveFeed { meeting_id: meeting_id.to_string(), started: Some(Instant::now()), ..Default::default() });
    }

    pub fn end(state: &AppState) {
        *state.live_feed.lock() = None;
    }

    pub fn snapshot(state: &AppState) -> Option<LiveSnapshot> {
        state.live_feed.lock().as_ref().map(|f| LiveSnapshot {
            meeting_id: f.meeting_id.clone(),
            finals: f.finals.clone(),
            partial: f.partial.clone(),
            notes: f.notes.clone(),
            notes_updated_ms: f.notes_updated_ms,
        })
    }

    /// Record a caption from the live transcriber (called for every emitted caption).
    pub fn push(state: &AppState, c: &Caption) {
        let mut g = state.live_feed.lock();
        let Some(f) = g.as_mut() else { return };
        if c.is_final {
            f.partial = None;
            f.finals.push(c.clone());
        } else {
            f.partial = Some(c.clone());
        }
    }
}

/// Keep the running notes fresh until the feed ends. Runs on its own thread;
/// yields when there is no language model or nothing new was said.
pub fn spawn_notes_loop(app: AppHandle, meeting_id: String, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let mut last_run: Option<Instant> = None;
        let mut wait = SOON;
        let mut warned = false;
        loop {
            std::thread::sleep(Duration::from_secs(3));
            if stop.load(Ordering::Relaxed) {
                return;
            }
            let (new_text, through, elapsed_ms) = {
                let g = state.live_feed.lock();
                let Some(f) = g.as_ref().filter(|f| f.meeting_id == meeting_id) else { return };
                if f.finals.len() <= f.notes_through {
                    continue;
                }
                let newest_end = f.finals.last().map(|c| c.end_ms).unwrap_or(0);
                let since = last_run.map(|t| t.elapsed()).unwrap_or(Duration::MAX);
                // Fresh lines and a pause since the last pass, or the cap; whichever first.
                let due = since >= AT_MOST || (since >= wait && newest_end > 0);
                if !due {
                    continue;
                }
                let text: Vec<String> = f.finals[f.notes_through..].iter().map(|c| format!("[{}] {}", stamp(c.start_ms), c.text)).collect();
                (text.join("\n"), f.finals.len(), f.started.map(|s| s.elapsed().as_millis() as u64).unwrap_or(0))
            };
            let Some(backend) = state.llm() else {
                if !warned {
                    warned = true;
                    notice(&app, "info", "Live notes need a language model; download one in Settings.".into());
                }
                continue;
            };
            let previous = state.live_feed.lock().as_ref().map(|f| f.notes.clone()).unwrap_or_default();
            let _busy = state.busy_guard();
            let t0 = Instant::now();
            match hark_llm::live_notes::update(backend.as_ref(), &previous, &new_text) {
                Ok(notes) => {
                    let mut g = state.live_feed.lock();
                    if let Some(f) = g.as_mut().filter(|f| f.meeting_id == meeting_id) {
                        f.notes = notes.clone();
                        f.notes_through = through;
                        f.notes_updated_ms = Some(elapsed_ms);
                    }
                    let _ = app.emit(events::LIVE_NOTES, LiveNotesPayload { meeting_id: meeting_id.clone(), notes, updated_ms: elapsed_ms });
                }
                Err(e) => log::warn!("live notes: {e}"),
            }
            last_run = Some(Instant::now());
            wait = if t0.elapsed() > SLOW_PASS { (wait * 2).min(AT_MOST) } else { SOON };
        }
    });
}

fn stamp(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}
