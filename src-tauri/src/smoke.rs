//! Headless smoke test: `HARK_SMOKE=1 hark` records for 8 s, waits for the
//! post-call pipeline, prints the transcript and exits 0 (or 1 on failure).

use crate::recorder::{self, StartOptions};
use crate::state::AppState;
use hark_store::MeetingStatus;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

pub fn run(app: AppHandle) {
    std::thread::spawn(move || {
        let code = match smoke(&app) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("SMOKE FAIL: {e}");
                1
            }
        };
        app.exit(code);
    });
}

fn smoke(app: &AppHandle) -> Result<(), String> {
    std::thread::sleep(Duration::from_secs(2));
    let video = std::env::var("HARK_SMOKE_VIDEO").map(|v| v == "1").unwrap_or(false);
    log::info!("SMOKE: calling recorder::start");
    let m = recorder::start(app, StartOptions { title: Some("Smoke test".into()), app: None, video: Some(video), target: None })?;
    println!("SMOKE recording {} (video={video})", m.id);
    std::thread::sleep(Duration::from_secs(8));
    let m = recorder::stop(app)?;
    println!("SMOKE stopped after {} ms", m.duration_ms);

    let state = app.state::<AppState>();
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let cur = state.store.get_meeting(&m.id).map_err(|e| e.to_string())?.ok_or("meeting vanished")?;
        match cur.status {
            MeetingStatus::Ready => {
                // Give the cleanup pass a chance to finish (it runs after Ready).
                let wait_until = Instant::now() + Duration::from_secs(90);
                while Instant::now() < wait_until {
                    let segs = state.store.segments(&m.id).map_err(|e| e.to_string())?;
                    if segs.is_empty() || segs.iter().any(|s| s.clean_text.is_some()) {
                        break;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
                let segs = state.store.segments(&m.id).map_err(|e| e.to_string())?;
                println!("SMOKE ready: {} segments, has_video={}", segs.len(), cur.has_video);
                for s in &segs {
                    println!("  [{}-{}] {}: {}", s.start_ms, s.end_ms, s.speaker.as_deref().unwrap_or("?"), s.text);
                    if let Some(c) = &s.clean_text {
                        println!("      clean: {c}");
                    }
                }
                for sp in state.store.meeting_speakers(&m.id).unwrap_or_default() {
                    println!("  speaker {:?} suggested={:?} score={:?}", sp.label, sp.suggested_name, sp.suggested_score);
                }
                // HARK_SMOKE_NAME=<name>: rename "Speaker 1" so the next run can suggest it.
                if let Ok(name) = std::env::var("HARK_SMOKE_NAME") {
                    let emb: Option<std::collections::BTreeMap<String, Vec<f32>>> =
                        state.store.get_setting(&format!("speaker_embeddings:{}", m.id)).ok().flatten();
                    let e = emb.and_then(|m| m.get("Speaker 1").cloned());
                    match state.store.rename_speaker(&m.id, "Speaker 1", &name, e.as_deref()) {
                        Ok(sp) => println!("SMOKE renamed Speaker 1 -> {} (embedding dims {})", sp.name, sp.embedding.len()),
                        Err(e) => println!("SMOKE rename failed: {e}"),
                    }
                }
                let dir = state.store.recordings_dir(&m.id);
                for f in ["mic.wav", "sys.wav", "mix.wav", "screen.mp4"] {
                    if let Ok(md) = std::fs::metadata(dir.join(f)) {
                        println!("  {f}: {} bytes", md.len());
                    }
                }
                return Ok(());
            }
            MeetingStatus::Failed => return Err("post-processing failed".into()),
            _ => {}
        }
        if Instant::now() > deadline {
            return Err("timed out waiting for processing".into());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}
