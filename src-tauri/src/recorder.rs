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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use parking_lot::Mutex;
use std::thread::JoinHandle;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct StartOptions {
    pub title: Option<String>,
    pub app: Option<String>,
    pub video: Option<bool>,
    pub target: Option<VideoTarget>,
    /// Capture system audio only from this process tree (from the window picker).
    #[serde(default)]
    pub audio_pid: Option<u32>,
}

pub struct ActiveRecording {
    pub meeting: Meeting,
    recorder: Option<Recorder>,
    video: Option<VideoRecorder>,
    /// Feeds live ASR; shared with the forwarder so `stop` can close it from here
    /// (the only sender) instead of waiting for the capture channel to drain.
    pcm_tx: Arc<Mutex<Option<crossbeam_channel::Sender<Vec<f32>>>>>,
    /// Loads the live model and starts the transcriber; yields it (or `None`) when joined.
    live: Option<std::thread::JoinHandle<Option<LiveTranscriber>>>,
    forwarder: Option<JoinHandle<()>>,
    forwarder_stop: Arc<AtomicBool>,
    pub highlights: Vec<u64>,
    /// Stops the running-notes loop.
    notes_stop: Arc<AtomicBool>,
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
    let event = crate::calendar::current_event(&state);
    let title = opts
        .title
        .clone()
        .or_else(|| event.as_ref().map(|e| e.title.clone()))
        .unwrap_or_else(|| default_title(opts.app.as_deref()));
    let mut meeting = Meeting::new_recording(&title, opts.app.as_deref());
    if let Some(e) = &event {
        meeting.calendar_uid = Some(e.uid.clone());
        meeting.participants = e.attendees.iter().filter(|a| !a.eq_ignore_ascii_case(settings.user_name.trim())).cloned().collect();
        log::info!("recorder: matched calendar event '{}'", e.title);
    }
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
            audio_pid: opts.audio_pid,
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
                        spawn_screen_name_reader(app.clone(), meeting.id.clone(), ffmpeg.clone(), target.clone(), dir.clone());
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
    log::info!("recorder: audio open");
    crate::live_feed::LiveFeed::begin(&state, &meeting.id);
    let notes_stop = Arc::new(AtomicBool::new(false));
    if settings.summary_enabled {
        crate::live_feed::spawn_notes_loop(app.clone(), meeting.id.clone(), notes_stop.clone());
    }

    // Live captions if a speech model is downloaded. Loading it can take many seconds (a cold
    // disk, a large model on the GPU), so it happens off the Record path: audio queues in the
    // channel meanwhile and the captions catch up once the model is ready.
    let tier = settings.tier(&state.hardware);
    let (live_id, _, _) = settings.model_ids(&state.catalog, tier);
    let (pcm_tx, pcm_rx) = crossbeam_channel::unbounded::<Vec<f32>>();
    let live = {
        let (app, lang) = (app.clone(), settings.language.clone());
        std::thread::spawn(move || -> Option<LiveTranscriber> {
            let state = app.state::<AppState>();
            let started = std::time::Instant::now();
            let Some(engine) = state.whisper_live(&live_id) else {
                notice(&app, "info", "Live captions off: transcription model not downloaded yet.".into());
                return None; // drops pcm_rx, so the forwarder's sends fail fast
            };
            log::info!("recorder: live model {} ready in {:.1}s", engine.model_id(), started.elapsed().as_secs_f32());
            let (cap_tx2, cap_rx2) = crossbeam_channel::unbounded::<Caption>();
            let live = LiveTranscriber::start(engine, pcm_rx, cap_tx2, lang);
            std::thread::spawn(move || {
                for c in cap_rx2.iter() {
                    crate::live_feed::LiveFeed::push(&app.state::<AppState>(), &c);
                    let _ = app.emit(events::CAPTION, &c);
                }
            });
            Some(live)
        })
    };
    let pcm_tx = Some(pcm_tx);

    // Forward capture events to the UI and the live transcriber. Exits on the
    // stop flag: the capture channel may stay open while a dead stream is
    // being disposed off-thread, so we never wait for it to close.
    let pcm_tx = Arc::new(Mutex::new(pcm_tx));
    let forwarder_stop = Arc::new(AtomicBool::new(false));
    let forwarder = {
        let app = app.clone();
        let pcm_tx = pcm_tx.clone();
        let stop = forwarder_stop.clone();
        std::thread::spawn(move || {
            loop {
                let ev = match cap_rx.recv_timeout(std::time::Duration::from_millis(300)) {
                    Ok(ev) => ev,
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        continue;
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                };
                match ev {
                    CaptureEvent::Levels { mic_db, sys_db } => {
                        let _ = app.emit(events::LEVELS, LevelsPayload { mic_db, sys_db });
                    }
                    CaptureEvent::Pcm16k(pcm) => {
                        if let Some(tx) = pcm_tx.lock().as_ref() {
                            let _ = tx.send(pcm);
                        }
                    }
                    CaptureEvent::Warning(m) if m.contains("underrun") || m.contains("overrun") => log::debug!("capture: {m}"),
                    CaptureEvent::Warning(m) => notice(&app, "warning", m),
                    CaptureEvent::Error(m) => log::warn!("capture: {m}"),
                    CaptureEvent::Stalled { mic, sys } => reopen_streams(&app, mic, sys),
                }
            }
        })
    };

