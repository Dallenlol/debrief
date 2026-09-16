//! Mic + system-audio recorder producing archive WAVs and a live 16 kHz feed.

use crate::devices::find_device;
use crate::dsp::{mix, rms_db, Resampler};
use crate::stream::{open_input, InputStream, StreamError};
use crate::wav::WavWriter;
use crossbeam_channel::Sender;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const ARCHIVE_HZ: u32 = 48_000;
pub const ASR_HZ: u32 = 16_000;
const TICK: Duration = Duration::from_millis(100);
/// No microphone callback for this long while unpaused means the stream died
/// (a device switch or unplug); the mic always delivers, even silence.
const STALL_AFTER: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub struct RecordConfig {
    pub dir: PathBuf,
    pub mic_device: Option<String>,
    pub loopback_device: Option<String>,
    pub capture_system: bool,
    /// Capture system audio only from this process tree (Windows); `None` = everything.
    pub audio_pid: Option<u32>,
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
    /// A stream stopped delivering audio (device switched or unplugged). The
    /// owner should call [`Recorder::reopen`]; the recorder itself keeps going.
    Stalled { mic: bool, sys: bool },
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
    /// Millisecond timestamps (since `epoch`) of the last mic / system callback.
    mic_seen_ms: AtomicU64,
    sys_seen_ms: AtomicU64,
    /// Set by a stream error callback; cleared by `reopen`.
    stream_failed: AtomicBool,
    epoch: Instant,
}

impl Shared {
    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }
}

