//! Splits a continuous 16 kHz mono stream into overlapping windows for live ASR.

use std::time::Duration;

pub struct Chunker {
    sample_rate: usize,
    window: usize,
    overlap: usize,
    buf: Vec<f32>,
    /// Absolute sample index of `buf[0]`.
    base: usize,
}

impl Chunker {
    pub fn new(sample_rate: u32, window: Duration, overlap: Duration) -> Self {
        let sr = sample_rate as usize;
        let window = (window.as_secs_f64() * sr as f64) as usize;
        let overlap = ((overlap.as_secs_f64() * sr as f64) as usize).min(window / 2);
        Chunker { sample_rate: sr, window, overlap, buf: Vec::with_capacity(window * 2), base: 0 }
    }

    /// Push audio; returns `(start_seconds, samples)` when a full window is ready.
    pub fn push(&mut self, pcm: &[f32]) -> Option<(f64, Vec<f32>)> {
        self.buf.extend_from_slice(pcm);
        if self.buf.len() < self.window {
            return None;
        }
        let start = self.base as f64 / self.sample_rate as f64;
        let chunk = self.buf[..self.window].to_vec();
        let advance = self.window - self.overlap;
        self.buf.drain(..advance);
        self.base += advance;
        Some((start, chunk))
    }

    /// Whatever is left (>= 0.5 s), e.g. at stop.
    pub fn flush(&mut self) -> Option<(f64, Vec<f32>)> {
        if self.buf.len() < self.sample_rate / 2 {
            self.buf.clear();
            return None;
        }
        let start = self.base as f64 / self.sample_rate as f64;
        let chunk = std::mem::take(&mut self.buf);
        self.base += chunk.len();
        Some((start, chunk))
    }

    /// Seconds of audio consumed so far (start of the next window).
    pub fn position(&self) -> f64 {
        self.base as f64 / self.sample_rate as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_advance_by_window_minus_overlap() {
        let mut c = Chunker::new(16000, Duration::from_secs(6), Duration::from_secs(1));
        assert!(c.push(&vec![0.0; 16000 * 5]).is_none());
        let (s, chunk) = c.push(&vec![0.0; 16000 * 2]).unwrap(); // 7 s total
        assert_eq!(s, 0.0);
        assert_eq!(chunk.len(), 16000 * 6);
        // 7 s - 5 s advance = 2 s buffered; need 4 more.
        assert!(c.push(&vec![0.0; 16000 * 3]).is_none());
        let (s2, _) = c.push(&vec![0.0; 16000]).unwrap();
        assert_eq!(s2, 5.0);
    }

    #[test]
    fn flush_returns_remainder_only_if_long_enough() {
        let mut c = Chunker::new(16000, Duration::from_secs(6), Duration::from_secs(1));
        c.push(&vec![0.0; 4000]); // 0.25 s
        assert!(c.flush().is_none());
        c.push(&vec![0.0; 16000]); // 1 s
        let (s, chunk) = c.flush().unwrap();
        assert_eq!(s, 0.0);
        assert_eq!(chunk.len(), 16000);
        assert_eq!(c.position(), 1.0);
    }
}
