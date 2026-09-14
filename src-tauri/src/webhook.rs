//! Optional outbound webhook: POST the finished meeting (summary + transcript)
//! to a URL the user configured. Off by default; never retried.

use crate::state::AppState;
use serde::Serialize;
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
struct SegmentOut<'a> {
    start_ms: i64,
    end_ms: i64,
    speaker: Option<&'a str>,
    text: &'a str,
}

#[derive(Serialize)]
struct Payload<'a> {
    event: &'a str,
    sent_at: chrono::DateTime<chrono::Utc>,
    meeting: Option<&'a hark_store::Meeting>,
    summary: Option<&'a hark_store::Summary>,
    transcript: Vec<SegmentOut<'a>>,
    participants: &'a [String],
}

/// Send `meeting.ready` for a meeting. Returns the HTTP status on success.
pub fn fire(app: &AppHandle, meeting_id: &str) -> Result<u16, String> {
    let state = app.state::<AppState>();
    let (enabled, url) = {
        let s = state.settings.read();
        (s.webhook_enabled, s.webhook_url.trim().to_string())
    };
    if !enabled || url.is_empty() {
        return Err("webhook disabled".into());
    }
    let meeting = state.store.get_meeting(meeting_id).map_err(|e| e.to_string())?.ok_or("meeting not found")?;
    let summary = state.store.get_summary(meeting_id).ok().flatten();
    let segs = state.store.segments(meeting_id).unwrap_or_default();
    let transcript: Vec<SegmentOut> = segs
        .iter()
        .map(|s| SegmentOut { start_ms: s.start_ms, end_ms: s.end_ms, speaker: s.speaker.as_deref(), text: s.clean_text.as_deref().unwrap_or(&s.text) })
        .collect();
    let payload = Payload { event: "meeting.ready", sent_at: chrono::Utc::now(), meeting: Some(&meeting), summary: summary.as_ref(), transcript, participants: &meeting.participants };
    post(&url, &payload)
}

/// Send a `test` event with no meeting.
pub fn test(url: &str) -> Result<u16, String> {
    let payload = Payload { event: "test", sent_at: chrono::Utc::now(), meeting: None, summary: None, transcript: Vec::new(), participants: &[] };
    post(url.trim(), &payload)
}

fn post<T: Serialize>(url: &str, body: &T) -> Result<u16, String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("webhook URL must start with http:// or https://".into());
    }
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("Hark/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.post(url).json(body).send().map_err(|e| e.to_string())?;
    let status = resp.status();
    if status.is_success() {
        Ok(status.as_u16())
    } else {
        Err(format!("HTTP {status}"))
    }
}
