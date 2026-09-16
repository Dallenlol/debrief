mod calendar;
mod commands;
mod detector_loop;
mod embed_stage;
mod events;
mod pipeline;
mod recorder;
mod settings;
mod shortcuts;
mod import;
mod live_feed;
mod smoke;
mod state;
mod summary_stage;
mod tray;
mod webhook;
mod windows;

use state::AppState;
use tauri::{Manager, WindowEvent};

/// Updater keyed by build variant: Windows CPU and CUDA builds look up
/// `windows-x86_64-cpu` / `windows-x86_64-cuda` in latest.json; macOS uses the
/// default `darwin-<arch>` key (one Metal build per architecture).
fn updater_plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R, tauri_plugin_updater::Config> {
    let b = tauri_plugin_updater::Builder::new();
    if cfg!(windows) {
        b.target(format!("windows-{}-{}", std::env::consts::ARCH, env!("HARK_VARIANT"))).build()
    } else {
        b.build()
    }
}

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
        .plugin(tauri_plugin_process::init())
        .plugin(updater_plugin())
        .manage(app_state)
        .setup(|app| {
            let handle = app.handle().clone();
            tray::build(&handle)?;
            let hotkey = handle.state::<AppState>().settings.read().hotkey.clone();
            if let Err(e) = shortcuts::register(&handle, &hotkey) {
                log::warn!("hotkey: {e}");
            }
            recorder::recover_orphans(&handle);
            detector_loop::spawn(handle.clone());
            calendar::spawn(handle.clone());
            spawn_engine_idle_unloader(handle.clone());
            {
                let st = handle.state::<AppState>();
                let any_share = st.store.list_shares().map(|s| s.iter().any(|x| x.enabled)).unwrap_or(false);
                if any_share {
                    let port = st.settings.read().share_port;
                    match hark_share::ShareServer::start(st.store.clone(), st.store.root().join("clips"), port) {
                        Ok(h) => *st.share_server.lock() = Some(h),
                        Err(e) => log::warn!("share server: {e}"),
                    }
                }
            }
            // The player streams mix.wav / screen.mp4 through the asset protocol; the
            // config-file scope cannot know a user-chosen data folder, so allow it here.
            let recordings = hark_store::data_dir().join("recordings");
            if let Err(e) = handle.asset_protocol_scope().allow_directory(&recordings, true) {
                log::warn!("asset scope: {e}");
            }
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
            commands::meetings::list_upcoming,
            commands::meetings::refresh_calendar,
            commands::meetings::rediarize,
            commands::meetings::reembed,
            commands::meetings::import_recording,
            commands::recording::start_recording,
            commands::recording::stop_recording,
            commands::recording::pause_recording,
            commands::recording::resume_recording,
            commands::recording::mark_highlight,
            commands::recording::recording_status,
            commands::recording::live_snapshot,
            commands::recording::open_record_picker,
            commands::recording::dismiss_detection,
            commands::recording::open_main,
            commands::settings::get_settings,
            commands::settings::set_settings,
            commands::settings::data_info,
            commands::settings::test_webhook,
            commands::models::probe_hardware,
            commands::models::list_models,
            commands::models::download_model,
            commands::models::cancel_download,
            commands::models::remove_model,
            commands::speakers::meeting_speakers,
            commands::speakers::rename_speaker,
            commands::speakers::accept_speaker_suggestion,
            commands::speakers::list_known_speakers,
            commands::speakers::delete_known_speaker,
            commands::speakers::rerun_cleanup,
            commands::speakers::test_llm_endpoint,
            commands::ai::list_templates,
            commands::ai::save_template,
            commands::ai::delete_template,
            commands::ai::get_summary,
            commands::ai::generate_summary,
            commands::ai::draft_followup,
            commands::ai::list_chats,
            commands::ai::create_chat,
            commands::ai::delete_chat,
            commands::ai::chat_messages,
            commands::ai::send_chat,
            commands::ai::cancel_chat,
            commands::organize::list_folders,
            commands::organize::create_folder,
            commands::organize::update_folder,
            commands::organize::delete_folder,
            commands::organize::move_meeting,
            commands::organize::list_meetings_filtered,
            commands::organize::list_tags,
            commands::organize::meeting_tags,
            commands::organize::tag_meeting,
            commands::organize::untag_meeting,
            commands::organize::export_clip,
            commands::organize::create_share,
            commands::organize::list_shares,
            commands::organize::set_share_enabled,
            commands::organize::delete_share,
            commands::organize::export_meetings,
            commands::organize::import_meetings,
            commands::organize::export_html,
            commands::organize::save_text_file,
            commands::organize::change_data_dir,
            commands::devices::list_audio_devices,
            commands::devices::list_video_sources,
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
