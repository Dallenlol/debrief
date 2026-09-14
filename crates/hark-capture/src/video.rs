//! Screen video via an ffmpeg sidecar. Argument builders are pure and tested;
//! `VideoRecorder` just runs the process and stops it cleanly.

use crate::monitors::MonitorInfo;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum VideoTarget {
    /// Whole monitor by index (0 = primary).
    Monitor { index: u32 },
    /// A single window by exact title (Windows only; macOS falls back to monitor 0).
    Window { title: String },
}

impl Default for VideoTarget {
    fn default() -> Self {
        VideoTarget::Monitor { index: 0 }
    }
}

fn common_encode_args(out: &Path, max_height: u32) -> Vec<String> {
    vec![
        "-vf".into(),
        format!("scale=-2:'min({max_height},ih)'"),
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "veryfast".into(),
        "-crf".into(),
        "28".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-an".into(),
        "-movflags".into(),
        "+frag_keyframe+empty_moov+default_base_moof".into(),
        "-f".into(),
        "mp4".into(),
        "-y".into(),
        out.to_string_lossy().into_owned(),
    ]
}

/// ffmpeg arguments to capture `target` to a fragmented MP4 at `fps`.
/// `monitor` is the resolved display for `VideoTarget::Monitor` (geometry on
/// Windows, avfoundation device on macOS); `None` captures the whole desktop.
pub fn ffmpeg_capture_args(target: &VideoTarget, monitor: Option<&MonitorInfo>, out: &Path, fps: u32, max_height: u32) -> Vec<String> {
    let mut args: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into()];
    if cfg!(windows) {
        args.extend(["-f".into(), "gdigrab".into(), "-framerate".into(), fps.to_string(), "-draw_mouse".into(), "1".into()]);
        match target {
            VideoTarget::Monitor { .. } => {
                if let Some(m) = monitor.filter(|m| m.width > 0 && m.height > 0) {
                    args.extend([
                        "-offset_x".into(),
                        m.x.to_string(),
                        "-offset_y".into(),
                        m.y.to_string(),
                        "-video_size".into(),
                        format!("{}x{}", m.width, m.height),
                    ]);
                }
                args.extend(["-i".into(), "desktop".into()]);
            }
            VideoTarget::Window { title } => args.extend(["-i".into(), format!("title={title}")]),
        }
    } else if cfg!(target_os = "macos") {
        let index = match (target, monitor) {
            (VideoTarget::Monitor { .. }, Some(m)) => m.device,
            (VideoTarget::Monitor { index }, None) => *index,
            (VideoTarget::Window { .. }, Some(m)) => m.device,
            (VideoTarget::Window { .. }, None) => 0,
        };
        args.extend([
            "-f".into(),
            "avfoundation".into(),
            "-framerate".into(),
            fps.to_string(),
            "-capture_cursor".into(),
            "1".into(),
            "-i".into(),
            format!("{index}:none"),
        ]);
    } else {
        args.extend(["-f".into(), "x11grab".into(), "-framerate".into(), fps.to_string(), "-i".into(), ":0.0".into()]);
    }
    args.extend(common_encode_args(out, max_height));
    args
}

/// Mux the finished video with the mixed audio WAV into a normal MP4.
pub fn ffmpeg_mux_args(video: &Path, audio_wav: &Path, out: &Path) -> Vec<String> {
    vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-i".into(),
        video.to_string_lossy().into_owned(),
        "-i".into(),
        audio_wav.to_string_lossy().into_owned(),
        "-c:v".into(),
        "copy".into(),
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "96k".into(),
        "-shortest".into(),
        "-movflags".into(),
        "+faststart".into(),
        "-y".into(),
        out.to_string_lossy().into_owned(),
    ]
}

/// Cut `[start, end)` seconds out of `input` into `out` (mp4 or mp3 by extension).
pub fn ffmpeg_clip_args(input: &Path, start_s: f64, end_s: f64, out: &Path) -> Vec<String> {
    let is_audio = out.extension().map(|e| e.eq_ignore_ascii_case("mp3")).unwrap_or(false);
    let mut args = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-ss".into(),
        format!("{start_s:.3}"),
        "-to".into(),
        format!("{end_s:.3}"),
        "-i".into(),
        input.to_string_lossy().into_owned(),
    ];
    if is_audio {
        args.extend(["-vn".into(), "-c:a".into(), "libmp3lame".into(), "-q:a".into(), "4".into()]);
    } else {
        args.extend([
            "-c:v".into(),
            "libx264".into(),
            "-preset".into(),
            "veryfast".into(),
            "-crf".into(),
            "23".into(),
            "-c:a".into(),
            "aac".into(),
        ]);
    }
    args.extend(["-y".into(), out.to_string_lossy().into_owned()]);
    args
}

