//! Post-call pipeline: quality transcription -> speaker diarization -> LLM cleanup.
//! Each optional stage fails soft: the meeting still becomes Ready.

use crate::events::{self, ProcessingPayload};
use crate::recorder::{notice, read_wav_as_16k_mono};
use crate::state::AppState;
use hark_capture::dsp::rms_db;
use hark_capture::video::{ffmpeg_mux_args, run_ffmpeg};
use hark_diarize::{assign_clusters, best_match, find_me_cluster, DiarizeOutput, KnownSpeaker, Turn};
use hark_store::{Meeting, MeetingSpeaker, MeetingStatus, NewSegment};
use std::collections::BTreeMap;
use std::path::Path;
use tauri::{AppHandle, Emitter, Manager};

/// Cosine similarity needed to suggest a known speaker (TitaNet same-speaker ~0.6-0.85).
pub const SUGGEST_THRESHOLD: f32 = 0.62;
const LEVEL_HOP_MS: i64 = 250;

/// One-off choices for a (re-)run of the pipeline.
#[derive(Default, Clone)]
pub struct PostOpts {
    /// Quality ASR model id to use instead of the configured one.
    pub asr_model: Option<String>,
}

pub fn post_process(app: &AppHandle, meeting: Meeting) {
    post_process_with(app, meeting, PostOpts::default())
}

pub fn post_process_with(app: &AppHandle, mut meeting: Meeting, opts: PostOpts) {
    let state = app.state::<AppState>();
    let dir = state.store.recordings_dir(&meeting.id);
    let settings = state.settings.read().clone();
    let meeting_id = meeting.id.clone();
    meeting.error = None;
    let _ = state.store.set_meeting_error(&meeting_id, None);
    let emit = |stage: &'static str, progress: f32, error: Option<String>| {
        let _ = app.emit(events::PROCESSING, ProcessingPayload { meeting_id: meeting_id.clone(), stage, progress, error });
    };

    // 1. Mux video (if any).
    emit("mux", 0.02, None);
    let raw = dir.join("screen.raw.mp4");
    if meeting.has_video && raw.exists() {
        if let Some(ffmpeg) = &state.ffmpeg {
            let out = dir.join("screen.mp4");
            match run_ffmpeg(ffmpeg, &ffmpeg_mux_args(&raw, &dir.join("mix.wav"), &out)) {
                Ok(()) => {
                    let _ = std::fs::remove_file(&raw);
                }
                Err(e) => {
                    log::error!("mux failed: {e}");
                    let _ = std::fs::rename(&raw, &out);
                }
            }
        }
    }

    // 2. Quality transcription.
    emit("transcribe", 0.1, None);
    let tier = settings.tier(&state.hardware);
    let (live_id, mut quality_id, _) = settings.model_ids(&state.catalog, tier);
    if let Some(m) = opts.asr_model.clone() {
        quality_id = m;
    }
    let mix = dir.join("mix.wav");
    let pcm = match read_wav_as_16k_mono(&mix) {
        Ok(p) => p,
        Err(e) => {
            fail(&state, &mut meeting, &emit, format!("read audio: {e}"));
            return;
        }
    };
    let mut captions = match state.whisper_or_any(&[&quality_id, &live_id]) {
        Some(engine) => match engine.transcribe_with(&pcm, 0, settings.language.as_deref(), true, 80) {
            Ok(c) => c,
            Err(e) => {
                fail(&state, &mut meeting, &emit, format!("transcription: {e}"));
                return;
            }
        },
        None => {
            notice(app, "info", "Transcription skipped: no speech model downloaded. Download one in Settings, then Re-transcribe.".into());
            Vec::new()
        }
    };
    captions.retain(|c| !c.text.trim().is_empty());

    // 3. Diarization -> speaker labels.
    emit("diarize", 0.45, None);
    let spans: Vec<(i64, i64)> = captions.iter().map(|c| (c.start_ms, c.end_ms)).collect();
    let (labels, speaker_rows, embeddings) = if settings.diarize_enabled && !captions.is_empty() {
        match diarize_spans(app, &meeting.id, &dir, &spans) {
            Ok(d) => d,
            Err(e) => {
                log::warn!("diarization skipped: {e}");
                if !e.contains("not downloaded") {
                    notice(app, "warning", format!("Speaker detection failed: {e}"));
                    let _ = state.store.set_meeting_error(&meeting.id, Some(&format!("speakers: {e}")));
                }
                (vec![None; captions.len()], Vec::new(), BTreeMap::new())
            }
        }
    } else {
        (vec![None; captions.len()], Vec::new(), BTreeMap::new())
    };

    let segs: Vec<NewSegment> = captions
        .iter()
        .zip(labels)
        .map(|(c, sp)| NewSegment { start_ms: c.start_ms, end_ms: c.end_ms, speaker: sp, text: c.text.clone() })
        .collect();
    if let Err(e) = state.store.replace_segments(&meeting.id, &segs) {
        fail(&state, &mut meeting, &emit, format!("save transcript: {e}"));
        return;
    }
    let _ = state.store.set_meeting_speakers(&meeting.id, &speaker_rows);
    let _ = state.store.set_setting(&format!("speaker_embeddings:{}", meeting.id), &embeddings);

    // Ready now; cleanup improves it in place.
    meeting.status = MeetingStatus::Ready;
    let _ = state.store.update_meeting(&meeting);
    emit("cleanup", 0.75, None);

    // 4. LLM cleanup, retrieval chunks, summary.
    if settings.cleanup_enabled && !segs.is_empty() {
        run_cleanup(app, &meeting.id);
    }
    let _ = state.store.rebuild_chunks(&meeting.id);
    emit("embed", 0.85, None);
    if let Err(e) = crate::embed_stage::embed_meeting(app, &meeting.id) {
        log::warn!("embedding failed: {e}");
    }
    if settings.summary_enabled && !segs.is_empty() {
        run_summary(app, &meeting.id, None);
    }
    emit("done", 1.0, None);
}
pub use crate::summary_stage::run_summary;

