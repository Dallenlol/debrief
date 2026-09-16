//! Thin cpal wrappers that deliver mono f32 audio, resampled to a rate the
//! caller picks, so a replacement device with another native rate is a drop-in.

use crate::dsp::{to_mono, Resampler};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use parking_lot::Mutex;
use std::sync::Arc;

/// Callback receiving mono f32 frames.
pub type AudioSink = Arc<dyn Fn(&[f32]) + Send + Sync>;
/// Callback receiving a stream error message.
pub type ErrorSink = Arc<dyn Fn(String) + Send + Sync>;

#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    #[error("no audio device available")]
    NoDevice,
    #[error("audio: {0}")]
    Cpal(String),
}

/// A running input stream plus the format it delivers.
pub struct InputStream {
    inner: Inner,
    /// Native rate of the device (what cpal delivers before resampling).
    pub device_rate: u32,
    /// Rate `on_audio` receives.
    pub sample_rate: u32,
}

enum Inner {
    Cpal(Option<Stream>),
    /// Windows per-process loopback; stops itself on drop without blocking.
    #[cfg(windows)]
    Process(crate::process_loopback::ProcessLoopback),
    #[allow(dead_code)]
    Gone,
}

impl InputStream {
    /// Tear the stream down without ever blocking the caller. A WASAPI stream
    /// whose device was invalidated can hang in its destructor, so on Windows
    /// the drop happens on a throwaway thread; elsewhere `cpal::Stream` is not
    /// `Send` and the drop is well behaved.
    pub fn dispose(mut self) {
        self.teardown();
    }

    fn teardown(&mut self) {
        match std::mem::replace(&mut self.inner, Inner::Gone) {
            Inner::Cpal(Some(s)) => dispose_stream(s),
            #[cfg(windows)]
            Inner::Process(p) => drop(p),
            _ => {}
        }
    }

    /// Capture only what one process tree plays (Windows 10 2004+).
    #[cfg(windows)]
    pub fn process_loopback(pid: u32, target_hz: u32, on_audio: AudioSink, on_error: ErrorSink) -> Result<InputStream, StreamError> {
        let p = crate::process_loopback::open_process_loopback(pid, target_hz, on_audio, on_error)?;
        Ok(InputStream { inner: Inner::Process(p), device_rate: 48_000, sample_rate: target_hz })
    }
}

impl Drop for InputStream {
    fn drop(&mut self) {
        self.teardown();
    }
}

#[cfg(windows)]
fn dispose_stream(s: Stream) {
    let _ = std::thread::Builder::new().name("hark-stream-drop".into()).spawn(move || drop(s));
}

#[cfg(not(windows))]
fn dispose_stream(s: Stream) {
    drop(s);
}

/// Open an input (capture) stream on `device`. For an *output* device this
/// yields system-audio loopback (WASAPI on Windows, CoreAudio taps on macOS 14.6+).
///
/// `on_audio` receives mono f32 frames at `target_hz`.
pub fn open_input(
    device: &cpal::Device,
    loopback: bool,
    target_hz: u32,
    on_audio: AudioSink,
    on_error: ErrorSink,
) -> Result<InputStream, StreamError> {
    let supported = if loopback { device.default_output_config() } else { device.default_input_config() }
        .map_err(|e| StreamError::Cpal(e.to_string()))?;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.config();
    let channels = config.channels as usize;
    let device_rate = config.sample_rate;
    let resampler = Arc::new(Mutex::new(Resampler::new(device_rate, target_hz)));

    let err_cb = {
        let on_error = on_error.clone();
        move |e: cpal::Error| on_error(e.to_string())
    };

    macro_rules! build {
        ($t:ty, $conv:expr) => {{
            let on_audio = on_audio.clone();
            let resampler = resampler.clone();
            device
                .build_input_stream::<$t, _, _>(
                    config.clone(),
                    move |data: &[$t], _| {
                        let f: Vec<f32> = data.iter().map($conv).collect();
                        let out = resampler.lock().process(&to_mono(&f, channels));
                        if !out.is_empty() {
                            on_audio(&out);
                        }
                    },
                    err_cb,
                    None,
                )
                .map_err(|e| StreamError::Cpal(e.to_string()))?
        }};
    }

    let stream = match sample_format {
        SampleFormat::F32 => build!(f32, |s: &f32| *s),
        SampleFormat::I16 => build!(i16, |s: &i16| *s as f32 / i16::MAX as f32),
        SampleFormat::U16 => build!(u16, |s: &u16| (*s as f32 - 32768.0) / 32768.0),
        SampleFormat::I32 => build!(i32, |s: &i32| *s as f32 / i32::MAX as f32),
        SampleFormat::U8 => build!(u8, |s: &u8| (*s as f32 - 128.0) / 128.0),
        other => return Err(StreamError::Cpal(format!("unsupported sample format {other:?}"))),
    };
    stream.play().map_err(|e| StreamError::Cpal(e.to_string()))?;
    Ok(InputStream { inner: Inner::Cpal(Some(stream)), device_rate, sample_rate: target_hz })
}
