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

pub fn post_process(app: &AppHandle, mut meeting: Meeting) {
    let state = app.state::<AppState>();
    let dir = state.store.recordings_dir(&meeting.id);
    let settings = state.settings.read().clone();
    let meeting_id = meeting.id.clone();
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
    let (live_id, quality_id, _) = settings.model_ids(&state.catalog, tier);
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
    let mut labels: Vec<Option<String>> = vec![None; captions.len()];
    let mut speaker_rows: Vec<MeetingSpeaker> = Vec::new();
    let mut embeddings: BTreeMap<String, Vec<f32>> = BTreeMap::new();
    if settings.diarize_enabled && !captions.is_empty() {
        match run_diarize_sidecar(&state, &mix) {
            Ok(out) if !out.turns.is_empty() => {
                let turns = out.turns;
                let spans: Vec<(i64, i64)> = captions.iter().map(|c| (c.start_ms, c.end_ms)).collect();
                let clusters = assign_clusters(&spans, &turns);
                let me = me_cluster(&dir, &turns);
                let names = cluster_names(&clusters, me, &settings.user_name);
                for (i, c) in clusters.iter().enumerate() {
                    labels[i] = c.and_then(|c| names.get(&c).cloned());
                }
                let known: Vec<KnownSpeaker> = state
                    .store
                    .list_speakers()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|s| !s.embedding.is_empty())
                    .map(|s| KnownSpeaker { id: s.id, name: s.name, embedding: s.embedding })
                    .collect();
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
                    speaker_rows.push(MeetingSpeaker {
                        meeting_id: meeting.id.clone(),
                        label: label.clone(),
                        speaker_id: None,
                        suggested_id,
                        suggested_name: None,
                        suggested_score,
                    });
                }
            }
            Ok(_) => log::info!("diarization found no turns"),
            Err(e) => {
                log::warn!("diarization skipped: {e}");
                if !e.contains("not downloaded") {
                    notice(app, "warning", format!("Speaker detection failed: {e}"));
                }
            }
        }
    }

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

    // 4. LLM cleanup.
    if settings.cleanup_enabled && !segs.is_empty() {
        run_cleanup(app, &meeting.id);
    }
    emit("done", 1.0, None);
}

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
        }
    }
}

fn fail(state: &AppState, meeting: &mut Meeting, emit: &dyn Fn(&'static str, f32, Option<String>), err: String) {
    log::error!("post-process {}: {err}", meeting.id);
    meeting.status = MeetingStatus::Failed;
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
