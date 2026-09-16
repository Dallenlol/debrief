//! Records 5 seconds of mic + system audio to a temp dir and prints levels.
use hark_capture::{CaptureEvent, RecordConfig, Recorder};
use std::time::Duration;

fn main() {
    let dir = std::env::temp_dir().join("hark-record-example");
    let (tx, rx) = crossbeam_channel::unbounded();
    println!("inputs:  {:?}", hark_capture::input_devices().iter().map(|d| &d.name).collect::<Vec<_>>());
    println!("outputs: {:?}", hark_capture::output_devices().iter().map(|d| &d.name).collect::<Vec<_>>());
    let rec = Recorder::start(
        RecordConfig { dir: dir.clone(), mic_device: None, loopback_device: None, capture_system: true, audio_pid: None },
        tx,
    )
    .expect("start");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut pcm = 0usize;
    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(CaptureEvent::Levels { mic_db, sys_db }) => println!("mic {mic_db:6.1} dB  sys {sys_db:6.1} dB"),
            Ok(CaptureEvent::Pcm16k(v)) => pcm += v.len(),
            Ok(other) => println!("{other:?}"),
            Err(_) => {}
        }
    }
    let out = rec.stop().expect("stop");
    println!("16k samples: {pcm}  -> {:?}", out);
}
