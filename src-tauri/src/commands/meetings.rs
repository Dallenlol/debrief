use super::{err, CmdResult};
use crate::recorder;
use crate::state::AppState;
use hark_store::{Meeting, MeetingSpeaker, SearchHit, Segment};
use serde::Serialize;
use tauri::{AppHandle, State};

#[derive(Serialize)]
pub struct Media {
    pub audio: String,
    pub video: Option<String>,
}

#[derive(Serialize)]
pub struct MeetingDetail {
    pub meeting: Meeting,
    pub segments: Vec<Segment>,
    pub media: Media,
    pub highlights: Vec<u64>,
    pub speakers: Vec<MeetingSpeaker>,
}

#[tauri::command]
pub fn list_meetings(state: State<AppState>) -> CmdResult<Vec<Meeting>> {
    state.store.list_meetings().map_err(err)
}

#[tauri::command]
pub fn get_meeting(state: State<AppState>, id: String) -> CmdResult<MeetingDetail> {
    let meeting = state.store.get_meeting(&id).map_err(err)?.ok_or("meeting not found")?;
    let segments = state.store.segments(&id).map_err(err)?;
    let dir = state.store.recordings_dir(&id);
    let video = dir.join("screen.mp4");
    let media = Media {
        audio: dir.join("mix.wav").to_string_lossy().into_owned(),
        video: video.exists().then(|| video.to_string_lossy().into_owned()),
    };
    let highlights = state.store.get_setting::<Vec<u64>>(&format!("highlights:{id}")).map_err(err)?.unwrap_or_default();
    let speakers = state.store.meeting_speakers(&id).map_err(err)?;
    Ok(MeetingDetail { meeting, segments, media, highlights, speakers })
}

#[tauri::command]
pub fn rename_meeting(state: State<AppState>, id: String, title: String) -> CmdResult<Meeting> {
    let mut m = state.store.get_meeting(&id).map_err(err)?.ok_or("meeting not found")?;
    m.title = title.trim().to_string();
    if m.title.is_empty() {
        return Err("title cannot be empty".into());
    }
    state.store.update_meeting(&m).map_err(err)?;
    Ok(m)
}

#[tauri::command]
pub fn delete_meeting(state: State<AppState>, id: String) -> CmdResult<()> {
    if state.recording.lock().as_ref().map(|a| a.meeting.id == id).unwrap_or(false) {
        return Err("cannot delete a meeting that is being recorded".into());
    }
    state.store.delete_meeting(&id).map_err(err)?;
    let dir = state.store.root().join("recordings").join(&id);
    let _ = std::fs::remove_dir_all(dir);
    Ok(())
}

#[tauri::command]
pub fn search(state: State<AppState>, query: String, limit: Option<usize>) -> CmdResult<Vec<SearchHit>> {
    state.store.search(&query, limit.unwrap_or(50)).map_err(err)
}

#[tauri::command]
pub fn retranscribe(app: AppHandle, id: String) -> CmdResult<()> {
    recorder::retranscribe(&app, &id)
}