pub struct Recorder {
    shared: Arc<Shared>,
    cfg: RecordConfig,
    on_error: crate::stream::ErrorSink,
    mic: Option<InputStream>,
    sys: Option<InputStream>,
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
            mic_seen_ms: AtomicU64::new(0),
            sys_seen_ms: AtomicU64::new(0),
            stream_failed: AtomicBool::new(false),
            epoch: Instant::now(),
        });

        let err_tx = tx.clone();
        let err_shared = shared.clone();
        let on_error: crate::stream::ErrorSink = Arc::new(move |e| {
            // Buffer over/underruns are dropped packets, not a dead stream.
            if is_fatal_stream_error(&e) {
                err_shared.stream_failed.store(true, Ordering::SeqCst);
                let _ = err_tx.send(CaptureEvent::Error(e));
            } else {
                let _ = err_tx.send(CaptureEvent::Warning(e));
            }
        });

        // Microphone (required).
        let mic = open_mic(&cfg, &shared, &on_error)?;
        // System audio (optional).
        let sys = open_sys(&cfg, &shared, &on_error, &tx);

        let mic_wav = cfg.dir.join("mic.wav");
        let sys_wav = sys.as_ref().map(|_| cfg.dir.join("sys.wav"));
        let mix_wav = cfg.dir.join("mix.wav");
        let out = RecordOutput { mic_wav: mic_wav.clone(), sys_wav: sys_wav.clone(), mix_wav: mix_wav.clone(), duration: Duration::ZERO };

        let worker = {
            let shared = shared.clone();
            let has_sys = sys.is_some();
            let tx = tx.clone();
            std::thread::Builder::new().name("hark-capture".into()).spawn(move || {
                run_worker(shared, tx, has_sys, mic_wav, sys_wav, mix_wav)
            })?
        };

        Ok(Recorder {
            shared,
            cfg,
            on_error,
            mic: Some(mic),
            sys,
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

    /// Rebuild the audio streams on the current devices after a stall or
    /// error (the user switched or unplugged a device). Dead streams are
    /// disposed without blocking; the worker keeps its writers, so the
    /// archive files simply continue.
    pub fn reopen(&mut self) -> Result<(), RecordError> {
        if let Some(old) = self.mic.take() {
            old.dispose();
        }
        if let Some(old) = self.sys.take() {
            old.dispose();
        }
        // Whatever the dead streams left behind is stale.
        self.shared.mic_buf.lock().clear();
        self.shared.sys_buf.lock().clear();
        self.shared.stream_failed.store(false, Ordering::SeqCst);
        let (tx, _rx) = crossbeam_channel::bounded::<CaptureEvent>(4);
        self.mic = Some(open_mic(&self.cfg, &self.shared, &self.on_error)?);
        self.sys = open_sys(&self.cfg, &self.shared, &self.on_error, &tx);
        let now = self.shared.now_ms();
        self.shared.mic_seen_ms.store(now, Ordering::SeqCst);
        self.shared.sys_seen_ms.store(now, Ordering::SeqCst);
        Ok(())
    }

    /// True when a stream reported a fatal error since the last `reopen`.
    pub fn stream_failed(&self) -> bool {
        self.shared.stream_failed.load(Ordering::SeqCst)
    }

    pub fn stop(mut self) -> Result<RecordOutput, RecordError> {
        self.resume();
        self.shared.stop.store(true, Ordering::SeqCst);
        let duration = match self.worker.take() {
            Some(h) => h.join().map_err(|_| RecordError::Io(std::io::Error::other("capture thread panicked")))??,
            None => Duration::ZERO,
        };
        // Streams are disposed off-thread (see `InputStream::dispose`) so a dead
        // device can never hang the stop.
        if let Some(m) = self.mic.take() {
            m.dispose();
        }
        if let Some(s) = self.sys.take() {
            s.dispose();
        }
        let mut out = self.out.clone();
        out.duration = duration;
        Ok(out)
    }
}

/// cpal funnels transient glitches through the same callback as device loss.
pub fn is_fatal_stream_error(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    !(m.contains("underrun") || m.contains("overrun"))
}

fn open_mic(cfg: &RecordConfig, shared: &Arc<Shared>, on_error: &crate::stream::ErrorSink) -> Result<InputStream, RecordError> {
    let mic_dev = find_device(cfg.mic_device.as_deref(), false).ok_or(RecordError::NoMic)?;
    let mic_shared = shared.clone();
    Ok(open_input(
        &mic_dev,
        false,
        ARCHIVE_HZ,
        Arc::new(move |pcm| {
            mic_shared.mic_seen_ms.store(mic_shared.now_ms(), Ordering::Relaxed);
            mic_shared.mic_buf.lock().extend_from_slice(pcm)
        }),
        on_error.clone(),
    )?)
}

fn open_sys(cfg: &RecordConfig, shared: &Arc<Shared>, on_error: &crate::stream::ErrorSink, tx: &Sender<CaptureEvent>) -> Option<InputStream> {
    if !cfg.capture_system {
        return None;
    }
    #[cfg(windows)]
    if let Some(pid) = cfg.audio_pid {
        let sys_shared = shared.clone();
        match InputStream::process_loopback(
            pid,
            ARCHIVE_HZ,
            Arc::new(move |pcm| {
                sys_shared.sys_seen_ms.store(sys_shared.now_ms(), Ordering::Relaxed);
                sys_shared.sys_buf.lock().extend_from_slice(pcm)
            }),
            on_error.clone(),
        ) {
            Ok(s) => return Some(s),
            Err(e) => {
                let _ = tx.send(CaptureEvent::Warning(format!("Could not capture that app's audio alone ({e}); recording all system audio instead.")));
            }
        }
    }
    let Some(dev) = find_device(cfg.loopback_device.as_deref(), true) else {
        let _ = tx.send(CaptureEvent::Warning("No output device for system audio; mic only.".into()));
        return None;
    };
    let sys_shared = shared.clone();
    match open_input(
        &dev,
        true,
        ARCHIVE_HZ,
        Arc::new(move |pcm| {
            sys_shared.sys_seen_ms.store(sys_shared.now_ms(), Ordering::Relaxed);
            sys_shared.sys_buf.lock().extend_from_slice(pcm)
        }),
        on_error.clone(),
    ) {
        Ok(s) => Some(s),
        Err(e) => {
            let _ = tx.send(CaptureEvent::Warning(format!("System audio unavailable ({e}); recording microphone only.")));
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_worker(
    shared: Arc<Shared>,
    tx: Sender<CaptureEvent>,
    has_sys: bool,
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
    // Streams already deliver ARCHIVE_HZ; only the live feed needs another rate.
    let mut to16 = Resampler::new(ARCHIVE_HZ, ASR_HZ);

    let mut written_frames: u64 = 0;
    let mut last_stall_report: Option<Instant> = None;
    loop {
        std::thread::sleep(TICK);
        let stopping = shared.stop.load(Ordering::SeqCst);
        let mic48: Vec<f32> = std::mem::take(&mut *shared.mic_buf.lock());
        let sys48: Vec<f32> = std::mem::take(&mut *shared.sys_buf.lock());

        if shared.paused.load(Ordering::SeqCst) && !stopping {
            let _ = tx.send(CaptureEvent::Levels { mic_db: -100.0, sys_db: -100.0 });
            continue;
        }

        // Stall watch: the mic callback went quiet, or a stream reported an
        // error. Reported at most every 5 s until `reopen` clears it.
        let now_ms = shared.now_ms();
        let stall_ms = STALL_AFTER.as_millis() as u64;
        let mic_stalled = now_ms.saturating_sub(shared.mic_seen_ms.load(Ordering::Relaxed)) > stall_ms;
        let failed = shared.stream_failed.load(Ordering::SeqCst);
        if (mic_stalled || failed) && !stopping {
            if last_stall_report.map(|t| t.elapsed() >= Duration::from_secs(5)).unwrap_or(true) {
                last_stall_report = Some(Instant::now());
                let sys_stalled = has_sys && now_ms.saturating_sub(shared.sys_seen_ms.load(Ordering::Relaxed)) > stall_ms;
                let _ = tx.send(CaptureEvent::Stalled { mic: mic_stalled || failed, sys: sys_stalled || failed });
            }
        } else {
            last_stall_report = None;
        }

        let mic_db = rms_db(&mic48);
        let sys_db = if has_sys { rms_db(&sys48) } else { -100.0 };
        let _ = tx.send(CaptureEvent::Levels { mic_db, sys_db });

        mic_w.write(&mic48)?;
        if let Some(w) = sys_w.as_mut() {
            w.write(&sys48)?;
        }
        // fable: per-tick padding means the two clocks can drift a few ms per
        // minute in the mix; the separate tracks stay sample-accurate.
        let mixed48 = mix(&mic48, &sys48, 1.0, 1.0);
        written_frames += mixed48.len() as u64;
        mix_w.write(&mixed48)?;

        let mixed16 = to16.process(&mixed48);
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
