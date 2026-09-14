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
    let emit = &emit;
    emit("summary", 0.9, None);
    let Some(backend) = state.llm() else {
        notice(app, "info", "Summary skipped: no language model available. Download one in Settings or connect an endpoint.".into());
        let _ = state.store.set_meeting_error(meeting_id, Some(crate::pipeline::NO_LLM));
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
    let mut participants: Vec<String> = meeting.participants.clone();
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
    // Auto-title from the opening minutes, unless the user already named the meeting.
    if meeting.title_auto {
        let app_label = meeting.app.as_deref().map(app_label);
        match hark_llm::title::generate(backend.as_ref(), &vars.transcript, app_label) {
            Ok(Some(t)) => {
                let _ = state.store.set_title(meeting_id, &t, false);
                emit("title", 0.9, None);
            }
            Ok(None) => {}
            Err(e) => log::warn!("auto-title failed: {e}"),
        }
    }

    // ~24k chars (~6k tokens) per model call keeps 8k-context models safe; longer
    // transcripts are condensed block by block first (see hark_llm::chunked).
    let emit_p = emit.clone();
    let progress = move |done: usize, total: usize| emit_p("summary", 0.9 + 0.08 * (done as f32 / total.max(1) as f32), None);
    match hark_llm::summary::generate_with_progress(backend.as_ref(), &template.body, &vars, 24_000, progress) {
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
            let _ = state.store.set_meeting_error(meeting_id, Some(&format!("summary: {e}")));
            emit("done", 1.0, Some(e.to_string()));
        }
    }
}

/// Human label for a detected app id ("zoom" -> "Zoom").
fn app_label(app: &str) -> &str {
    match app {
        "zoom" => "Zoom",
        "teams" => "Microsoft Teams",
        "meet" => "Google Meet",
        "webex" => "Webex",
        "discord" => "Discord",
        "slack" => "Slack",
        "facetime" => "FaceTime",
        other => other,
    }
}
