use hark_capture::AudioDevice;
use crate::state::AppState;
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
pub struct AudioDevices {
    pub inputs: Vec<AudioDevice>,
    pub outputs: Vec<AudioDevice>,
}

#[tauri::command]
pub fn list_audio_devices() -> AudioDevices {
    AudioDevices { inputs: hark_capture::input_devices(), outputs: hark_capture::output_devices() }
}

/// Sample current mic / system levels for ~400 ms (Settings page meters).
#[tauri::command]
pub fn sample_levels(mic: Option<String>, loopback: Option<String>) -> (f32, f32) {
    hark_capture::probe::sample_levels(mic.as_deref(), loopback.as_deref(), std::time::Duration::from_millis(400))
}

#[derive(Serialize)]
pub struct VideoSource {
    pub target: hark_capture::VideoTarget,
    pub label: String,
    /// True for the window of the meeting app that was just detected.
    pub is_meeting: bool,
    /// Owning process (windows only); lets the recorder capture just that app's audio.
    pub pid: Option<u32>,
}

/// Displays first, then visible windows (the detected meeting window, if any, first).
#[tauri::command]
pub fn list_video_sources(state: State<AppState>, meeting_app: Option<String>) -> Vec<VideoSource> {
    let mut out: Vec<VideoSource> = hark_capture::list_monitors(state.ffmpeg.as_deref())
        .into_iter()
        .map(|m| VideoSource {
            label: if m.width > 0 { format!("{} ({}x{}){}", m.name, m.width, m.height, if m.primary { ", primary" } else { "" }) } else { m.name.clone() },
            target: hark_capture::VideoTarget::Monitor { index: m.index },
            is_meeting: false,
            pid: None,
        })
        .collect();
    let windows = hark_detect::list_visible_windows();
    let patterns = hark_detect::PatternSet::builtin();
    let meeting_title = meeting_app
        .as_deref()
        .and_then(|a| patterns.match_windows(&windows).filter(|d| d.app == a))
        .map(|d| d.title);
    let mut seen: Vec<String> = Vec::new();
    let mut wins: Vec<VideoSource> = windows
        .into_iter()
        .filter(|w| !w.title.trim().is_empty() && w.process != "hark.exe" && w.process != "hark")
        .filter(|w| {
            if seen.iter().any(|t| t == &w.title) {
                false
            } else {
                seen.push(w.title.clone());
                true
            }
        })
        .take(40)
        .map(|w| VideoSource {
            is_meeting: meeting_title.as_deref() == Some(w.title.as_str()),
            label: w.title.chars().take(70).collect(),
            pid: (w.pid != 0).then_some(w.pid),
            target: hark_capture::VideoTarget::Window { title: w.title },
        })
        .collect();
    wins.sort_by_key(|w| !w.is_meeting);
    out.extend(wins);
    out
}
