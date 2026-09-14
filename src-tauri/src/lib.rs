mod commands;
mod detector_loop;
mod events;
mod recorder;
mod settings;
mod shortcuts;
mod smoke;
mod state;
mod tray;
mod windows;

use state::AppState;
use tauri::{Manager, WindowEvent};

pub fn run() {
    let data_dir = hark_store::data_dir();
    let store = hark_store::Store::open(&data_dir.join("hark.db")).expect("open database");
    let app_state = AppState::new(store);

    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().level(log::LevelFilter::Info).build())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| windows::focus_main(app)))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_os::init())
        .manage(app_state)
        .setup(|app| {
            let handle = app.handle().clone();
            tray::build(&handle)?;
            let hotkey = handle.state::<AppState>().settings.read().hotkey.clone();
            if let Err(e) = shortcuts::register(&handle, &hotkey) {
                log::warn!("hotkey: {e}");
            }
            detector_loop::spawn(handle.clone());
            spawn_engine_idle_unloader(handle.clone());
            log::info!("Hark started; data dir {}", hark_store::data_dir().display());
            if handle.state::<AppState>().ffmpeg.is_none() {
                log::warn!("ffmpeg sidecar not found; screen video disabled");
            }
            if std::env::var_os("HARK_SMOKE").is_some() {
                smoke::run(handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let close_to_tray = app.state::<AppState>().settings.read().close_to_tray;
                // Popup/record bar always hide; the main window hides when close-to-tray is on.
                if window.label() != "main" || close_to_tray {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::meetings::list_meetings,
            commands::meetings::get_meeting,
            commands::meetings::rename_meeting,
            commands::meetings::delete_meeting,
            commands::meetings::search,
            commands::meetings::retranscribe,
            commands::recording::start_recording,
            commands::recording::stop_recording,
            commands::recording::pause_recording,
            commands::recording::resume_recording,
            commands::recording::mark_highlight,
            commands::recording::recording_status,
            commands::recording::dismiss_detection,
            commands::recording::open_main,
            commands::settings::get_settings,
            commands::settings::set_settings,
            commands::settings::data_info,
            commands::models::probe_hardware,
            commands::models::list_models,
            commands::models::download_model,
            commands::models::cancel_download,
            commands::models::remove_model,
            commands::devices::list_audio_devices,
            commands::devices::sample_levels,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Hark");
}

/// Free model memory after 10 minutes without use (never while recording).
fn spawn_engine_idle_unloader(app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
        let state = app.state::<AppState>();
        if state.recording.lock().is_some() {
            continue;
        }
        let idle = state.engines.lock().last_used.map(|t| t.elapsed() > std::time::Duration::from_secs(600)).unwrap_or(false);
        if idle {
            log::info!("unloading idle AI engines");
            state.unload_engines();
        }
    });
}
