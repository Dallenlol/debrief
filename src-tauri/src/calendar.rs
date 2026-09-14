//! Calendar context: user-configured .ics sources (private iCal URLs from
//! Google/Outlook, or local files) refreshed in the background, used to name
//! recordings and seed attendees. Only the URLs the user enters are fetched.

use crate::state::AppState;
use chrono::{Duration, Utc};
use hark_calendar::CalEvent;
use std::time::Duration as StdDuration;
use tauri::{AppHandle, Manager};

/// Fetch every configured source and replace the cached events. Returns the
/// per-source errors (empty when everything loaded).
pub fn refresh(app: &AppHandle) -> Vec<String> {
    let state = app.state::<AppState>();
    let sources = state.settings.read().calendar_sources.clone();
    let mut events: Vec<CalEvent> = Vec::new();
    let mut errors = Vec::new();
    for src in sources.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        match load_source(src) {
            Ok(text) => events.extend(hark_calendar::parse_ics(&text)),
            Err(e) => errors.push(format!("{src}: {e}")),
        }
    }
    events.sort_by_key(|e| e.start);
    events.dedup_by(|a, b| a.uid == b.uid && a.start == b.start);
    log::info!("calendar: {} events from {} source(s)", events.len(), sources.len());
    *state.calendar.write() = events;
    *state.calendar_errors.write() = errors.clone();
    errors
}

fn load_source(src: &str) -> Result<String, String> {
    if src.starts_with("http://") || src.starts_with("https://") || src.starts_with("webcal://") {
        let url = src.replacen("webcal://", "https://", 1);
        let client = reqwest::blocking::Client::builder()
            .user_agent(concat!("Hark/", env!("CARGO_PKG_VERSION")))
            .timeout(StdDuration::from_secs(20))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client.get(&url).send().map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        resp.text().map_err(|e| e.to_string())
    } else {
        std::fs::read_to_string(src).map_err(|e| e.to_string())
    }
}

/// Background refresher; re-reads the interval from settings each cycle.
pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || loop {
        let has_sources = {
            let st = app.state::<AppState>();
            let s = st.settings.read();
            s.calendar_sources.iter().any(|x| !x.trim().is_empty())
        };
        if has_sources {
            refresh(&app);
        }
        let mins = app.state::<AppState>().settings.read().calendar_refresh_min.clamp(1, 24 * 60);
        std::thread::sleep(StdDuration::from_secs(mins * 60));
    });
}

/// The event you are most likely in right now.
pub fn current_event(state: &AppState) -> Option<CalEvent> {
    hark_calendar::best_match(&state.calendar.read(), Utc::now()).cloned()
}

/// Events from 10 minutes ago to `hours` ahead.
pub fn upcoming(state: &AppState, hours: i64) -> Vec<CalEvent> {
    hark_calendar::events_around(&state.calendar.read(), Utc::now(), Duration::minutes(10), Duration::hours(hours))
}
