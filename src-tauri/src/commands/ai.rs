use super::{err, CmdResult};
use crate::events;
use crate::pipeline;
use crate::state::AppState;
use hark_llm::chat::{build_messages, parse_citations, Passage};
use hark_llm::{GenOptions, Role};
use hark_store::{Chat, ChatMessage, Summary, Template};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

// ---- templates ----

#[tauri::command]
pub fn list_templates(state: State<AppState>) -> CmdResult<Vec<Template>> {
    state.store.list_templates().map_err(err)
}

#[tauri::command]
pub fn save_template(state: State<AppState>, template: Template) -> CmdResult<Template> {
    let mut t = template;
    if t.name.trim().is_empty() {
        return Err("template needs a name".into());
    }
    if !t.body.contains("{{transcript}}") {
        return Err("template must contain {{transcript}}".into());
    }
    if t.id.trim().is_empty() {
        t.id = uuid::Uuid::new_v4().to_string();
    }
    // Only the seed can mark templates built-in.
    let existing = state.store.get_template(&t.id).map_err(err)?;
    t.builtin = existing.map(|e| e.builtin).unwrap_or(false);
    state.store.upsert_template(&t).map_err(err)?;
    Ok(t)
}

#[tauri::command]
pub fn delete_template(state: State<AppState>, id: String) -> CmdResult<()> {
    state.store.delete_template(&id).map_err(err)
}

// ---- summaries ----

#[tauri::command]
pub fn get_summary(state: State<AppState>, id: String) -> CmdResult<Option<Summary>> {
    state.store.get_summary(&id).map_err(err)
}

/// Generate (or regenerate) the summary in the background; emits `processing` with stage "summary".
#[tauri::command]
pub fn generate_summary(app: AppHandle, id: String, template_id: Option<String>) -> CmdResult<()> {
    let state = app.state::<AppState>();
    state.store.get_meeting(&id).map_err(err)?.ok_or("meeting not found")?;
    if state.llm().is_none() {
        return Err("No language model available. Download one in Settings or connect an endpoint.".into());
    }
    let app2 = app.clone();
    std::thread::spawn(move || pipeline::run_summary(&app2, &id, template_id.as_deref()));
    Ok(())
}

/// Draft a follow-up email from the summary (or the transcript when there is none).
#[tauri::command]
pub async fn draft_followup(app: AppHandle, id: String) -> CmdResult<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _busy = state.busy_guard();
        let backend = state.llm().ok_or("No language model available. Download one in Settings or connect an endpoint.")?;
        let meeting = state.store.get_meeting(&id).map_err(err)?.ok_or("meeting not found")?;
        let settings = state.settings.read().clone();
        let segs = state.store.segments(&id).map_err(err)?;
        let mut participants = meeting.participants.clone();
        for sp in segs.iter().filter_map(|s| s.speaker.as_ref()) {
            if !participants.contains(sp) {
                participants.push(sp.clone());
            }
        }
        let summary = state.store.get_summary(&id).map_err(err)?;
        let (notes, notes_are_transcript) = match summary {
            Some(s) if !s.markdown.trim().is_empty() => (s.markdown, false),
            _ => {
                let t = segs
                    .iter()
                    .map(|s| hark_llm::summary::transcript_line(s.start_ms, s.speaker.as_deref(), s.clean_text.as_deref().unwrap_or(&s.text)))
                    .collect::<Vec<_>>()
                    .join("\n");
                (t.chars().take(hark_llm::followup::TRANSCRIPT_FALLBACK_CHARS).collect(), true)
            }
        };
        if notes.trim().is_empty() {
            return Err("Nothing to write from yet: the meeting has no transcript.".into());
        }
        let input = hark_llm::followup::FollowupInput {
            title: &meeting.title,
            date: &meeting.started_at.with_timezone(&chrono::Local).format("%b %-d, %Y").to_string(),
            participants: &if participants.is_empty() { "unknown".to_string() } else { participants.join(", ") },
            notes: &notes,
            notes_are_transcript,
            sender: if settings.user_name.trim().eq_ignore_ascii_case("me") { "" } else { &settings.user_name },
        };
        hark_llm::followup::generate(backend.as_ref(), &input).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---- chat ----

#[tauri::command]
pub fn list_chats(state: State<AppState>, scope_kind: String, scope_id: Option<String>) -> CmdResult<Vec<Chat>> {
    state.store.list_chats(&scope_kind, scope_id.as_deref()).map_err(err)
}

#[tauri::command]
pub fn create_chat(state: State<AppState>, scope_kind: String, scope_id: Option<String>, title: Option<String>) -> CmdResult<Chat> {
    if scope_kind != "meeting" && scope_kind != "all" {
        return Err("scope_kind must be 'meeting' or 'all'".into());
    }
    state.store.create_chat(&scope_kind, scope_id.as_deref(), title.as_deref().unwrap_or("New chat")).map_err(err)
}

