//! Mic + system-audio recorder producing archive WAVs and a live 16 kHz feed.

use crate::devices::find_device;
use crate::dsp::{mix, rms_db, Resampler};
use crate::stream::{open_input, InputStream, StreamError};
use crate::wav::WavWriter;
use crossbeam_channel::Sender;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const ARCHIVE_HZ: u32 = 48_000;
pub const ASR_HZ: u32 = 16_000;
const TICK: Duration = Duration::from_millis(100);

#[derive(Debug, Clone)]
pub struct RecordConfig {
    pub dir: PathBuf,
    pub mic_device: Option<String>,
    pub loopback_device: Option<String>,
    pub capture_system: bool,
}

#[derive(Debug, Clone)]
pub enum CaptureEvent {
    /// Every ~100 ms.
    Levels { mic_db: f32, sys_db: f32 },
    /// Mixed mono 16 kHz audio, ~100 ms per message. Not sent while paused.
    Pcm16k(Vec<f32>),
    /// Non-fatal: e.g. system audio unavailable, recording continues mic-only.
    Warning(String),
    /// Fatal stream error; the recorder keeps running but data may be missing.
    Error(String),
}

#[derive(Debug, Clone)]
pub struct RecordOutput {
    pub mic_wav: PathBuf,
    pub sys_wav: Option<PathBuf>,
    pub mix_wav: PathBuf,
    pub duration: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    #[error("no microphone available")]
    NoMic,
    #[error(transparent)]
    Stream(#[from] StreamError),
    #[error("wav: {0}")]
    Wav(#[from] hound::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

struct Shared {
    mic_buf: Mutex<Vec<f32>>,
    sys_buf: Mutex<Vec<f32>>,
    paused: AtomicBool,
    stop: AtomicBool,
}

pub struct Recorder {
    shared: Arc<Shared>,
    _mic: InputStream,
    _sys: Option<InputStream>,
    worker: Option<JoinHandle<Result<Duration, RecordError>>>,
    started: Instant,
    paused_total: Arc<Mutex<Duration>>,
    paused_since: Mutex<Option<Instant>>,
    out: RecordOutput,
}

impl Recorder {
    pub fn start(cfg: RecordConfig, tx: Sender<CaptureEvent>) -> Result<Recorder, RecordError> {
        std::fs::create_dir_all(&cfg.dir)?;
        let shared = Arc::new(Shared {
            mic_buf: Mutex::new(Vec::with_capacity(48_000)),
            sys_buf: Mutex::new(Vec::with_capacity(48_000)),
            paused: AtomicBool::new(false),
            stop: AtomicBool::new(false),
        });

        let err_tx = tx.clone();
        let on_error: crate::stream::ErrorSink = Arc::new(move |e| {
            let _ = err_tx.send(CaptureEvent::Error(e));
        });

        // Microphone (required).
        let mic_dev = find_device(cfg.mic_device.as_deref(), false).ok_or(RecordError::NoMic)?;
        let mic_shared = shared.clone();
        let mic = open_input(
            &mic_dev,
            false,
            Arc::new(move |pcm| mic_shared.mic_buf.lock().extend_from_slice(pcm)),
            on_error.clone(),
        )?;

        // System audio (optional).
        let sys = if cfg.capture_system {
            match find_device(cfg.loopback_device.as_deref(), true) {
                Some(dev) => {
                    let sys_shared = shared.clone();
                    match open_input(
                        &dev,
                        true,
                        Arc::new(move |pcm| sys_shared.sys_buf.lock().extend_from_slice(pcm)),
                        on_error.clone(),
                    ) {
                        Ok(s) => Some(s),
                        Err(e) => {
                            let _ = tx.send(CaptureEvent::Warning(format!(
                                "System audio unavailable ({e}); recording microphone only."
                            )));
                            None
                        }
                    }
                }
                None => {
                    let _ = tx.send(CaptureEvent::Warning("No output device for system audio; mic only.".into()));
                    None
                }
            }
        } else {
            None
        };

        let mic_wav = cfg.dir.join("mic.wav");
        let sys_wav = sys.as_ref().map(|_| cfg.dir.join("sys.wav"));
        let mix_wav = cfg.dir.join("mix.wav");
        let out = RecordOutput { mic_wav: mic_wav.clone(), sys_wav: sys_wav.clone(), mix_wav: mix_wav.clone(), duration: Duration::ZERO };

        let worker = {
            let shared = shared.clone();
            let mic_hz = mic.sample_rate;
            let sys_hz = sys.as_ref().map(|s| s.sample_rate);
            let tx = tx.clone();
            std::thread::Builder::new().name("hark-capture".into()).spawn(move || {
                run_worker(shared, tx, mic_hz, sys_hz, mic_wav, sys_wav, mix_wav)
            })?
        };

        Ok(Recorder {
            shared,
            _mic: mic,
            _sys: sys,
            worker: Some(worker),
            started: Instant::now(),
            paused_total: Arc::new(Mutex::new(Duration::ZERO)),
            paused_since: Mutex::new(None),
            out,
        })
    }

    pub fn pause(&self) {
        if !self.shared.paused.swap(true, Ordering::SeqCst) {
            *self.paused_since.lock() = Some(Instant::now());
        }
    }

    pub fn resume(&self) {
        if self.shared.paused.swap(false, Ordering::SeqCst) {
            if let Some(t) = self.paused_since.lock().take() {
                *self.paused_total.lock() += t.elapsed();
            }
        }
    }

    pub fn is_paused(&self) -> bool {
        self.shared.paused.load(Ordering::SeqCst)
    }

    /// Wall time recorded, excluding paused time.
    pub fn elapsed(&self) -> Duration {
        let paused = *self.paused_total.lock() + self.paused_since.lock().map(|t| t.elapsed()).unwrap_or_default();
        self.started.elapsed().saturating_sub(paused)
    }

    pub fn stop(mut self) -> Result<RecordOutput, RecordError> {
        self.resume();
        self.shared.stop.store(true, Ordering::SeqCst);
        let duration = match self.worker.take() {
            Some(h) => h.join().map_err(|_| RecordError::Io(std::io::Error::other("capture thread panicked")))??,
            None => Duration::ZERO,
        };
        let mut out = self.out.clone();
        out.duration = duration;
        Ok(out)
    }
}

#[allow(clippy::too_many_arguments)]
fn run_worker(
    shared: Arc<Shared>,
    tx: Sender<CaptureEvent>,
    mic_hz: u32,
    sys_hz: Option<u32>,
    mic_wav: PathBuf,
    sys_wav: Option<PathBuf>,
    mix_wav: PathBuf,
) -> Result<Duration, RecordError> {
    let mut mic_w = WavWriter::create(&mic_wav, ARCHIVE_HZ, 1)?;
    let mut sys_w = match &sys_wav {
        Some(p) => Some(WavWriter::create(p, ARCHIVE_HZ, 1)?),
        None => None,
    };
    let mut mix_w = WavWriter::create(&mix_wav, ARCHIVE_HZ, 1)?;

    let mut mic_to48 = Resampler::new(mic_hz, ARCHIVE_HZ);
    let mut mic_to16 = Resampler::new(mic_hz, ASR_HZ);
    let mut sys_to48 = sys_hz.map(|hz| Resampler::new(hz, ARCHIVE_HZ));
    let mut sys_to16 = sys_hz.map(|hz| Resampler::new(hz, ASR_HZ));

    let mut written_frames: u64 = 0;
    loop {
        std::thread::sleep(TICK);
        let stopping = shared.stop.load(Ordering::SeqCst);
        let mic_raw: Vec<f32> = std::mem::take(&mut *shared.mic_buf.lock());
        let sys_raw: Vec<f32> = std::mem::take(&mut *shared.sys_buf.lock());

        if shared.paused.load(Ordering::SeqCst) && !stopping {
            let _ = tx.send(CaptureEvent::Levels { mic_db: -100.0, sys_db: -100.0 });
            continue;
        }

        let mic_db = rms_db(&mic_raw);
        let sys_db = if sys_hz.is_some() { rms_db(&sys_raw) } else { -100.0 };
        let _ = tx.send(CaptureEvent::Levels { mic_db, sys_db });

        let mic48 = mic_to48.process(&mic_raw);
        let sys48 = match sys_to48.as_mut() {
            Some(r) => r.process(&sys_raw),
            None => Vec::new(),
        };
        mic_w.write(&mic48)?;
        if let Some(w) = sys_w.as_mut() {
            w.write(&sys48)?;
        }
        // fable: per-tick padding means the two clocks can drift a few ms per
        // minute in the mix; the separate tracks stay sample-accurate.
        let mixed48 = mix(&mic48, &sys48, 1.0, 1.0);
        written_frames += mixed48.len() as u64;
        mix_w.write(&mixed48)?;

        let mic16 = mic_to16.process(&mic_raw);
        let sys16 = match sys_to16.as_mut() {
            Some(r) => r.process(&sys_raw),
            None => Vec::new(),
        };
        let mixed16 = mix(&mic16, &sys16, 1.0, 1.0);
        if !mixed16.is_empty() {
            let _ = tx.send(CaptureEvent::Pcm16k(mixed16));
        }

        if stopping {
            break;
        }
    }
    mic_w.finish()?;
    if let Some(w) = sys_w {
        w.finish()?;
    }
    mix_w.finish()?;
    Ok(Duration::from_secs_f64(written_frames as f64 / ARCHIVE_HZ as f64))
}