pub struct VideoRecorder {
    child: Child,
    pub output: PathBuf,
}

impl VideoRecorder {
    pub fn start(ffmpeg: &Path, target: &VideoTarget, out: &Path, fps: u32, max_height: u32) -> std::io::Result<VideoRecorder> {
        let monitor = match target {
            VideoTarget::Monitor { index } => crate::monitors::list_monitors(Some(ffmpeg)).into_iter().find(|m| m.index == *index),
            VideoTarget::Window { .. } => None,
        };
        let child = Command::new(ffmpeg)
            .args(ffmpeg_capture_args(target, monitor.as_ref(), out, fps, max_height))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
        Ok(VideoRecorder { child, output: out.to_path_buf() })
    }

    /// Ask ffmpeg to finish (writes the trailer), wait up to 10 s, else kill.
    pub fn stop(mut self) -> std::io::Result<()> {
        if let Some(mut stdin) = self.child.stdin.take() {
            let _ = stdin.write_all(b"q\n");
            let _ = stdin.flush();
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if self.child.try_wait()?.is_some() {
                return Ok(());
            }
            if Instant::now() > deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// Run ffmpeg to completion with `args`; returns stderr on failure.
pub fn run_ffmpeg(ffmpeg: &Path, args: &[String]) -> Result<(), String> {
    let out = Command::new(ffmpeg).args(args).stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_args_use_platform_grabber_and_fragmented_mp4() {
        let out = Path::new("out.mp4");
        let a = ffmpeg_capture_args(&VideoTarget::Monitor { index: 0 }, None, out, 15, 1080);
        let joined = a.join(" ");
        if cfg!(windows) {
            assert!(joined.contains("-f gdigrab"));
            assert!(joined.contains("-i desktop"));
        } else if cfg!(target_os = "macos") {
            assert!(joined.contains("-f avfoundation"));
            assert!(joined.contains("-i 0:none"));
        }
        assert!(joined.contains("-framerate 15"));
        assert!(joined.contains("frag_keyframe"));
        assert!(joined.contains("min(1080,ih)"));
        assert_eq!(a.last().unwrap(), "out.mp4");
    }

    #[test]
    fn window_target_on_windows_uses_title() {
        let a = ffmpeg_capture_args(&VideoTarget::Window { title: "Zoom Meeting".into() }, None, Path::new("o.mp4"), 10, 720);
        if cfg!(windows) {
            assert!(a.contains(&"title=Zoom Meeting".to_string()));
        }
    }

    #[test]
    fn monitor_geometry_selects_a_region() {
        let m = MonitorInfo { index: 1, name: "DISPLAY2".into(), x: -1920, y: 0, width: 1920, height: 1080, primary: false, device: 2 };
        let a = ffmpeg_capture_args(&VideoTarget::Monitor { index: 1 }, Some(&m), Path::new("o.mp4"), 10, 720).join(" ");
        if cfg!(windows) {
            assert!(a.contains("-offset_x -1920 -offset_y 0 -video_size 1920x1080 -i desktop"));
        } else if cfg!(target_os = "macos") {
            assert!(a.contains("-i 2:none"));
        }
    }

    #[test]
    fn mux_and_clip_args() {
        let m = ffmpeg_mux_args(Path::new("v.mp4"), Path::new("mix.wav"), Path::new("screen.mp4"));
        assert!(m.join(" ").contains("-c:v copy -c:a aac"));
        assert_eq!(m.last().unwrap(), "screen.mp4");
        let c = ffmpeg_clip_args(Path::new("screen.mp4"), 1.5, 4.0, Path::new("clip.mp3"));
        assert!(c.join(" ").contains("-ss 1.500 -to 4.000"));
        assert!(c.contains(&"-vn".to_string()));
        let c2 = ffmpeg_clip_args(Path::new("screen.mp4"), 0.0, 2.0, Path::new("clip.mp4"));
        assert!(c2.contains(&"libx264".to_string()));
    }
}
