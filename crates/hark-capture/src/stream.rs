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
    /// macOS per-app tap: the stream on its aggregate device, then the tap itself
    /// (destroyed after the stream stops).
    #[cfg(target_os = "macos")]
    MacTap(Option<Stream>, crate::mac_app_tap::AppTap),
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
            #[cfg(target_os = "macos")]
            Inner::MacTap(stream, tap) => {
                if let Some(s) = stream {
                    dispose_stream(s);
                }
                drop(tap);
            }
            _ => {}
        }
    }

    /// Capture only what one process tree plays (Windows 10 2004+).
    #[cfg(windows)]
    pub fn process_loopback(pid: u32, target_hz: u32, on_audio: AudioSink, on_error: ErrorSink) -> Result<InputStream, StreamError> {
        let p = crate::process_loopback::open_process_loopback(pid, target_hz, on_audio, on_error)?;
        Ok(InputStream { inner: Inner::Process(p), device_rate: 48_000, sample_rate: target_hz })
    }

    /// Capture only what one app (and its helper processes) plays (macOS 14.2+).
    #[cfg(target_os = "macos")]
    pub fn app_tap(pid: u32, target_hz: u32, on_audio: AudioSink, on_error: ErrorSink) -> Result<InputStream, StreamError> {
        use cpal::traits::HostTrait;
        let tap = crate::mac_app_tap::create(pid).map_err(StreamError::Cpal)?;
        // The private aggregate device appears in this process's device list shortly after creation.
        let host = cpal::default_host();
        let mut device = None;
        for _ in 0..20 {
            device = host.input_devices().ok().and_then(|mut it| it.find(|d| d.id().map(|id| id.to_string().contains(&tap.uid)).unwrap_or(false)));
            if device.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let device = device.ok_or_else(|| StreamError::Cpal("app audio tap device did not appear".into()))?;
        tracing::info!("macOS app audio tap on {} process(es)", tap.processes);
        let mut s = open_input(&device, false, target_hz, on_audio, on_error)?;
        let stream = match std::mem::replace(&mut s.inner, Inner::Gone) {
            Inner::Cpal(st) => st,
            _ => None,
        };
        Ok(InputStream { inner: Inner::MacTap(stream, tap), device_rate: s.device_rate, sample_rate: s.sample_rate })
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
