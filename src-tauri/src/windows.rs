//! Helpers for the three windows: `main`, `popup` (call detected), `recordbar`.

use tauri::{AppHandle, Manager, PhysicalPosition, WebviewWindow};

const MARGIN: i32 = 16;

fn window(app: &AppHandle, label: &str) -> Option<WebviewWindow> {
    app.get_webview_window(label)
}

/// Place `w` at a screen corner of the monitor the main window is on (or primary).
fn place(w: &WebviewWindow, bottom: bool, right: bool) {
    let monitor = w.current_monitor().ok().flatten().or_else(|| w.primary_monitor().ok().flatten());
    let Some(m) = monitor else { return };
    let Ok(size) = w.outer_size() else { return };
    let ms = m.size();
    let mp = m.position();
    let x = if right { mp.x + ms.width as i32 - size.width as i32 - MARGIN } else { mp.x + MARGIN };
    let y = if bottom { mp.y + ms.height as i32 - size.height as i32 - MARGIN - 48 } else { mp.y + MARGIN };
    let _ = w.set_position(PhysicalPosition::new(x, y));
}

pub fn show_popup(app: &AppHandle) {
    if let Some(w) = window(app, "popup") {
        place(&w, true, true);
        let _ = w.show();
        let _ = w.set_always_on_top(true);
    }
}

pub fn hide_popup(app: &AppHandle) {
    if let Some(w) = window(app, "popup") {
        let _ = w.hide();
    }
}

pub fn show_recordbar(app: &AppHandle) {
    if let Some(w) = window(app, "recordbar") {
        place(&w, false, true);
        let _ = w.show();
        let _ = w.set_always_on_top(true);
    }
}

pub fn hide_recordbar(app: &AppHandle) {
    if let Some(w) = window(app, "recordbar") {
        let _ = w.hide();
    }
}

pub fn focus_main(app: &AppHandle) {
    if let Some(w) = window(app, "main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}
