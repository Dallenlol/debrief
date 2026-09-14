//! Summary generation stage (separate file to keep pipeline.rs focused).

use crate::events::{self, ProcessingPayload};
use crate::recorder::notice;
use crate::state::AppState;
use tauri::{AppHandle, Emitter, Manager};

/// Generate the summary for a meeting with `template_id` (or the default) and store it.
pub fn run_summary(app: &AppHandle, meeting_id: &str, template_id: Option<&str>) {
    let state = app.state::<AppState>();
    let settings = state.settings.read().clone();
    let emit = |stage: &'static str, progress: f32, error: Option<String>| {
        let _ = app.emit(events::PROCESSING, ProcessingPayload { meeting_id: meeting_id.to_string(), stage, progress, error });
    };
    emit("summary", 0.9, None);
    let Some(backend) = state.llm() else {
        notice(app, "info", "Summary skipped: no language model available. Download one in Settings or connect an endpoint.".into());
        emit("done", 1.0, None);
        return;
    };
    let tid = template_id.unwrap_or(settings.default_template_id.as_str());
    let template = match state.store.get_template(tid).ok().flatten().or_else(|| state.store.get_template("general").ok().flatten()) {
        Some(t) => t,
        None => {
            emit("done", 1.0, Some("no summary template".into()));
            return;
        }
    };
    let Ok(Some(meeting)) = state.store.get_meeting(meeting_id) else { return };
    let segs = state.store.segments(meeting_id).unwrap_or_default();
    let mut participants: Vec<String> = Vec::new();
    for s in &segs {
        if let Some(sp) = &s.speaker {
            if !participants.contains(sp) {
                participants.push(sp.clone());
            }
        }
    }
    let transcript = segs
        .iter()
        .map(|s| hark_llm::summary::transcript_line(s.start_ms, s.speaker.as_deref(), s.clean_text.as_deref().unwrap_or(&s.text)))
        .collect::<Vec<_>>()
        .join("\n");
    let highlights: Vec<u64> = state.store.get_setting(&format!("highlights:{meeting_id}")).ok().flatten().unwrap_or_default();
    let vars = hark_llm::summary::TemplateVars {
        title: meeting.title.clone(),
        date: meeting.started_at.with_timezone(&chrono::Local).format("%b %-d, %Y").to_string(),
        duration: hark_llm::summary::fmt_stamp(meeting.duration_ms),
        participants: if participants.is_empty() { "unknown".into() } else { participants.join(", ") },
        transcript,
        highlights: highlights.iter().map(|h| format!("[{}]", hark_llm::summary::fmt_stamp(*h as i64))).collect::<Vec<_>>().join(", "),
    };
    // fable: ~24k chars (~6k tokens) keeps 8k-context models safe; long meetings get head+tail.
    match hark_llm::summary::generate(backend.as_ref(), &template.body, &vars, 24_000) {
        Ok(r) => {
            let s = hark_store::Summary {
                meeting_id: meeting_id.to_string(),
                template_id: template.id.clone(),
                markdown: r.markdown,
                structured: serde_json::to_value(&r.structured).unwrap_or(serde_json::Value::Null),
                model: backend.name(),
                created_at: chrono::Utc::now(),
            };
            let _ = state.store.set_summary(&s);
            emit("done", 1.0, None);
        }
        Err(e) => {
            log::error!("summary failed: {e}");
            notice(app, "warning", format!("Summary failed: {e}"));
            emit("done", 1.0, Some(e.to_string()));
        }
    }
}
