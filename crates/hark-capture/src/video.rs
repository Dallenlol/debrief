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
    /// A single window (Windows only; macOS falls back to monitor 0). `hwnd` is the window
    /// handle when known (this session only); the title is the fallback match.
    Window {
        title: String,
        #[serde(default)]
        hwnd: u64,
    },
}

impl Default for VideoTarget {
    fn default() -> Self {
        VideoTarget::Monitor { index: 0 }
    }
}

/// A process that never opens a console window. Debrief is a GUI app, and on Windows every
/// console program it starts (ffmpeg, the diarizer) would otherwise flash a terminal.
pub fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Scale to at most `max_height`, keeping both sides even (libx264 + yuv420p refuse odd sizes,
/// and a window can be any size).
fn scale_filter(max_height: u32) -> String {
    format!("scale=-2:'trunc(min({max_height},ih)/2)*2'")
}

/// Escape a window title for gfxcapture's ECMAScript regex, anchored to match it exactly.
fn title_regex(title: &str) -> String {
    let mut out = String::from("^");
    for c in title.chars() {
        if "\\^$.|?*+()[]{}".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('$');
    out
}

/// Value for a filter option: wrapped in quotes so ffmpeg's filter parser leaves `:` `,` `=` alone.
fn filter_quote(v: &str) -> String {
    format!("'{}'", v.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn common_encode_args(out: &Path, max_height: u32) -> Vec<String> {
    let mut v = vec!["-vf".into(), scale_filter(max_height)];
    v.extend(encode_tail(out));
    v
}

fn encode_tail(out: &Path) -> Vec<String> {
    vec![
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
    if let (true, VideoTarget::Window { title, hwnd }) = (cfg!(windows), target) {
        // Windows.Graphics.Capture: GDI's window grab returns black frames for anything the GPU
        // draws (browsers, Zoom, Teams, Electron), which is nearly every meeting window.
        let pick = if *hwnd != 0 { format!("hwnd={hwnd}") } else { format!("window_title={}", filter_quote(&title_regex(title))) };
        args.extend([
            "-filter_complex".into(),
            format!(
                "gfxcapture={pick}:max_framerate={fps}:capture_cursor=1:resize_mode=scale_aspect,hwdownload,format=bgra,fps={fps},{},format=yuv420p",
                scale_filter(max_height)
            ),
        ]);
        args.extend(encode_tail(out));
        return args;
    }
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
            VideoTarget::Window { .. } => unreachable!("handled below"),
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

/// One still frame of `target` as a PNG, its long side scaled to about 3200 px: Windows OCR
/// misses small labels (video-tile names) at screen size and reads them reliably at 2x.
pub fn ffmpeg_snapshot_args(target: &VideoTarget, monitor: Option<&MonitorInfo>, out: &Path) -> Vec<String> {
    const SCALE: &str = "scale='if(gt(iw,ih),3200,-2)':'if(gt(iw,ih),-2,3200)':flags=lanczos";
    let mut args: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into()];
    match target {
        VideoTarget::Window { title, hwnd } if cfg!(windows) => {
            let pick = if *hwnd != 0 { format!("hwnd={hwnd}") } else { format!("window_title={}", filter_quote(&title_regex(title))) };
            args.extend(["-filter_complex".into(), format!("gfxcapture={pick}:capture_cursor=0,hwdownload,format=bgra,{SCALE}")]);
        }
        _ => {
            // Same source as the recording, then swap the encoder tail for one PNG.
            let mut a = ffmpeg_capture_args(target, monitor, out, 1, 4320);
            let cut = a.iter().position(|x| x == "-vf").unwrap_or(a.len());
            a.truncate(cut);
            args = a;
            args.extend(["-vf".into(), SCALE.into()]);
        }
    }
    args.extend(["-frames:v".into(), "1".into(), "-y".into(), out.to_string_lossy().into_owned()]);
    args
}

/// Grab one frame of `target` into `out` (PNG).
pub fn snapshot(ffmpeg: &Path, target: &VideoTarget, out: &Path) -> Result<(), String> {
    let monitor = match target {
        VideoTarget::Monitor { index } => crate::monitors::list_monitors(Some(ffmpeg)).into_iter().find(|m| m.index == *index),
        VideoTarget::Window { .. } => None,
    };
    run_ffmpeg(ffmpeg, &ffmpeg_snapshot_args(target, monitor.as_ref(), out))
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
    /// `None` once stopped, so the fallback watcher never restarts a finished recording.
    child: std::sync::Arc<std::sync::Mutex<Option<Child>>>,
    pub output: PathBuf,
}

fn spawn_capture(ffmpeg: &Path, target: &VideoTarget, out: &Path, fps: u32, max_height: u32) -> std::io::Result<Child> {
    let monitor = match target {
        VideoTarget::Monitor { index } => crate::monitors::list_monitors(Some(ffmpeg)).into_iter().find(|m| m.index == *index),
        VideoTarget::Window { .. } => None,
    };
    let child = hidden_command(ffmpeg)
        .args(ffmpeg_capture_args(target, monitor.as_ref(), out, fps, max_height))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        // Never read: a piped stderr that fills up would stall the recorder.
        .stderr(Stdio::null())
        .spawn()?;
    tie_to_process(&child);
    Ok(child)
}

impl VideoRecorder {
    pub fn start(ffmpeg: &Path, target: &VideoTarget, out: &Path, fps: u32, max_height: u32) -> std::io::Result<VideoRecorder> {
        // A handle from an earlier session points at nothing (or at another window): match by title.
        let target = match target {
            VideoTarget::Window { title, hwnd } if *hwnd != 0 && !window_alive(*hwnd) => VideoTarget::Window { title: title.clone(), hwnd: 0 },
            t => t.clone(),
        };
        let child = std::sync::Arc::new(std::sync::Mutex::new(Some(spawn_capture(ffmpeg, &target, out, fps, max_height)?)));
        if matches!(target, VideoTarget::Window { .. }) {
            // Some windows cannot be captured (minimised, suspended store apps): ffmpeg exits at once.
            // Then record the main screen instead, so the call still has a picture.
            // fable: the fallback video starts ~1.5 s after the audio; mux does not offset for it.
            let (child, ffmpeg, out) = (child.clone(), ffmpeg.to_path_buf(), out.to_path_buf());
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(1500));
                let mut slot = child.lock().unwrap_or_else(|e| e.into_inner());
                let Some(c) = slot.as_mut() else { return };
                if let Ok(Some(status)) = c.try_wait() {
                    tracing::warn!("window capture stopped at once ({status}); recording the main screen instead");
                    match spawn_capture(&ffmpeg, &VideoTarget::Monitor { index: 0 }, &out, fps, max_height) {
                        Ok(n) => *slot = Some(n),
                        Err(e) => tracing::warn!("screen capture fallback failed: {e}"),
                    }
                }
            });
        }
        Ok(VideoRecorder { child, output: out.to_path_buf() })
    }

    /// Ask ffmpeg to finish (writes the trailer), wait up to 10 s, else kill.
    pub fn stop(self) -> std::io::Result<()> {
        let Some(mut child) = self.child.lock().unwrap_or_else(|e| e.into_inner()).take() else { return Ok(()) };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(b"q\n");
            let _ = stdin.flush();
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if child.try_wait()?.is_some() {
                return Ok(());
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// Make the OS kill ffmpeg if Hark dies, so a crash never leaves a screen
/// recorder running in the background. Windows job object; no-op elsewhere.
#[cfg(windows)]
fn tie_to_process(child: &Child) {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    unsafe {
        let Ok(job) = CreateJobObjectW(None, None) else { return };
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = SetInformationJobObject(job, JobObjectExtendedLimitInformation, &info as *const _ as *const _, std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32).is_ok();
        if ok {
            let _ = AssignProcessToJobObject(job, HANDLE(child.as_raw_handle() as *mut _));
        }
        // fable: the job handle is deliberately leaked; it lives as long as this process,
        // which is exactly the lifetime that should kill ffmpeg.
    }
}

#[cfg(not(windows))]
fn tie_to_process(_child: &Child) {}

#[cfg(windows)]
fn window_alive(hwnd: u64) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::IsWindow;
    unsafe { IsWindow(Some(HWND(hwnd as usize as *mut _))).as_bool() }
}

#[cfg(not(windows))]
fn window_alive(_hwnd: u64) -> bool {
    false
}

/// Run ffmpeg to completion with `args`; returns stderr on failure.
pub fn run_ffmpeg(ffmpeg: &Path, args: &[String]) -> Result<(), String> {
    let out = hidden_command(ffmpeg).args(args).stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        // A crash or kill leaves stderr empty; the exit code is then the only clue.
        Err(if err.is_empty() { format!("ffmpeg exited with {}", out.status) } else { err })
    }
}

/// Whether `input` carries an audio stream. `None` when ffmpeg cannot run or cannot
/// read the file, so callers never act on a guess.
pub fn has_audio_stream(ffmpeg: &Path, input: &Path) -> Option<bool> {
    // `ffmpeg -i` with no output exits 1 but prints the stream list to stderr.
    let out = hidden_command(ffmpeg).args(["-hide_banner", "-i"]).arg(input).stdin(Stdio::null()).output().ok()?;
    stream_list_has_audio(&String::from_utf8_lossy(&out.stderr))
}

fn stream_list_has_audio(stderr: &str) -> Option<bool> {
    let streams: Vec<&str> = stderr.lines().map(str::trim_start).filter(|l| l.starts_with("Stream #")).collect();
    (!streams.is_empty()).then(|| streams.iter().any(|l| l.contains("Audio:")))
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
    fn window_target_on_windows_uses_graphics_capture() {
        let a = ffmpeg_capture_args(&VideoTarget::Window { title: "Zoom Meeting".into(), hwnd: 0 }, None, Path::new("o.mp4"), 10, 720).join(" ");
        let b = ffmpeg_capture_args(&VideoTarget::Window { title: "x".into(), hwnd: 4242 }, None, Path::new("o.mp4"), 10, 720).join(" ");
        if cfg!(windows) {
            assert!(a.contains("gfxcapture=window_title='^Zoom Meeting$'") && a.contains("hwdownload,format=bgra"), "{a}");
            assert!(b.contains("gfxcapture=hwnd=4242:") && !b.contains("gdigrab"), "{b}");
        }
        assert_eq!(title_regex("(3) Chat | Teams"), r"^\(3\) Chat \| Teams$");
        assert_eq!(filter_quote("it's"), r"'it\'s'");
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

    #[test]
    fn audio_stream_detection() {
        let silent = "Input #0, mov,mp4, from 'screen.mp4':
  Stream #0:0[0x1](und): Video: h264 (High), yuv420p, 1920x1080, 15 fps
";
        let muxed = format!("{silent}  Stream #0:1[0x2](und): Audio: aac (LC), 48000 Hz, stereo, fltp, 96 kb/s
");
        assert_eq!(stream_list_has_audio(silent), Some(false));
        assert_eq!(stream_list_has_audio(&muxed), Some(true));
        assert_eq!(stream_list_has_audio("[in#0 @ 0x1] moov atom not found
screen.mp4: Invalid data found"), None);
    }
}
