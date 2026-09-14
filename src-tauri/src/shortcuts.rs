//! Global hotkey: toggles recording from anywhere.

use crate::tray;
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub fn register(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let shortcut: Shortcut = hotkey.parse().map_err(|e| format!("invalid hotkey '{hotkey}': {e}"))?;
    gs.on_shortcut(shortcut, |app, _sc, ev| {
        if ev.state == ShortcutState::Pressed {
            tray::toggle_recording(app);
        }
    })
    .map_err(|e| e.to_string())
}