    *state.recording.lock() = Some(ActiveRecording {
        meeting: meeting.clone(),
        recorder: Some(recorder),
        video,
        pcm_tx,
        live: Some(live),
        forwarder: Some(forwarder),
        forwarder_stop,
        highlights: Vec::new(),
        notes_stop,
    });
    state.refresh_priority();
    state.detection_paused.store(true, Ordering::SeqCst);
    spawn_participant_sampler(app.clone(), meeting.id.clone(), opts.app.clone());
    log::info!("recorder: showing record bar");
    windows::hide_popup(app);
    windows::show_recordbar(app);
    emit_state(app);
    log::info!("recorder: started {}", meeting.id);
    Ok(meeting)
}

/// While this meeting records, read attendee names from the meeting window's
/// accessibility tree every 45 s and merge them into the meeting.
/// Manual start: ask which window/app to record before anything is captured.
/// The popup is the same one detection uses, with app "manual" and no countdown.
pub fn open_picker(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.recording.lock().is_some() {
        return;
    }
    let _ = app.emit(
        events::DETECTION,
        events::DetectionPayload {
            app: "manual".into(),
            label: "Ready to record".into(),
            title: "Pick the window or app to record.".into(),
            confidence: 1.0,
            event: crate::calendar::current_event(&state).map(|e| e.title),
        },
    );
    windows::show_popup_near(app, 0);
}

/// A stream died (device switched, unplugged, or invalidated by Windows):
/// rebuild it on the current devices so the recording carries on.
fn reopen_streams(app: &AppHandle, mic: bool, sys: bool) {
    let state = app.state::<AppState>();
    let mut guard = state.recording.lock();
    let Some(active) = guard.as_mut() else { return };
    let Some(rec) = active.recorder.as_mut() else { return };
    log::warn!("capture stalled (mic={mic}, sys={sys}); reopening audio devices");
    match rec.reopen() {
        Ok(()) => notice(app, "warning", "Audio device changed - Debrief switched to the current one and is still recording.".into()),
        Err(e) => {
            log::error!("reopen audio: {e}");
            notice(app, "error", format!("Audio device lost ({e}). Debrief keeps trying; stop the recording to keep what was captured."));
        }
    }
}

/// Meetings left in `recording` by a crash or a hung stop: finish them from
/// the files on disk so nothing that was captured is lost.
pub fn recover_orphans(app: &AppHandle) {
    let state = app.state::<AppState>();
    let Ok(meetings) = state.store.list_meetings() else { return };
    for mut m in meetings.into_iter().filter(|m| matches!(m.status, MeetingStatus::Recording | MeetingStatus::Processing)) {
        let dir = state.store.recordings_dir(&m.id);
        let mix = dir.join("mix.wav");
        if m.status == MeetingStatus::Processing {
            // Interrupted mid-pipeline (crash or kill): just run it again.
            if !mix.exists() {
                m.status = MeetingStatus::Failed;
                m.error = Some("Processing was interrupted and the audio file is missing.".into());
                let _ = state.store.update_meeting(&m);
                continue;
            }
            log::info!("resuming interrupted processing of {}", m.id);
            notice(app, "info", format!("Finishing \"{}\" from the last session.", m.title));
            let app2 = app.clone();
            std::thread::spawn(move || crate::pipeline::post_process(&app2, m));
            continue;
        }
        let duration = hound::WavReader::open(&mix)
            .ok()
            .map(|r| r.len() as i64 * 1000 / (r.spec().sample_rate.max(1) as i64 * r.spec().channels.max(1) as i64))
            .unwrap_or(0);
        if duration < 1000 {
            log::warn!("orphaned recording {} has no usable audio; marking failed", m.id);
            m.status = MeetingStatus::Failed;
            m.error = Some("Recording was interrupted before any audio was saved.".into());
            let _ = state.store.update_meeting(&m);
            continue;
        }
        let ended = std::fs::metadata(&mix).and_then(|md| md.modified()).map(chrono::DateTime::<chrono::Utc>::from).unwrap_or_else(|_| chrono::Utc::now());
        log::info!("recovering orphaned recording {} ({} ms)", m.id, duration);
        m.duration_ms = duration;
        m.ended_at = Some(ended);
        m.status = MeetingStatus::Processing;
        m.has_video = m.has_video || dir.join("screen.raw.mp4").exists() || dir.join("screen.mp4").exists();
        let _ = state.store.update_meeting(&m);
        notice(app, "info", format!("Finishing \"{}\" from the last session.", m.title));
        let app2 = app.clone();
        std::thread::spawn(move || crate::pipeline::post_process(&app2, m));
    }
}

