use super::CmdResult;
use crate::events::RecordingStatePayload;
use crate::recorder::{self, StartOptions};
use crate::state::AppState;
use crate::tray;
use crate::windows;
use hark_store::Meeting;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

// Start and stop open/close audio devices and join capture threads; they run
// off the main thread so a slow or dead device can never freeze the window.
#[tauri::command]
pub async fn start_recording(app: AppHandle, opts: Option<StartOptions>) -> CmdResult<Meeting> {
    tauri::async_runtime::spawn_blocking(move || {
        let m = recorder::start(&app, opts.unwrap_or_default())?;
        tray::refresh(&app);
        Ok(m)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn stop_recording(app: AppHandle) -> CmdResult<Meeting> {
    tauri::async_runtime::spawn_blocking(move || {
        let m = recorder::stop(&app)?;
        tray::refresh(&app);
        Ok(m)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn pause_recording(app: AppHandle) {
    recorder::pause(&app);
}

#[tauri::command]
pub fn resume_recording(app: AppHandle) {
    recorder::resume(&app);
}

#[tauri::command]
pub fn mark_highlight(app: AppHandle) -> CmdResult<u64> {
    recorder::mark_highlight(&app).ok_or_else(|| "not recording".to_string())
}

#[tauri::command]
pub fn recording_status(app: AppHandle) -> RecordingStatePayload {
    recorder::status_payload(&app.state::<AppState>())
}

/// Show the "what to record" popup for a manual start (Record now, hotkey, tray).
#[tauri::command]
pub fn open_record_picker(app: AppHandle) {
    recorder::open_picker(&app);
}

/// Captions and running notes of the recording in progress (None when idle).
#[tauri::command]
pub fn live_snapshot(app: AppHandle) -> Option<crate::live_feed::LiveSnapshot> {
    crate::live_feed::LiveFeed::snapshot(&app.state::<AppState>())
}

/// `mode`: "now" hides the popup; "never" also adds the app to the never-list;
/// "snooze" pauses detection for an hour.
#[tauri::command]
pub fn dismiss_detection(app: AppHandle, app_id: String, mode: String) -> CmdResult<()> {
    let state = app.state::<AppState>();
    match mode.as_str() {
        "never" => {
            let mut s = state.settings.write();
            if !s.never_apps.contains(&app_id) {
                s.never_apps.push(app_id);
            }
            s.save(&state.store).map_err(|e| e.to_string())?;
        }
        "snooze" => {
            *state.snooze_until.lock() = Some(Instant::now() + Duration::from_secs(3600));
        }
        _ => {}
    }
    windows::hide_popup(&app);
    Ok(())
}

#[tauri::command]
pub fn open_main(app: AppHandle) {
    windows::focus_main(&app);
}
