use super::CmdResult;
use crate::events::RecordingStatePayload;
use crate::recorder::{self, StartOptions};
use crate::state::AppState;
use crate::tray;
use crate::windows;
use hark_store::Meeting;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

#[tauri::command]
pub fn start_recording(app: AppHandle, opts: Option<StartOptions>) -> CmdResult<Meeting> {
    let m = recorder::start(&app, opts.unwrap_or_default())?;
    tray::refresh(&app);
    Ok(m)
}

#[tauri::command]
pub fn stop_recording(app: AppHandle) -> CmdResult<Meeting> {
    let m = recorder::stop(&app)?;
    tray::refresh(&app);
    Ok(m)
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
