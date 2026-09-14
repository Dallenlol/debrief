//! Thin cpal wrappers that deliver mono f32 audio at the device's native rate.

use crate::dsp::to_mono;
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    #[error("no audio device available")]
    NoDevice,
    #[error("audio: {0}")]
    Cpal(String),
}

/// A running input stream plus the format it delivers.
pub struct InputStream {
    _stream: Stream,
    pub sample_rate: u32,
}

/// Open an input (capture) stream on `device`. For an *output* device this
/// yields system-audio loopback (WASAPI on Windows, CoreAudio taps on macOS 14.6+).
///
/// `on_audio` receives mono f32 frames at `InputStream::sample_rate`.
pub fn open_input(
    device: &cpal::Device,
    loopback: bool,
    on_audio: Arc<dyn Fn(&[f32]) + Send + Sync>,
    on_error: Arc<dyn Fn(String) + Send + Sync>,
) -> Result<InputStream, StreamError> {
    let supported = if loopback { device.default_output_config() } else { device.default_input_config() }
        .map_err(|e| StreamError::Cpal(e.to_string()))?;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.config();
    let channels = config.channels as usize;
    let sample_rate = config.sample_rate;

    let err_cb = {
        let on_error = on_error.clone();
        move |e: cpal::Error| on_error(e.to_string())
    };

    macro_rules! build {
        ($t:ty, $conv:expr) => {{
            let on_audio = on_audio.clone();
            device
                .build_input_stream::<$t, _, _>(
                    config.clone(),
                    move |data: &[$t], _| {
                        let f: Vec<f32> = data.iter().map($conv).collect();
                        on_audio(&to_mono(&f, channels));
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
    Ok(InputStream { _stream: stream, sample_rate })
}
