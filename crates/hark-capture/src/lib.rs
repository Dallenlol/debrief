//! Audio (and screen) capture for Hark.
//!
//! * `devices` - enumerate mics / outputs (outputs double as loopback sources)
//! * `recorder` - mic + system audio to WAV, plus a live 16 kHz mono feed
//! * `video` - ffmpeg sidecar argument builders and process control
//! * `probe` - short level sampling for the audio-activity detector

pub mod devices;
pub mod dsp;
pub mod probe;
pub mod recorder;
pub mod stream;
pub mod video;
pub mod wav;

pub use devices::{find_device, input_devices, output_devices, AudioDevice};
pub use recorder::{CaptureEvent, RecordConfig, RecordError, RecordOutput, Recorder, ARCHIVE_HZ, ASR_HZ};
pub use video::{VideoRecorder, VideoTarget};
pub use wav::WavWriter;
