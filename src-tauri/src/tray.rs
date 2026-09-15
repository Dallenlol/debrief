use crate::recorder::{self, StartOptions};
use crate::state::AppState;
use crate::windows;
use std::sync::atomic::Ordering;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

pub const TRAY_ID: &str = "hark-tray";


pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Hark", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "Start recording", true, None::<&str>)?;
    let detect = MenuItem::with_id(app, "detect", "Pause detection", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &toggle, &detect, &PredefinedMenuItem::separator(app)?, &quit])?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(app.default_window_icon().cloned().expect("app icon"))
        .tooltip("Hark")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, ev| match ev.id().as_ref() {
            "open" => windows::focus_main(app),
            "toggle" => toggle_recording(app),
            "detect" => {
                let state = app.state::<AppState>();
                let paused = !state.detection_paused.load(Ordering::SeqCst);
                state.detection_paused.store(paused, Ordering::SeqCst);
                let _ = detect.set_text(if paused { "Resume detection" } else { "Pause detection" });
            }
            "quit" => {
                if app.state::<AppState>().recording.lock().is_some() {
                    let _ = recorder::stop(app);
                }
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                windows::focus_main(tray.app_handle());
            }
        })
        .build(app)?;
    app.manage(TrayItems { toggle });
    Ok(())
}

pub fn toggle_recording(app: &AppHandle) {
    // Off the main thread: device setup/teardown must never block the event loop.
    let app = app.clone();
    std::thread::spawn(move || {
        let is_recording = app.state::<AppState>().recording.lock().is_some();
        let result = if is_recording { recorder::stop(&app).map(|_| ()) } else { recorder::start(&app, StartOptions::default()).map(|_| ()) };
        if let Err(e) = result {
            recorder::notice(&app, "error", e);
        }
        refresh(&app);
    });
}

/// Update the tray "Start/Stop recording" label after state changes.
pub fn refresh(app: &AppHandle) {
    let is_recording = app.state::<AppState>().recording.lock().is_some();
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.toggle.set_text(if is_recording { "Stop recording" } else { "Start recording" });
    }
}

/// Menu items we need to update later.
pub struct TrayItems {
    pub toggle: MenuItem<tauri::Wry>,
}
