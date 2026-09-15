//! Import an existing audio/video file as a meeting: ffmpeg converts it to
//! `mix.wav`, then the normal post-call pipeline runs.

use crate::pipeline;
use crate::state::AppState;
use hark_capture::video::run_ffmpeg;
use hark_store::{Meeting, MeetingStatus};
use std::path::Path;
use std::process::{Command, Stdio};
use tauri::{AppHandle, Manager};

/// Title for an imported file: the file stem with separators turned into spaces.
pub fn title_from_path(path: &Path) -> String {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let t: String = stem.chars().map(|c| if c == '_' || c == '-' || c == '.' { ' ' } else { c }).collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() { "Imported recording".into() } else { t }
}

/// Arguments that turn any ffmpeg-readable input into 16 kHz mono PCM.
pub fn ffmpeg_audio_args(input: &Path, out: &Path) -> Vec<String> {
    vec![
        "-hide_banner".into(), "-loglevel".into(), "error".into(), "-y".into(),
        "-i".into(), input.to_string_lossy().into_owned(),
        "-vn".into(), "-ac".into(), "1".into(), "-ar".into(), "16000".into(), "-c:a".into(), "pcm_s16le".into(),
        out.to_string_lossy().into_owned(),
    ]
}

/// Copy the video stream (no transcode) into an mp4 the player can show.
pub fn ffmpeg_video_args(input: &Path, out: &Path) -> Vec<String> {
    vec![
        "-hide_banner".into(), "-loglevel".into(), "error".into(), "-y".into(),
        "-i".into(), input.to_string_lossy().into_owned(),
        "-map".into(), "0:v:0".into(), "-map".into(), "0:a:0?".into(),
        "-c:v".into(), "copy".into(), "-c:a".into(), "aac".into(), "-movflags".into(), "+faststart".into(),
        out.to_string_lossy().into_owned(),
    ]
}

fn has_video_stream(ffmpeg: &Path, input: &Path) -> bool {
    // `ffmpeg -i` with no output exits 1 but prints the stream list to stderr.
    let Ok(out) = Command::new(ffmpeg).args(["-hide_banner", "-i"]).arg(input).stdin(Stdio::null()).output() else { return false };
    let err = String::from_utf8_lossy(&out.stderr);
    err.lines().any(|l| l.contains("Video:") && !l.contains("attached pic"))
}

fn wav_duration_ms(path: &Path) -> Option<i64> {
    let r = hound::WavReader::open(path).ok()?;
    let spec = r.spec();
    let frames = r.len() as i64 / spec.channels.max(1) as i64;
    Some(frames * 1000 / spec.sample_rate.max(1) as i64)
}

/// Create the meeting, convert the file, and kick off processing.
pub fn import(app: &AppHandle, path: &Path, title: Option<&str>) -> Result<Meeting, String> {
    let state = app.state::<AppState>();
    if !path.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    let ffmpeg = state.ffmpeg.clone().ok_or("ffmpeg is missing from this install, so files cannot be imported")?;
    let title = title.map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).unwrap_or_else(|| title_from_path(path));
    let mut meeting = Meeting::new_recording(&title, None);
    meeting.status = MeetingStatus::Processing;
    meeting.title_auto = false; // the file name is the user's title until they change it
    if let Ok(md) = std::fs::metadata(path) {
        if let Ok(modified) = md.modified() {
            meeting.started_at = modified.into();
        }
    }
    let dir = state.store.recordings_dir(&meeting.id);
    let mix = dir.join("mix.wav");
    if let Err(e) = run_ffmpeg(&ffmpeg, &ffmpeg_audio_args(path, &mix)) {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(format!("could not read {}: {}", path.display(), if e.is_empty() { "unsupported format".to_string() } else { e }));
    }
    let duration = wav_duration_ms(&mix).unwrap_or(0);
    if duration < 500 {
        let _ = std::fs::remove_dir_all(&dir);
        return Err("the file has no audio".into());
    }
    meeting.duration_ms = duration;
    meeting.ended_at = Some(meeting.started_at + chrono::Duration::milliseconds(duration));
    if has_video_stream(&ffmpeg, path) {
        // fable: stream copy only - a codec the webview cannot play just means audio-only playback.
        match run_ffmpeg(&ffmpeg, &ffmpeg_video_args(path, &dir.join("screen.mp4"))) {
            Ok(()) => meeting.has_video = true,
            Err(e) => {
                log::warn!("import: video copy failed, audio only: {e}");
                let _ = std::fs::remove_file(dir.join("screen.mp4"));
            }
        }
    }
    state.store.create_meeting(&meeting).map_err(|e| e.to_string())?;
    log::info!("import: {} -> meeting {} ({} ms, video={})", path.display(), meeting.id, duration, meeting.has_video);
    let app2 = app.clone();
    let m2 = meeting.clone();
    std::thread::spawn(move || pipeline::post_process(&app2, m2));
    Ok(meeting)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_come_from_the_file_name() {
        assert_eq!(title_from_path(Path::new("C:/x/2026-09-15_team-sync.mp3")), "2026 09 15 team sync");
        assert_eq!(title_from_path(Path::new("Voice 010.m4a")), "Voice 010");
        assert_eq!(title_from_path(Path::new("")), "Imported recording");
    }

    #[test]
    fn audio_args_force_16k_mono_pcm() {
        let a = ffmpeg_audio_args(Path::new("in.mp4"), Path::new("out.wav"));
        assert!(a.windows(2).any(|w| w == ["-ar", "16000"]));
        assert!(a.windows(2).any(|w| w == ["-ac", "1"]));
        assert!(a.contains(&"-vn".to_string()));
        assert_eq!(a.last().unwrap(), "out.wav");
    }
}
