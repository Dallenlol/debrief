//! Short level probe used by the audio-activity fallback detector.

use crate::devices::find_device;
use crate::dsp::rms_db;
use crate::stream::open_input;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;

/// Open mic + loopback for `dur`, return their RMS levels in dBFS.
/// Missing devices report -100.0. Blocks for `dur`.
pub fn sample_levels(mic_id: Option<&str>, loopback_id: Option<&str>, dur: Duration) -> (f32, f32) {
    let mic_buf = Arc::new(Mutex::new(Vec::<f32>::new()));
    let sys_buf = Arc::new(Mutex::new(Vec::<f32>::new()));
    let noop_err: Arc<dyn Fn(String) + Send + Sync> = Arc::new(|_| {});

    let _mic = find_device(mic_id, false).and_then(|d| {
        let b = mic_buf.clone();
        open_input(&d, false, Arc::new(move |p| b.lock().extend_from_slice(p)), noop_err.clone()).ok()
    });
    let _sys = find_device(loopback_id, true).and_then(|d| {
        let b = sys_buf.clone();
        open_input(&d, true, Arc::new(move |p| b.lock().extend_from_slice(p)), noop_err.clone()).ok()
    });
    std::thread::sleep(dur);
    let mic = rms_db(&mic_buf.lock());
    let sys = rms_db(&sys_buf.lock());
    (mic, sys)
}