fn spawn_participant_sampler(app: AppHandle, meeting_id: String, app_id: Option<String>) {
    let Some(app_id) = app_id.filter(|a| a != "unknown") else { return };
    std::thread::spawn(move || {
        let patterns = hark_detect::PatternSet::builtin();
        let mut first = true;
        loop {
            if !first {
                std::thread::sleep(std::time::Duration::from_secs(45));
            }
            first = false;
            let state = app.state::<AppState>();
            let still = state.recording.lock().as_ref().map(|a| a.meeting.id == meeting_id).unwrap_or(false);
            if !still {
                break;
            }
            let windows = hark_detect::list_visible_windows();
            let Some(det) = patterns.match_windows(&windows).filter(|d| d.app == app_id && d.hwnd != 0) else { continue };
            let raw = hark_detect::scrape_window(det.hwnd);
            merge_participants(&app, &meeting_id, hark_detect::extract_names(&app_id, &raw));
        }
    });
}

/// Add newly seen names to the recording's participants (and tell the UI).
fn merge_participants(app: &AppHandle, meeting_id: &str, names: Vec<String>) {
    let state = app.state::<AppState>();
    let mut merged = state.recording.lock().as_ref().filter(|a| a.meeting.id == meeting_id).map(|a| a.meeting.participants.clone()).unwrap_or_default();
    let before = merged.len();
    for n in names {
        if !merged.iter().any(|m| m.eq_ignore_ascii_case(&n)) {
            merged.push(n);
        }
    }
    if merged.len() == before {
        return;
    }
    if let Some(a) = state.recording.lock().as_mut().filter(|a| a.meeting.id == meeting_id) {
        a.meeting.participants = merged.clone();
    }
    let _ = state.store.set_participants(meeting_id, &merged);
    let _ = app.emit(events::PARTICIPANTS, events::ParticipantsPayload { meeting_id: meeting_id.to_string(), participants: merged });
}

/// Read names off the screen being recorded (video-tile labels) with Windows OCR. A name
/// counts once it shows up in two different frames, so passing text on a shared screen
/// rarely gets in.
fn spawn_screen_name_reader(app: AppHandle, meeting_id: String, ffmpeg: std::path::PathBuf, target: VideoTarget, dir: std::path::PathBuf) {
    if !cfg!(windows) {
        return;
    }
    std::thread::spawn(move || {
        let shot = dir.join("names.png");
        let mut seen: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        let mut wait = 20;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(wait));
            wait = 45;
            let state = app.state::<AppState>();
            let still = state.recording.lock().as_ref().map(|a| a.meeting.id == meeting_id).unwrap_or(false);
            if !still {
                break;
            }
            if let Err(e) = hark_capture::video::snapshot(&ffmpeg, &target, &shot) {
                log::debug!("name snapshot: {e}");
                continue;
            }
            let names = hark_detect::ocr::names_in_image(&shot);
            let _ = std::fs::remove_file(&shot);
            let me = state.settings.read().user_name.trim().to_lowercase();
            let mut confirmed = Vec::new();
            for n in names {
                let count = seen.entry(n.to_lowercase()).or_insert(0);
                *count += 1;
                if *count == 2 && n.to_lowercase() != me {
                    confirmed.push(n);
                }
            }
            if !confirmed.is_empty() {
                log::info!("names on screen: {confirmed:?}");
                merge_participants(&app, &meeting_id, confirmed);
            }
        }
    });
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
    state.refresh_priority();
    state.detection_paused.store(false, Ordering::SeqCst);
    active.notes_stop.store(true, Ordering::Relaxed);
    windows::hide_recordbar(app);

    let out = active.recorder.take().ok_or("recorder missing")?.stop().map_err(|e| e.to_string())?;
    if let Some(v) = active.video.take() {
        let _ = v.stop();
    }
    // Stop forwarding, close the live feed (its only sender) and wait for the last window.
    active.forwarder_stop.store(true, Ordering::Relaxed);
    if let Some(f) = active.forwarder.take() {
        let _ = f.join();
    }
    drop(active.pcm_tx.lock().take());
    if let Some(Ok(Some(l))) = active.live.take().map(|h| h.join()) {
        l.join();
    }

    let mut meeting = active.meeting.clone();
    meeting.ended_at = Some(chrono::Utc::now());
    meeting.duration_ms = out.duration.as_millis() as i64;
    meeting.status = MeetingStatus::Processing;
    state.store.update_meeting(&meeting).map_err(|e| e.to_string())?;
    if !active.highlights.is_empty() {
        let _ = state.store.set_setting(&format!("highlights:{}", meeting.id), &active.highlights);
    }
    // The live captions become the transcript right away; post-processing replaces them
    // with the higher-quality, speaker-labelled version when it finishes.
    let finals: Vec<hark_store::NewSegment> = state
        .live_feed
        .lock()
        .as_ref()
        .map(|f| f.finals.iter().map(|c| hark_store::NewSegment { start_ms: c.start_ms, end_ms: c.end_ms, speaker: None, text: c.text.clone() }).collect())
        .unwrap_or_default();
    if !finals.is_empty() {
        if let Err(e) = state.store.replace_segments(&meeting.id, &finals) {
            log::warn!("provisional transcript: {e}");
        }
    }
    crate::live_feed::LiveFeed::end(&state);
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
