use hark_capture::AudioDevice;
use serde::Serialize;

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
