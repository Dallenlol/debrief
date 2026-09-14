//! Crash-tolerant 16-bit PCM WAV writer: flushes to disk about once per second
//! of audio so a crash loses at most that much.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

pub struct WavWriter {
    inner: Option<hound::WavWriter<BufWriter<File>>>,
    sample_rate: u32,
    since_flush: usize,
}

impl WavWriter {
    pub fn create(path: &Path, sample_rate: u32, channels: u16) -> hound::Result<WavWriter> {
        let spec = hound::WavSpec { channels, sample_rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        Ok(WavWriter { inner: Some(hound::WavWriter::create(path, spec)?), sample_rate, since_flush: 0 })
    }

    /// Write f32 samples in [-1, 1] (interleaved if channels > 1).
    pub fn write(&mut self, samples: &[f32]) -> hound::Result<()> {
        let w = self.inner.as_mut().expect("writer finished");
        for s in samples {
            w.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
        }
        self.since_flush += samples.len();
        if self.since_flush >= self.sample_rate as usize {
            w.flush()?;
            self.since_flush = 0;
        }
        Ok(())
    }

    /// Finalize the header. Safe to call once; dropping without it still
    /// leaves a readable file because `flush` updates the header.
    pub fn finish(mut self) -> hound::Result<()> {
        if let Some(w) = self.inner.take() {
            w.finalize()?;
        }
        Ok(())
    }
}

impl Drop for WavWriter {
    fn drop(&mut self) {
        if let Some(w) = self.inner.take() {
            let _ = w.finalize();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_writer_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.wav");
        let mut w = WavWriter::create(&p, 48000, 1).unwrap();
        w.write(&[0.25; 4800]).unwrap();
        w.finish().unwrap();
        let r = hound::WavReader::open(&p).unwrap();
        assert_eq!(r.spec().sample_rate, 48000);
        assert_eq!(r.spec().channels, 1);
        assert_eq!(r.len(), 4800);
        let first = r.into_samples::<i16>().next().unwrap().unwrap();
        assert!((first as f32 / i16::MAX as f32 - 0.25).abs() < 0.001);
    }

    #[test]
    fn dropped_writer_is_still_readable() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("b.wav");
        {
            let mut w = WavWriter::create(&p, 16000, 1).unwrap();
            w.write(&[0.1; 20000]).unwrap();
        }
        assert_eq!(hound::WavReader::open(&p).unwrap().len(), 20000);
    }
}