#[tauri::command]
pub fn delete_chat(state: State<AppState>, id: String) -> CmdResult<()> {
    state.store.delete_chat(&id).map_err(err)
}

#[tauri::command]
pub fn chat_messages(state: State<AppState>, chat_id: String) -> CmdResult<Vec<ChatMessage>> {
    state.store.messages(&chat_id).map_err(err)
}

#[derive(Serialize, Clone)]
pub struct ChatTokenPayload {
    pub chat_id: String,
    pub message_id: String,
    pub delta: String,
}

#[derive(Serialize, Clone)]
pub struct ChatDonePayload {
    pub chat_id: String,
    pub message: ChatMessage,
    pub error: Option<String>,
}

/// Send a user message; the assistant reply streams as `chat_token` events and
/// finishes with `chat_done`. Returns the (empty) assistant message id.
#[tauri::command]
pub fn send_chat(app: AppHandle, chat_id: String, text: String) -> CmdResult<ChatMessage> {
    let state = app.state::<AppState>();
    let chat = state.store.get_chat(&chat_id).map_err(err)?.ok_or("chat not found")?;
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("empty message".into());
    }
    let backend = state.llm().ok_or("No language model available. Download one in Settings or connect an endpoint.")?;
    let history: Vec<(Role, String)> = state
        .store
        .messages(&chat_id)
        .map_err(err)?
        .into_iter()
        .filter(|m| !m.content.is_empty())
        .map(|m| (if m.role == "assistant" { Role::Assistant } else { Role::User }, m.content))
        .collect();
    state.store.add_message(&chat_id, "user", &text, &serde_json::json!([])).map_err(err)?;
    if history.is_empty() {
        let title: String = text.chars().take(48).collect();
        let _ = state.store.rename_chat(&chat_id, title.trim());
    }
    let assistant = state.store.add_message(&chat_id, "assistant", "", &serde_json::json!([])).map_err(err)?;

    let cancel = Arc::new(AtomicBool::new(false));
    state.chat_cancels.lock().insert(chat_id.clone(), cancel.clone());

    let app2 = app.clone();
    let msg_id = assistant.id.clone();
    let scope_meeting = if chat.scope_kind == "meeting" { chat.scope_id.clone() } else { None };
    std::thread::spawn(move || {
        let state = app2.state::<AppState>();
        let _busy = state.busy_guard();
        // Retrieval: BM25 + embeddings (when available); a single short meeting is passed whole.
        let mut hits = match &scope_meeting {
            Some(mid) => {
                let all = state.store.chunks(mid).unwrap_or_default();
                if all.len() <= 12 {
                    all
                } else {
                    crate::embed_stage::retrieve(&state, &text, Some(mid), 10)
                }
            }
            None => crate::embed_stage::retrieve(&state, &text, None, 12),
        };
        hits.sort_by(|a, b| a.meeting_id.cmp(&b.meeting_id).then(a.start_ms.cmp(&b.start_ms)));
        let passages: Vec<Passage> = hits
            .iter()
            .map(|h| Passage {
                meeting_id: h.meeting_id.clone(),
                meeting_title: h.meeting_title.clone(),
                meeting_date: h.started_at.format("%b %-d, %Y").to_string(),
                start_ms: h.start_ms,
                end_ms: h.end_ms,
                text: h.text.clone(),
            })
            .collect();
        let msgs = build_messages(&text, &history, &passages, scope_meeting.is_none());
        let mut full = String::new();
        let chat_id2 = chat_id.clone();
        let msg_id2 = msg_id.clone();
        let result = backend.chat(&msgs, &GenOptions { max_tokens: 800, temperature: 0.2, json: false }, &mut |delta| {
            full.push_str(delta);
            let _ = app2.emit(
                events::CHAT_TOKEN,
                ChatTokenPayload { chat_id: chat_id2.clone(), message_id: msg_id2.clone(), delta: delta.to_string() },
            );
            !cancel.load(Ordering::Relaxed)
        });
        state.chat_cancels.lock().remove(&chat_id);
        let (content, error) = match result {
            Ok(s) => (s, None),
            Err(hark_llm::LlmError::Cancelled) => (full.trim().to_string(), None),
            Err(e) => (full.trim().to_string(), Some(e.to_string())),
        };
        let citations = serde_json::to_value(parse_citations(&content, &passages)).unwrap_or(serde_json::json!([]));
        let _ = state.store.update_message(&msg_id, &content, &citations);
        let message = ChatMessage { id: msg_id.clone(), chat_id: chat_id.clone(), role: "assistant".into(), content, citations, created_at: assistant.created_at };
        let _ = app2.emit(events::CHAT_DONE, ChatDonePayload { chat_id: chat_id.clone(), message, error });
    });
    Ok(ChatMessage { content: String::new(), ..assistant })
}

#[tauri::command]
pub fn cancel_chat(state: State<AppState>, chat_id: String) {
    if let Some(c) = state.chat_cancels.lock().get(&chat_id) {
        c.store(true, Ordering::Relaxed);
    }
}
