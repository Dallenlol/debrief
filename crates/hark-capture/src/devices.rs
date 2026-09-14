use cpal::traits::{DeviceTrait, HostTrait};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioDevice {
    /// Stable id usable with `find_device`. Display string of `cpal::DeviceId`.
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

fn describe(dev: &cpal::Device, default: Option<&cpal::Device>) -> Option<AudioDevice> {
    let id = dev.id().ok()?.to_string();
    let name = dev.description().map(|d| d.name().to_string()).unwrap_or_else(|_| id.clone());
    let is_default = default.map(|d| d == dev).unwrap_or(false);
    Some(AudioDevice { id, name, is_default })
}

/// Microphones / capture devices.
pub fn input_devices() -> Vec<AudioDevice> {
    let host = cpal::default_host();
    let default = host.default_input_device();
    host.input_devices()
        .map(|it| it.filter_map(|d| describe(&d, default.as_ref())).collect())
        .unwrap_or_default()
}

/// Playback devices; recording one of these gives system-audio loopback.
pub fn output_devices() -> Vec<AudioDevice> {
    let host = cpal::default_host();
    let default = host.default_output_device();
    host.output_devices()
        .map(|it| it.filter_map(|d| describe(&d, default.as_ref())).collect())
        .unwrap_or_default()
}

/// Resolve a device by stable id. `None` id means the host default of that direction.
pub fn find_device(id: Option<&str>, output: bool) -> Option<cpal::Device> {
    let host = cpal::default_host();
    if let Some(id) = id {
        if let Ok(parsed) = id.parse::<cpal::DeviceId>() {
            if let Some(d) = host.device_by_id(&parsed) {
                return Some(d);
            }
        }
    }
    if output {
        host.default_output_device()
    } else {
        host.default_input_device()
    }
}
