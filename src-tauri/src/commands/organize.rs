use super::{err, CmdResult};
use crate::state::AppState;
use hark_capture::video::{ffmpeg_clip_args, run_ffmpeg};
use hark_share::{export_bundle, import_bundle, ImportReport, ShareServer};
use hark_store::{Folder, Meeting, MeetingFilter, Share, Tag};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State};

// ---- folders & tags ----

#[tauri::command]
pub fn list_folders(state: State<AppState>) -> CmdResult<Vec<Folder>> {
    state.store.list_folders().map_err(err)
}

#[tauri::command]
pub fn create_folder(state: State<AppState>, name: String, parent_id: Option<String>) -> CmdResult<Folder> {
    if name.trim().is_empty() {
        return Err("folder needs a name".into());
    }
    state.store.create_folder(&name, parent_id.as_deref()).map_err(err)
}

#[tauri::command]
pub fn update_folder(state: State<AppState>, id: String, name: String, parent_id: Option<String>, default_template_id: Option<String>) -> CmdResult<()> {
    state.store.update_folder(&id, &name, parent_id.as_deref(), default_template_id.as_deref()).map_err(err)
}

#[tauri::command]
pub fn delete_folder(state: State<AppState>, id: String) -> CmdResult<()> {
    state.store.delete_folder(&id).map_err(err)
}

#[tauri::command]
pub fn move_meeting(state: State<AppState>, id: String, folder_id: Option<String>) -> CmdResult<()> {
    state.store.move_meeting(&id, folder_id.as_deref()).map_err(err)
}

#[tauri::command]
pub fn list_meetings_filtered(state: State<AppState>, filter: MeetingFilter) -> CmdResult<Vec<Meeting>> {
    state.store.list_meetings_filtered(&filter).map_err(err)
}

#[tauri::command]
pub fn list_tags(state: State<AppState>) -> CmdResult<Vec<Tag>> {
    state.store.list_tags().map_err(err)
}

#[tauri::command]
pub fn meeting_tags(state: State<AppState>, id: String) -> CmdResult<Vec<Tag>> {
    state.store.meeting_tags(&id).map_err(err)
}

#[tauri::command]
pub fn tag_meeting(state: State<AppState>, id: String, name: String) -> CmdResult<Tag> {
    if name.trim().is_empty() {
        return Err("tag needs a name".into());
    }
    state.store.tag_meeting(&id, &name).map_err(err)
}

#[tauri::command]
pub fn untag_meeting(state: State<AppState>, id: String, tag_id: String) -> CmdResult<()> {
    state.store.untag_meeting(&id, &tag_id).map_err(err)
}

// ---- clips ----

fn source_media(state: &AppState, meeting_id: &str) -> Result<(PathBuf, bool), String> {
    let dir = state.store.recordings_dir(meeting_id);
    let video = dir.join("screen.mp4");
    if video.exists() {
        Ok((video, true))
    } else {
        let audio = dir.join("mix.wav");
        if audio.exists() {
            Ok((audio, false))
        } else {
            Err("no media for this meeting".into())
        }
    }
}

/// Cut a clip to `out` (extension decides mp4 or mp3) and write a `.txt` transcript beside it.
#[tauri::command]
pub fn export_clip(state: State<AppState>, id: String, start_ms: i64, end_ms: i64, out: String) -> CmdResult<String> {
    if end_ms <= start_ms {
        return Err("clip end must be after start".into());
    }
    let ffmpeg = state.ffmpeg.as_ref().ok_or("ffmpeg not found")?;
    let (src, _) = source_media(&state, &id)?;
    let out_path = PathBuf::from(&out);
    run_ffmpeg(ffmpeg, &ffmpeg_clip_args(&src, start_ms as f64 / 1000.0, end_ms as f64 / 1000.0, &out_path))?;
    let segs = state.store.segments(&id).map_err(err)?;
    let text: String = segs
        .iter()
        .filter(|s| s.end_ms >= start_ms && s.start_ms <= end_ms)
        .map(|s| hark_llm::summary::transcript_line(s.start_ms - start_ms, s.speaker.as_deref(), s.clean_text.as_deref().unwrap_or(&s.text)))
        .collect::<Vec<_>>()
        .join("\n");
    let _ = std::fs::write(out_path.with_extension("txt"), text);
    Ok(out)
}

// ---- shares ----

#[derive(Serialize)]
pub struct ShareInfo {
    pub share: Share,
    pub url: String,
}

fn ensure_server(state: &AppState) -> Result<u16, String> {
    let mut guard = state.share_server.lock();
    if let Some(h) = guard.as_ref() {
        return Ok(h.port);
    }
    let port = state.settings.read().share_port;
    let clips = state.store.root().join("clips");
    let _ = std::fs::create_dir_all(&clips);
    let h = ShareServer::start(state.store.clone(), clips, port).map_err(|e| format!("start share server on port {port}: {e}"))?;
    let p = h.port;
    *guard = Some(h);
    Ok(p)
}

fn stop_server_if_idle(state: &AppState) {
    let active = state.store.list_shares().map(|s| s.iter().any(|x| x.enabled)).unwrap_or(false);
    if !active {
        if let Some(h) = state.share_server.lock().take() {
            h.stop();
        }
    }
}

