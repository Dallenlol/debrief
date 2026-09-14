//! Small, allocation-light audio helpers: downmix, levels, mixing, resampling.

/// Average interleaved channels into mono.
pub fn to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// RMS level in dBFS. Silence (or empty input) reports -100.0.
pub fn rms_db(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return -100.0;
    }
    let mean_sq = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
    if mean_sq <= 1e-10 {
        return -100.0;
    }
    (20.0 * mean_sq.sqrt().log10()).max(-100.0)
}

/// Sum two mono buffers with gains; output length is the longer input. Clamped to [-1, 1].
pub fn mix(a: &[f32], b: &[f32], gain_a: f32, gain_b: f32) -> Vec<f32> {
    let n = a.len().max(b.len());
    (0..n)
        .map(|i| {
            let x = a.get(i).copied().unwrap_or(0.0) * gain_a + b.get(i).copied().unwrap_or(0.0) * gain_b;
            x.clamp(-1.0, 1.0)
        })
        .collect()
}

/// Streaming mono resampler: windowed-sinc band-limited interpolation.
///
/// Quality is more than enough for speech (Whisper's input is 16 kHz), and
/// the cost is ~`TAPS` multiply-adds per output sample.
pub struct Resampler {
    ratio: f64, // in / out
    cutoff: f64, // normalized to input rate, in cycles/sample
    history: Vec<f32>, // trailing input samples kept for continuity
    phase: f64, // position (in input samples) of the next output sample, relative to history[0]
}

const TAPS: usize = 32; // total taps (half each side)

impl Resampler {
    pub fn new(from_hz: u32, to_hz: u32) -> Self {
        let ratio = from_hz as f64 / to_hz as f64;
        // Lowpass at 0.45 of the lower Nyquist to leave room for the window's transition.
        let cutoff = 0.45 / ratio.max(1.0);
        Resampler { ratio, cutoff, history: Vec::with_capacity(4096), phase: 0.0 }
    }

    pub fn is_identity(&self) -> bool {
        (self.ratio - 1.0).abs() < 1e-9
    }

    /// Feed input samples, get back however many output samples are now available.
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if self.is_identity() {
            return input.to_vec();
        }
        self.history.extend_from_slice(input);
        let half = (TAPS / 2) as f64;
        let mut out = Vec::with_capacity((input.len() as f64 / self.ratio) as usize + 2);
        // We can produce an output sample at `phase` while phase + half < history.len().
        while self.phase + half < self.history.len() as f64 {
            out.push(self.interpolate(self.phase));
            self.phase += self.ratio;
        }
        // Drop history that is no longer reachable (keep `half` before phase).
        let keep_from = (self.phase - half).floor().max(0.0) as usize;
        if keep_from > 0 {
            self.history.drain(..keep_from);
            self.phase -= keep_from as f64;
        }
        out
    }

    fn interpolate(&self, pos: f64) -> f32 {
        let center = pos.floor() as isize;
        let half = (TAPS / 2) as isize;
        let mut acc = 0.0f64;
        let mut wsum = 0.0f64;
        for k in (center - half + 1)..=(center + half) {
            if k < 0 || k as usize >= self.history.len() {
                continue;
            }
            let x = pos - k as f64; // distance in input samples
            let w = self.kernel(x);
            acc += w * self.history[k as usize] as f64;
            wsum += w;
        }
        if wsum.abs() < 1e-12 {
            0.0
        } else {
            (acc / wsum) as f32
        }
    }

    /// sinc lowpass * Hann window, evaluated at distance `x` (input samples).
    fn kernel(&self, x: f64) -> f64 {
        let half = (TAPS / 2) as f64;
        if x.abs() >= half {
            return 0.0;
        }
        let fc = self.cutoff; // cycles per input sample (<= 0.5)
        let sinc = if x.abs() < 1e-9 { 2.0 * fc } else { (2.0 * std::f64::consts::PI * fc * x).sin() / (std::f64::consts::PI * x) };
        let hann = 0.5 * (1.0 + (std::f64::consts::PI * x / half).cos());
        sinc * hann
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_downmix_averages() {
        assert_eq!(to_mono(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
        assert_eq!(to_mono(&[0.3, 0.7], 1), vec![0.3, 0.7]);
    }

    #[test]
    fn rms_db_silence_and_full_scale() {
        assert_eq!(rms_db(&[0.0; 100]), -100.0);
        assert_eq!(rms_db(&[]), -100.0);
        assert!((rms_db(&[1.0; 100]) - 0.0).abs() < 0.01);
        assert!((rms_db(&[0.5; 100]) + 6.02).abs() < 0.05);
    }

    #[test]
    fn mix_clamps_and_pads() {
        assert_eq!(mix(&[0.9], &[0.9], 1.0, 1.0), vec![1.0]);
        assert_eq!(mix(&[0.5], &[], 1.0, 1.0), vec![0.5]);
        assert_eq!(mix(&[], &[0.2, 0.4], 1.0, 0.5), vec![0.1, 0.2]);
    }

    #[test]
    fn resampler_48k_to_16k_ratio() {
        let mut r = Resampler::new(48000, 16000);
        let out = r.process(&vec![0.0; 48000]);
        assert!((out.len() as i64 - 16000).abs() < 100, "got {}", out.len());
    }

    #[test]
    fn resampler_preserves_a_1khz_tone() {
        let from = 44100u32;
        let to = 16000u32;
        let mut r = Resampler::new(from, to);
        let input: Vec<f32> = (0..from).map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / from as f32).sin()).collect();
        // Feed in realistic chunks to exercise the streaming path.
        let mut out = Vec::new();
        for chunk in input.chunks(480) {
            out.extend(r.process(chunk));
        }
        assert!((out.len() as i64 - to as i64).abs() < 100, "got {}", out.len());
        // Count zero crossings in the middle second-half: ~2000 per second for 1 kHz.
        let mid = &out[4000..12000];
        let zc = mid.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        let expected = 2 * 1000 * mid.len() / to as usize;
        assert!((zc as i64 - expected as i64).abs() < 20, "zc={zc} expected~{expected}");
        // Amplitude roughly preserved.
        let peak = mid.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.9 && peak < 1.05, "peak={peak}");
    }

    #[test]
    fn identity_resampler_passthrough() {
        let mut r = Resampler::new(16000, 16000);
        assert_eq!(r.process(&[0.1, 0.2]), vec![0.1, 0.2]);
    }
}
