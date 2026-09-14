//! Orchestrates one recording: capture + optional video + live captions, then
//! the post-call pipeline (mux, quality transcription).

use crate::events::{self, LevelsPayload, NoticePayload, RecordingStatePayload};
use crate::state::AppState;
use crate::windows;
use hark_asr::{Caption, LiveTranscriber};
use hark_capture::{CaptureEvent, RecordConfig, Recorder, VideoRecorder, VideoTarget};
use hark_store::{Meeting, MeetingStatus};
use serde::Deserialize;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct StartOptions {
    pub title: Option<String>,
    pub app: Option<String>,
    pub video: Option<bool>,
    pub target: Option<VideoTarget>,
}

pub struct ActiveRecording {
    pub meeting: Meeting,
    recorder: Option<Recorder>,
    video: Option<VideoRecorder>,
    /// Feeds live ASR; dropping it ends the transcriber.
    pcm_tx: Option<crossbeam_channel::Sender<Vec<f32>>>,
    live: Option<LiveTranscriber>,
    forwarder: Option<JoinHandle<()>>,
    pub highlights: Vec<u64>,
}

pub fn status_payload(state: &AppState) -> RecordingStatePayload {
    match state.recording.lock().as_ref() {
        Some(a) => RecordingStatePayload {
            state: if a.recorder.as_ref().map(|r| r.is_paused()).unwrap_or(false) { "paused" } else { "recording" },
            meeting_id: Some(a.meeting.id.clone()),
            elapsed_ms: a.recorder.as_ref().map(|r| r.elapsed().as_millis() as u64).unwrap_or(0),
        },
        None => RecordingStatePayload { state: "idle", meeting_id: None, elapsed_ms: 0 },
    }
}

fn emit_state(app: &AppHandle) {
    let state = app.state::<AppState>();
    let _ = app.emit(events::RECORDING_STATE, status_payload(&state));
}

pub fn start(app: &AppHandle, opts: StartOptions) -> Result<Meeting, String> {
    let state = app.state::<AppState>();
    if state.recording.lock().is_some() {
        return Err("already recording".into());
    }
    let settings = state.settings.read().clone();
    let title = opts.title.clone().unwrap_or_else(|| default_title(opts.app.as_deref()));
    let mut meeting = Meeting::new_recording(&title, opts.app.as_deref());
    let want_video = opts.video.unwrap_or(settings.video_enabled);
    let dir = state.store.recordings_dir(&meeting.id);

    log::info!("recorder: opening audio devices");
    let (cap_tx, cap_rx) = crossbeam_channel::unbounded::<CaptureEvent>();
    let recorder = Recorder::start(
        RecordConfig {
            dir: dir.clone(),
            mic_device: settings.mic_device.clone(),
            loopback_device: settings.loopback_device.clone(),
            capture_system: settings.capture_system,
        },
        cap_tx,
    )
    .map_err(|e| e.to_string())?;

    // Optional screen video.
    let video = if want_video {
        match state.ffmpeg.as_ref() {
            Some(ffmpeg) => {
                let target = opts.target.clone().unwrap_or_else(|| settings.video_target.clone());
                match VideoRecorder::start(ffmpeg, &target, &dir.join("screen.raw.mp4"), settings.video_fps, settings.video_max_height) {
                    Ok(v) => {
                        meeting.has_video = true;
                        Some(v)
                    }
                    Err(e) => {
                        notice(app, "warning", format!("Screen recording failed to start: {e}. Recording audio only."));
                        None
                    }
                }
            }
            None => {
                notice(app, "warning", "ffmpeg not found; recording audio only.".into());
                None
            }
        }
    } else {
        None
    };

    state.store.create_meeting(&meeting).map_err(|e| e.to_string())?;
    log::info!("recorder: audio open, loading live model");

    // Live captions if the live model is downloaded.
    let tier = settings.tier(&state.hardware);
    let (live_id, _, _) = settings.model_ids(&state.catalog, tier);
    let live_engine = state.whisper_or_any(&[&live_id]);
    log::info!("recorder: live model loaded={}", live_engine.is_some());
    let (pcm_tx, live) = match live_engine {
        Some(engine) => {
            let (pcm_tx, pcm_rx) = crossbeam_channel::unbounded::<Vec<f32>>();
            let (cap_tx2, cap_rx2) = crossbeam_channel::unbounded::<Caption>();
            let live = LiveTranscriber::start(engine, pcm_rx, cap_tx2, settings.language.clone());
            let app2 = app.clone();
            std::thread::spawn(move || {
                for c in cap_rx2.iter() {
                    let _ = app2.emit(events::CAPTION, &c);
                }
            });
            (Some(pcm_tx), Some(live))
        }
        None => {
            notice(app, "info", "Live captions off: transcription model not downloaded yet.".into());
            (None, None)
        }
    };

    // Forward capture events to the UI and the live transcriber.
    let forwarder = {
        let app = app.clone();
        let pcm_tx = pcm_tx.clone();
        std::thread::spawn(move || {
            for ev in cap_rx.iter() {
                match ev {
                    CaptureEvent::Levels { mic_db, sys_db } => {
                        let _ = app.emit(events::LEVELS, LevelsPayload { mic_db, sys_db });
                    }
                    CaptureEvent::Pcm16k(pcm) => {
                        if let Some(tx) = &pcm_tx {
                            let _ = tx.send(pcm);
                        }
                    }
                    CaptureEvent::Warning(m) => notice(&app, "warning", m),
                    CaptureEvent::Error(m) => notice(&app, "error", m),
                }
            }
        })
    };

    *state.recording.lock() = Some(ActiveRecording {
        meeting: meeting.clone(),
        recorder: Some(recorder),
        video,
        pcm_tx,
        live,
        forwarder: Some(forwarder),
        highlights: Vec::new(),
    });
    state.detection_paused.store(true, Ordering::SeqCst);
    log::info!("recorder: showing record bar");
    windows::hide_popup(app);
    windows::show_recordbar(app);
    emit_state(app);
    log::info!("recorder: started {}", meeting.id);
    Ok(meeting)
}