/// Re-run only the cleanup pass on stored segments.
pub fn run_cleanup(app: &AppHandle, meeting_id: &str) {
    let state = app.state::<AppState>();
    let Some(backend) = state.llm() else {
        notice(app, "info", "Transcript cleanup skipped: no language model available. Download one in Settings or point Hark at an endpoint.".into());
        return;
    };
    let stored = match state.store.segments(meeting_id) {
        Ok(s) => s,
        Err(e) => {
            log::error!("cleanup load: {e}");
            return;
        }
    };
    let raw: Vec<hark_llm::cleanup::RawSegment> = stored.iter().map(|s| (s.id, s.speaker.clone(), s.text.clone())).collect();
    let prog_app = app.clone();
    let mid = meeting_id.to_string();
    let progress = move |p: f32| {
        let _ = prog_app.emit(
            events::PROCESSING,
            ProcessingPayload { meeting_id: mid.clone(), stage: "cleanup", progress: 0.75 + 0.25 * p, error: None },
        );
    };
    match hark_llm::cleanup::run(backend.as_ref(), &raw, progress, || false) {
        Ok(updates) => {
            let _ = state.store.set_clean_text(meeting_id, &updates);
            let _ = app.emit(
                events::PROCESSING,
                ProcessingPayload { meeting_id: meeting_id.to_string(), stage: "done", progress: 1.0, error: None },
            );
        }
        Err(e) => {
            log::error!("cleanup failed: {e}");
            notice(app, "warning", format!("Transcript cleanup failed: {e}"));
            let _ = state.store.set_meeting_error(meeting_id, Some(&format!("cleanup: {e}")));
        }
    }
}

type Diarized = (Vec<Option<String>>, Vec<MeetingSpeaker>, BTreeMap<String, Vec<f32>>);

/// Run the sidecar over `mix.wav` and map its turns onto `spans` (one per
/// transcript segment): per-segment labels, the meeting_speakers rows (with
/// voice-memory suggestions) and the per-label embeddings.
fn diarize_spans(app: &AppHandle, meeting_id: &str, dir: &Path, spans: &[(i64, i64)]) -> Result<Diarized, String> {
    let state = app.state::<AppState>();
    let settings = state.settings.read().clone();
    let out = run_diarize_sidecar(&state, &dir.join("mix.wav"))?;
    if out.turns.is_empty() {
        return Err("no speech turns found".into());
    }
    let turns = out.turns;
    let clusters = assign_clusters(spans, &turns);
    let me = me_cluster(dir, &turns);
    let names = cluster_names(&clusters, me, &settings.user_name);
    let labels: Vec<Option<String>> = clusters.iter().map(|c| c.and_then(|c| names.get(&c).cloned())).collect();
    let known: Vec<KnownSpeaker> = state
        .store
        .list_speakers()
        .unwrap_or_default()
        .into_iter()
        .filter(|s| !s.embedding.is_empty())
        .map(|s| KnownSpeaker { id: s.id, name: s.name, embedding: s.embedding })
        .collect();
    let mut rows = Vec::new();
    let mut embeddings = BTreeMap::new();
    for (cluster, label) in &names {
        let Some(emb) = out.embeddings.get(cluster) else { continue };
        let (suggested_id, suggested_score) = if Some(*cluster) == me {
            (None, None)
        } else {
            match best_match(emb, &known, SUGGEST_THRESHOLD) {
                Some((k, score)) => (Some(k.id.clone()), Some(score)),
                None => (None, None),
            }
        };
        embeddings.insert(label.clone(), emb.clone());
        rows.push(MeetingSpeaker {
            meeting_id: meeting_id.to_string(),
            label: label.clone(),
            speaker_id: None,
            suggested_id,
            suggested_name: None,
            suggested_score,
        });
    }
    Ok((labels, rows, embeddings))
}

