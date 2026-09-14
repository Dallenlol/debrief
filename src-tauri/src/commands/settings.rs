use super::{err, CmdResult};
use crate::settings::Settings;
use crate::shortcuts;
use crate::state::AppState;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Settings {
    state.settings.read().clone()
}

#[tauri::command]
pub fn set_settings(app: AppHandle, new: Settings) -> CmdResult<Settings> {
    let state = app.state::<AppState>();
    let old_hotkey = state.settings.read().hotkey.clone();
    new.save(&state.store).map_err(err)?;
    *state.settings.write() = new.clone();
    if new.hotkey != old_hotkey {
        shortcuts::register(&app, &new.hotkey).map_err(err)?;
    }
    Ok(new)
}

#[derive(Serialize)]
pub struct DataInfo {
    pub data_dir: String,
    pub recordings_bytes: u64,
    pub models_bytes: u64,
}

#[tauri::command]
pub fn data_info(state: State<AppState>) -> DataInfo {
    let root = state.store.root().to_path_buf();
    DataInfo {
        data_dir: root.to_string_lossy().into_owned(),
        recordings_bytes: dir_size(&root.join("recordings")),
        models_bytes: dir_size(&root.join("models")),
    }
}

fn dir_size(p: &std::path::Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else { return 0 };
    rd.flatten()
        .map(|e| {
            let path = e.path();
            if path.is_dir() {
                dir_size(&path)
            } else {
                e.metadata().map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}