pub fn pause(app: &AppHandle) {
    let state = app.state::<AppState>();
    if let Some(a) = state.recording.lock().as_ref() {
        if let Some(r) = &a.recorder {
            r.pause();
        }
    }
    emit_state(app);
}

pub fn resume(app: &AppHandle) {
    let state = app.state::<AppState>();
    if let Some(a) = state.recording.lock().as_ref() {
        if let Some(r) = &a.recorder {
            r.resume();
        }
    }
    emit_state(app);
}

pub fn mark_highlight(app: &AppHandle) -> Option<u64> {
    let state = app.state::<AppState>();
    let mut g = state.recording.lock();
    let a = g.as_mut()?;
    let ms = a.recorder.as_ref()?.elapsed().as_millis() as u64;
    a.highlights.push(ms);
    Some(ms)
}

pub fn stop(app: &AppHandle) -> Result<Meeting, String> {
    let state = app.state::<AppState>();
    let mut active = state.recording.lock().take().ok_or("not recording")?;
    state.detection_paused.store(false, Ordering::SeqCst);
    windows::hide_recordbar(app);

    let out = active.recorder.take().ok_or("recorder missing")?.stop().map_err(|e| e.to_string())?;
    if let Some(v) = active.video.take() {
        let _ = v.stop();
    }
    // Close the live feed and wait for the last window.
    drop(active.pcm_tx.take());
    if let Some(l) = active.live.take() {
        l.join();
    }
    if let Some(f) = active.forwarder.take() {
        let _ = f.join();
    }

    let mut meeting = active.meeting.clone();
    meeting.ended_at = Some(chrono::Utc::now());
    meeting.duration_ms = out.duration.as_millis() as i64;
    meeting.status = MeetingStatus::Processing;
    state.store.update_meeting(&meeting).map_err(|e| e.to_string())?;
    if !active.highlights.is_empty() {
        let _ = state.store.set_setting(&format!("highlights:{}", meeting.id), &active.highlights);
    }
    emit_state(app);

    let app2 = app.clone();
    let m2 = meeting.clone();
    std::thread::spawn(move || crate::pipeline::post_process(&app2, m2));
    Ok(meeting)
}

/// Re-run the quality transcription for an existing meeting (e.g. after downloading a model).
pub fn retranscribe(app: &AppHandle, meeting_id: &str, asr_model: Option<String>) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut m = state.store.get_meeting(meeting_id).map_err(|e| e.to_string())?.ok_or("no such meeting")?;
    if m.status == MeetingStatus::Recording {
        return Err("still recording".into());
    }
    m.status = MeetingStatus::Processing;
    state.store.update_meeting(&m).map_err(|e| e.to_string())?;
    let app2 = app.clone();
    std::thread::spawn(move || crate::pipeline::post_process_with(&app2, m, crate::pipeline::PostOpts { asr_model }));
    Ok(())
}

pub fn read_wav_as_16k_mono(path: &Path) -> Result<Vec<f32>, String> {
    let reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    let ch = spec.channels as usize;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let max = ((1u64 << (spec.bits_per_sample - 1)) - 1) as f32;
            reader.into_samples::<i32>().map(|s| s.map(|v| v as f32 / max)).collect::<Result<_, _>>().map_err(|e| e.to_string())?
        }
        hound::SampleFormat::Float => reader.into_samples::<f32>().collect::<Result<_, _>>().map_err(|e| e.to_string())?,
    };
    let mono = hark_capture::dsp::to_mono(&samples, ch);
    let mut rs = hark_capture::dsp::Resampler::new(spec.sample_rate, hark_capture::ASR_HZ);
    let mut out = rs.process(&mono);
    // Push a tail of silence so the final samples make it through the filter delay.
    out.extend(rs.process(&vec![0.0; 64]));
    Ok(out)
}

fn default_title(app: Option<&str>) -> String {
    let when = chrono::Local::now().format("%b %-d, %-I:%M %p");
    match app {
        Some(a) => format!("{} call - {when}", capitalize(a)),
        None => format!("Recording - {when}"),
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

pub fn notice(app: &AppHandle, level: &'static str, message: String) {
    log::info!("[{level}] {message}");
    let _ = app.emit(events::NOTICE, NoticePayload { level, message });
}