/// Create a share link for the whole meeting or a clip (rendered with ffmpeg into `clips/`).
#[tauri::command]
pub fn create_share(app: AppHandle, id: String, start_ms: Option<i64>, end_ms: Option<i64>) -> CmdResult<ShareInfo> {
    let state = app.state::<AppState>();
    let kind = if start_ms.is_some() && end_ms.is_some() { "clip" } else { "meeting" };
    let share = state.store.create_share(&id, kind, start_ms, end_ms).map_err(err)?;
    if kind == "clip" {
        let ffmpeg = state.ffmpeg.as_ref().ok_or("ffmpeg not found")?;
        let (src, is_video) = source_media(&state, &id)?;
        let clips = state.store.root().join("clips");
        let _ = std::fs::create_dir_all(&clips);
        let out = clips.join(format!("{}.{}", share.token, if is_video { "mp4" } else { "mp3" }));
        if let Err(e) = run_ffmpeg(ffmpeg, &ffmpeg_clip_args(&src, start_ms.unwrap() as f64 / 1000.0, end_ms.unwrap() as f64 / 1000.0, &out)) {
            let _ = state.store.delete_share(&share.token);
            return Err(format!("render clip: {e}"));
        }
    }
    let port = ensure_server(&state)?;
    Ok(ShareInfo { url: hark_share::server::share_url(port, &share.token), share })
}

#[tauri::command]
pub fn list_shares(state: State<AppState>) -> CmdResult<Vec<ShareInfo>> {
    let port = state.share_server.lock().as_ref().map(|h| h.port).unwrap_or(state.settings.read().share_port);
    Ok(state.store.list_shares().map_err(err)?.into_iter().map(|s| ShareInfo { url: hark_share::server::share_url(port, &s.token), share: s }).collect())
}

#[tauri::command]
pub fn set_share_enabled(state: State<AppState>, token: String, enabled: bool) -> CmdResult<()> {
    state.store.set_share_enabled(&token, enabled).map_err(err)?;
    if enabled {
        ensure_server(&state)?;
    } else {
        stop_server_if_idle(&state);
    }
    Ok(())
}

#[tauri::command]
pub fn delete_share(state: State<AppState>, token: String) -> CmdResult<()> {
    state.store.delete_share(&token).map_err(err)?;
    let clips = state.store.root().join("clips");
    for ext in ["mp4", "mp3"] {
        let _ = std::fs::remove_file(clips.join(format!("{token}.{ext}")));
    }
    stop_server_if_idle(&state);
    Ok(())
}

// ---- export / import ----

#[tauri::command]
pub fn export_meetings(state: State<AppState>, ids: Vec<String>, out: String, include_video: bool) -> CmdResult<usize> {
    let ids = if ids.is_empty() { state.store.list_meetings().map_err(err)?.into_iter().map(|m| m.id).collect() } else { ids };
    let manifest = export_bundle(&state.store, &ids, Path::new(&out), include_video).map_err(err)?;
    Ok(manifest.meetings.len())
}

#[tauri::command]
pub fn import_meetings(state: State<AppState>, path: String) -> CmdResult<ImportReport> {
    import_bundle(&state.store, Path::new(&path)).map_err(err)
}

/// Write a standalone HTML page next to a copy of the media.
/// Write text the UI rendered (transcript .txt/.md/.srt, summary) to a path the user picked.
#[tauri::command]
pub fn save_text_file(path: String, text: String) -> CmdResult<()> {
    let p = PathBuf::from(&path);
    if p.file_name().is_none() {
        return Err("no file name".into());
    }
    std::fs::write(&p, text).map_err(|e| format!("could not write {}: {e}", p.display()))
}

#[tauri::command]
pub fn export_html(state: State<AppState>, id: String, out_dir: String) -> CmdResult<String> {
    let meeting = state.store.get_meeting(&id).map_err(err)?.ok_or("meeting not found")?;
    let segments = state.store.segments(&id).map_err(err)?;
    let summary = state.store.get_summary(&id).map_err(err)?;
    let (src, is_video) = source_media(&state, &id)?;
    let out_dir = PathBuf::from(&out_dir);
    std::fs::create_dir_all(&out_dir).map_err(err)?;
    let media_name = if is_video { "recording.mp4" } else { "recording.wav" };
    std::fs::copy(&src, out_dir.join(media_name)).map_err(err)?;
    let html = hark_share::render_standalone(&hark_share::html::PageData {
        meeting: &meeting,
        segments: &segments,
        summary: summary.as_ref(),
        media_url: Some(media_name),
        is_video,
        clip: None,
    });
    let page = out_dir.join("index.html");
    std::fs::write(&page, html).map_err(err)?;
    Ok(page.to_string_lossy().into_owned())
}

/// Move the whole data folder. Copies everything, then writes a pointer file
/// in the default location so the next launch opens the new folder.
#[tauri::command]
pub fn change_data_dir(state: State<AppState>, new_dir: String) -> CmdResult<String> {
    if state.recording.lock().is_some() {
        return Err("stop the current recording first".into());
    }
    let from = state.store.root().to_path_buf();
    let to = PathBuf::from(new_dir.trim());
    if to == from {
        return Ok(to.to_string_lossy().into_owned());
    }
    std::fs::create_dir_all(&to).map_err(err)?;
    state.store.checkpoint().map_err(err)?;
    copy_dir(&from, &to).map_err(|e| format!("copy: {e}"))?;
    hark_store::write_data_dir_pointer(&to).map_err(err)?;
    Ok(to.to_string_lossy().into_owned())
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&dest)?;
            copy_dir(&entry.path(), &dest)?;
        } else if entry.file_name() != "hark.db-wal" && entry.file_name() != "hark.db-shm" {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}