/// Re-run only speaker detection over the stored transcript.
pub fn rediarize(app: &AppHandle, meeting_id: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let dir = state.store.recordings_dir(meeting_id);
    let emit = |stage: &'static str, progress: f32, error: Option<String>| {
        let _ = app.emit(events::PROCESSING, ProcessingPayload { meeting_id: meeting_id.to_string(), stage, progress, error });
    };
    emit("diarize", 0.45, None);
    let segs = state.store.segments(meeting_id).map_err(|e| e.to_string())?;
    let spans: Vec<(i64, i64)> = segs.iter().map(|s| (s.start_ms, s.end_ms)).collect();
    match diarize_spans(app, meeting_id, &dir, &spans) {
        Ok((labels, rows, embeddings)) => {
            let updates: Vec<(i64, Option<String>)> = segs.iter().zip(labels).map(|(s, l)| (s.id, l)).collect();
            state.store.set_segment_speakers(meeting_id, &updates).map_err(|e| e.to_string())?;
            let _ = state.store.set_meeting_speakers(meeting_id, &rows);
            let _ = state.store.set_setting(&format!("speaker_embeddings:{meeting_id}"), &embeddings);
            let _ = state.store.set_meeting_error(meeting_id, None);
            let _ = state.store.rebuild_chunks(meeting_id);
            let _ = crate::embed_stage::embed_meeting(app, meeting_id);
            emit("done", 1.0, None);
            Ok(())
        }
        Err(e) => {
            let _ = state.store.set_meeting_error(meeting_id, Some(&format!("speakers: {e}")));
            emit("done", 1.0, Some(e.clone()));
            Err(e)
        }
    }
}

/// Rebuild retrieval chunks and their embeddings.
pub fn reembed(app: &AppHandle, meeting_id: &str) -> Result<usize, String> {
    let state = app.state::<AppState>();
    let emit = |stage: &'static str, progress: f32, error: Option<String>| {
        let _ = app.emit(events::PROCESSING, ProcessingPayload { meeting_id: meeting_id.to_string(), stage, progress, error });
    };
    emit("embed", 0.5, None);
    state.store.rebuild_chunks(meeting_id).map_err(|e| e.to_string())?;
    let r = crate::embed_stage::embed_meeting(app, meeting_id);
    emit("done", 1.0, r.as_ref().err().cloned());
    r
}

fn fail(state: &AppState, meeting: &mut Meeting, emit: &dyn Fn(&'static str, f32, Option<String>), err: String) {
    log::error!("post-process {}: {err}", meeting.id);
    meeting.status = MeetingStatus::Failed;
    meeting.error = Some(err.clone());
    let _ = state.store.update_meeting(meeting);
    emit("failed", 1.0, Some(err));
}

/// Level series (250 ms hops) from the separate mic/system tracks, then the cluster that is "me".
fn me_cluster(dir: &Path, turns: &[Turn]) -> Option<i32> {
    let mic = level_series(&dir.join("mic.wav"));
    let sys = level_series(&dir.join("sys.wav"));
    if mic.is_empty() {
        return None;
    }
    find_me_cluster(turns, &mic, &sys, 6.0)
}

fn level_series(wav: &Path) -> Vec<(i64, f32)> {
    let Ok(pcm) = read_wav_as_16k_mono(wav) else { return Vec::new() };
    let hop = (16_000 * LEVEL_HOP_MS / 1000) as usize;
    pcm.chunks(hop).enumerate().map(|(i, c)| (i as i64 * LEVEL_HOP_MS, rms_db(c))).collect()
}

/// Cluster -> display label. "Me" cluster gets the user's name; others "Speaker 1..n" by first appearance.
fn cluster_names(clusters: &[Option<i32>], me: Option<i32>, user_name: &str) -> BTreeMap<i32, String> {
    let mut names = BTreeMap::new();
    let mut n = 0;
    for c in clusters.iter().flatten() {
        if names.contains_key(c) {
            continue;
        }
        if Some(*c) == me {
            names.insert(*c, if user_name.trim().is_empty() { "Me".to_string() } else { user_name.trim().to_string() });
        } else {
            n += 1;
            names.insert(*c, format!("Speaker {n}"));
        }
    }
    names
}

/// Run the `hark-diarize` sidecar on a WAV and parse its JSON.
fn run_diarize_sidecar(state: &AppState, wav: &Path) -> Result<DiarizeOutput, String> {
    let bin = state.diarize_bin.as_ref().ok_or("hark-diarize sidecar not found")?;
    let (seg, emb) = state.diarize_models().ok_or("speaker models not downloaded")?;
    let out = std::process::Command::new(bin)
        .arg("--seg")
        .arg(&seg)
        .arg("--emb")
        .arg(&emb)
        .arg("--wav")
        .arg(wav)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("spawn sidecar: {e}"))?;
    if !out.status.success() {
        return Err(format!("sidecar exited with {}: {}", out.status, String::from_utf8_lossy(&out.stderr).trim()));
    }
    let parsed: DiarizeOutput = serde_json::from_slice(&out.stdout).map_err(|e| format!("sidecar output: {e}"))?;
    if let Some(e) = parsed.error {
        return Err(e);
    }
    Ok(parsed)
}
