use super::{err, CmdResult};
use crate::pipeline;
use crate::state::AppState;
use hark_store::{MeetingSpeaker, Speaker};
use std::collections::BTreeMap;
use tauri::{AppHandle, Manager, State};

fn stored_embedding(state: &AppState, meeting_id: &str, label: &str) -> Option<Vec<f32>> {
    let map: BTreeMap<String, Vec<f32>> = state.store.get_setting(&format!("speaker_embeddings:{meeting_id}")).ok().flatten()?;
    map.get(label).cloned()
}

#[tauri::command]
pub fn meeting_speakers(state: State<AppState>, id: String) -> CmdResult<Vec<MeetingSpeaker>> {
    state.store.meeting_speakers(&id).map_err(err)
}

/// Rename a label ("Speaker 2") to a person's name everywhere in this meeting.
/// The voiceprint is stored so future meetings can suggest the name.
#[tauri::command]
pub fn rename_speaker(state: State<AppState>, id: String, label: String, name: String) -> CmdResult<Speaker> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("name cannot be empty".into());
    }
    let emb = stored_embedding(&state, &id, &label);
    let speaker = state.store.rename_speaker(&id, &label, &name, emb.as_deref()).map_err(err)?;
    // Keep the embedding reachable under the new label for further renames.
    if let Some(e) = emb {
        let mut map: BTreeMap<String, Vec<f32>> = state.store.get_setting(&format!("speaker_embeddings:{id}")).ok().flatten().unwrap_or_default();
        map.remove(&label);
        map.insert(speaker.name.clone(), e);
        let _ = state.store.set_setting(&format!("speaker_embeddings:{id}"), &map);
    }
    Ok(speaker)
}

/// Accept the "Is this Sarah?" suggestion for a label.
#[tauri::command]
pub fn accept_speaker_suggestion(state: State<AppState>, id: String, label: String) -> CmdResult<Speaker> {
    let rows = state.store.meeting_speakers(&id).map_err(err)?;
    let row = rows.into_iter().find(|r| r.label == label).ok_or("unknown speaker label")?;
    let name = row.suggested_name.ok_or("no suggestion for this speaker")?;
    rename_speaker(state, id, label, name)
}

#[tauri::command]
pub fn list_known_speakers(state: State<AppState>) -> CmdResult<Vec<Speaker>> {
    let mut v = state.store.list_speakers().map_err(err)?;
    for s in &mut v {
        s.embedding.clear(); // not useful to the UI, keep payload small
    }
    Ok(v)
}

#[tauri::command]
pub fn delete_known_speaker(state: State<AppState>, id: String) -> CmdResult<()> {
    state.store.delete_speaker(&id).map_err(err)
}

#[tauri::command]
pub fn rerun_cleanup(app: AppHandle, id: String) -> CmdResult<()> {
    let state = app.state::<AppState>();
    state.store.get_meeting(&id).map_err(err)?.ok_or("meeting not found")?;
    state.store.clear_clean_text(&id).map_err(err)?;
    let app2 = app.clone();
    std::thread::spawn(move || {
        pipeline::run_cleanup(&app2, &id);
        let _ = pipeline::reembed(&app2, &id);
    });
    Ok(())
}

/// Try an OpenAI-compatible endpoint; returns the model list on success.
#[tauri::command]
pub async fn test_llm_endpoint(url: String, api_key: Option<String>, model: String) -> CmdResult<Vec<String>> {
    tauri::async_runtime::spawn_blocking(move || {
        let c = hark_llm::OpenAiCompat::new(&url, api_key.as_deref(), &model);
        c.list_models().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
