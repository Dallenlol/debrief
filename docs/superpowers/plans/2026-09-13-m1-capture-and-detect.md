# Hark M1 — Capture & Detect Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A running Tauri desktop app that detects meetings, shows a manual "Record?" popup, records mic + system audio (+ optional screen video) with live captions, and lists/plays recordings with a transcript.

**Architecture:** Cargo workspace of small Tauri-agnostic crates (`hark-store`, `hark-detect`, `hark-capture`, `hark-models`, `hark-asr`) wired by `src-tauri` into IPC commands/events; React/TS UI in `app/`. Pure logic (pattern matching, debounce, mixing, chunking, tier selection, ffmpeg args) is unit-tested; platform glue (cpal streams, window enumeration, whisper) is thin and behind small traits.

**Tech Stack:** Rust 1.95, Tauri 2.11.5, cpal 0.18, whisper-rs 0.16, rusqlite 0.40 (bundled, fts5), sysinfo 0.39, nvml-wrapper 0.13 (Windows), windows 0.62 (Windows), core-graphics 0.25 (macOS), React 19 + Vite 6 + TypeScript 5 + Tailwind 4 + shadcn/ui, pnpm.

## Global Constraints

- Nothing records without a user click / hotkey. Detection only emits events.
- No network calls except model downloads (HuggingFace) initiated from the UI.
- Data dir: `%APPDATA%\Hark` (Windows) / `~/Library/Application Support/Hark` (macOS); resolved once by `hark_store::data_dir()`.
- Recording files: `recordings/<meeting-id>/mic.wav`, `sys.wav`, `mix.wav` (M1; opus later), `screen.mp4`.
- ASR sample format: 16 kHz mono f32. Archive WAV: 48 kHz, 16-bit PCM.
- Each crate is independent of Tauri and has `cargo test` passing on Windows; macOS paths compile under `cfg(target_os = "macos")` and are reviewed, not executed, in M1.
- Commit after every task; conventional commit messages.

---

## Repository layout (created across tasks)

```
hark/
  Cargo.toml                 workspace: crates/*, src-tauri
  package.json               pnpm scripts: dev, build, tauri
  app/                       React UI (Vite root)
    index.html  src/main.tsx  src/App.tsx  src/lib/ipc.ts  src/routes/*  src/components/*
    src/windows/Popup.tsx  src/windows/RecordBar.tsx
  src-tauri/                 Tauri app crate `hark`
    tauri.conf.json  Cargo.toml  build.rs  capabilities/default.json
    src/main.rs  src/lib.rs  src/state.rs  src/commands/*.rs  src/events.rs
    src/tray.rs  src/windows.rs  src/detector_loop.rs  src/recorder.rs
  crates/
    hark-store/    src/lib.rs  src/db.rs  src/migrations.rs  src/meetings.rs  src/segments.rs  src/settings.rs
    hark-detect/   src/lib.rs  src/patterns.rs  src/debounce.rs  src/audio_activity.rs  src/windows_enum.rs  patterns.toml
    hark-capture/  src/lib.rs  src/devices.rs  src/stream.rs  src/wav.rs  src/dsp.rs  src/recorder.rs  src/video.rs
    hark-models/   src/lib.rs  src/hardware.rs  src/tiers.rs  src/catalog.rs  src/download.rs  catalog.json
    hark-asr/      src/lib.rs  src/chunker.rs  src/whisper.rs  src/live.rs
```

---

### Task 1: Workspace + Tauri + React scaffold

**Files:**
- Create: `Cargo.toml`, `package.json`, `pnpm-workspace.yaml` (none needed; single package), `.gitignore`, `rust-toolchain.toml`
- Create: `app/index.html`, `app/src/main.tsx`, `app/src/App.tsx`, `app/src/index.css`, `vite.config.ts`, `tsconfig.json`, `tailwind` via `@tailwindcss/vite`
- Create: `src-tauri/Cargo.toml`, `src-tauri/build.rs`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`, `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `src-tauri/icons/*` (generated via `pnpm tauri icon` from a placeholder PNG)

**Interfaces:**
- Produces: workspace members `crates/*` + `src-tauri`; `pnpm tauri dev` opens a window showing "Hark".

- [ ] **Step 1:** Root `Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = ["crates/*", "src-tauri"]

[workspace.package]
edition = "2021"
license = "MIT"
repository = "https://github.com/hark-app/hark"

[workspace.dependencies]
anyhow = "1"
thiserror = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
parking_lot = "0.12"
crossbeam-channel = "0.5"
```
- [ ] **Step 2:** `package.json` with `@tauri-apps/cli@^2`, `@tauri-apps/api@^2`, react 19, vite 6, typescript, tailwindcss 4, `@tailwindcss/vite`, `react-router-dom@7`, `lucide-react`, vitest, `@testing-library/react`. Scripts: `dev` (vite), `build` (tsc -b && vite build), `tauri` (tauri), `test` (vitest run).
- [ ] **Step 3:** `src-tauri/tauri.conf.json`: productName `Hark`, identifier `app.hark.desktop`, `build.frontendDist = "../app/dist"`, `build.devUrl = "http://localhost:1420"`, `beforeDevCommand = "pnpm dev"`, `beforeBuildCommand = "pnpm build"`, main window 1200x780 min 900x600, `bundle.active = true`, targets `nsis`, `msi`, `dmg`.
- [ ] **Step 4:** `src-tauri/src/lib.rs` exposes `pub fn run()` building `tauri::Builder::default()` with `tauri_plugin_log`, `tauri_plugin_single_instance`; `main.rs` calls `hark_lib::run()`.
- [ ] **Step 5:** Run `pnpm install`, `cargo check --workspace`, `pnpm build`. Expected: both succeed; `pnpm tauri dev` shows the window.
- [ ] **Step 6:** Commit `chore: scaffold workspace, tauri app and react ui`.

---

### Task 2: `hark-store` — SQLite schema, meetings, settings, segments + FTS

**Files:**
- Create: `crates/hark-store/Cargo.toml` (rusqlite `bundled`, `fts5`; dirs 7; serde; uuid; chrono; thiserror)
- Create: `crates/hark-store/src/{lib.rs,db.rs,migrations.rs,meetings.rs,segments.rs,settings.rs}`
- Test: inline `#[cfg(test)]` modules using `Store::open_in_memory()`

**Interfaces (Produces):**
```rust
pub fn data_dir() -> PathBuf;                       // %APPDATA%\Hark | ~/Library/Application Support/Hark, created if missing
pub struct Store { conn: Mutex<rusqlite::Connection> }
impl Store {
  pub fn open(path: &Path) -> Result<Store>;         // runs migrations
  pub fn open_in_memory() -> Result<Store>;
  pub fn recordings_dir(&self, meeting_id: &str) -> PathBuf; // data_dir/recordings/<id>
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Meeting { pub id: String, pub title: String, pub app: Option<String>, pub started_at: DateTime<Utc>,
  pub ended_at: Option<DateTime<Utc>>, pub duration_ms: i64, pub has_video: bool, pub status: MeetingStatus,
  pub folder_id: Option<String>, pub created_at: DateTime<Utc> }
pub enum MeetingStatus { Recording, Processing, Ready, Failed }
impl Store {
  pub fn create_meeting(&self, m: &Meeting) -> Result<()>;
  pub fn update_meeting(&self, m: &Meeting) -> Result<()>;
  pub fn get_meeting(&self, id: &str) -> Result<Option<Meeting>>;
  pub fn list_meetings(&self) -> Result<Vec<Meeting>>;       // newest first
  pub fn delete_meeting(&self, id: &str) -> Result<()>;      // cascades segments
}
pub struct Segment { pub id: i64, pub meeting_id: String, pub start_ms: i64, pub end_ms: i64,
  pub speaker: Option<String>, pub text: String, pub clean_text: Option<String> }
impl Store {
  pub fn replace_segments(&self, meeting_id: &str, segs: &[NewSegment]) -> Result<()>;
  pub fn segments(&self, meeting_id: &str) -> Result<Vec<Segment>>;
  pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>>; // FTS5 over segments.text + meetings.title
  pub fn get_setting<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>>;
  pub fn set_setting<T: Serialize>(&self, key: &str, v: &T) -> Result<()>;
}
```

- [ ] **Step 1:** Write failing tests in `meetings.rs`, `segments.rs`, `settings.rs`:
```rust
#[test] fn create_list_get_delete_meeting() { let s = Store::open_in_memory().unwrap();
  let m = Meeting::new_recording("Zoom call", Some("zoom")); s.create_meeting(&m).unwrap();
  assert_eq!(s.list_meetings().unwrap().len(), 1);
  assert_eq!(s.get_meeting(&m.id).unwrap().unwrap().title, "Zoom call");
  s.delete_meeting(&m.id).unwrap(); assert!(s.get_meeting(&m.id).unwrap().is_none()); }
#[test] fn segments_roundtrip_and_fts() { let s = Store::open_in_memory().unwrap();
  let m = Meeting::new_recording("Budget review", None); s.create_meeting(&m).unwrap();
  s.replace_segments(&m.id, &[NewSegment{start_ms:0,end_ms:1500,speaker:None,text:"we need to cut the marketing budget".into()}]).unwrap();
  assert_eq!(s.segments(&m.id).unwrap().len(), 1);
  let hits = s.search("marketing", 10).unwrap(); assert_eq!(hits[0].meeting_id, m.id);
  assert!(s.search("zebra", 10).unwrap().is_empty()); }
#[test] fn settings_json_roundtrip() { let s = Store::open_in_memory().unwrap();
  s.set_setting("video_enabled", &true).unwrap();
  assert_eq!(s.get_setting::<bool>("video_enabled").unwrap(), Some(true));
  assert_eq!(s.get_setting::<bool>("missing").unwrap(), None); }
```
- [ ] **Step 2:** `cargo test -p hark-store` → compile errors (types missing).
- [ ] **Step 3:** Implement `migrations.rs` with `user_version`-based migrations; v1 SQL:
```sql
CREATE TABLE meetings(id TEXT PRIMARY KEY, title TEXT NOT NULL, app TEXT, started_at TEXT NOT NULL, ended_at TEXT,
  duration_ms INTEGER NOT NULL DEFAULT 0, has_video INTEGER NOT NULL DEFAULT 0, status TEXT NOT NULL,
  folder_id TEXT, created_at TEXT NOT NULL);
CREATE TABLE segments(id INTEGER PRIMARY KEY, meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL, speaker TEXT, text TEXT NOT NULL, clean_text TEXT);
CREATE INDEX segments_meeting ON segments(meeting_id, start_ms);
CREATE VIRTUAL TABLE segments_fts USING fts5(text, clean_text, content='segments', content_rowid='id');
CREATE TRIGGER segments_ai AFTER INSERT ON segments BEGIN INSERT INTO segments_fts(rowid,text,clean_text) VALUES(new.id,new.text,new.clean_text); END;
CREATE TRIGGER segments_ad AFTER DELETE ON segments BEGIN INSERT INTO segments_fts(segments_fts,rowid,text,clean_text) VALUES('delete',old.id,old.text,old.clean_text); END;
CREATE TRIGGER segments_au AFTER UPDATE ON segments BEGIN INSERT INTO segments_fts(segments_fts,rowid,text,clean_text) VALUES('delete',old.id,old.text,old.clean_text); INSERT INTO segments_fts(rowid,text,clean_text) VALUES(new.id,new.text,new.clean_text); END;
CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
```
  `PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;` on open. `search` uses `segments_fts MATCH ?1` joined to meetings, returns `SearchHit { meeting_id, meeting_title, segment_id, start_ms, snippet }` via `snippet(segments_fts, 0, '<b>', '</b>', '…', 12)`.
- [ ] **Step 4:** `cargo test -p hark-store` → PASS.
- [ ] **Step 5:** Commit `feat(store): sqlite schema, meetings, segments, fts search, settings`.

---

### Task 3: `hark-detect` — patterns, matcher, debounce, audio activity (pure)

**Files:**
- Create: `crates/hark-detect/Cargo.toml` (serde, toml 1, regex, chrono), `patterns.toml`, `src/{lib.rs,patterns.rs,debounce.rs,audio_activity.rs}`

**Interfaces (Produces):**
```rust
pub struct WindowInfo { pub process: String, pub title: String }           // process = exe/bundle name lowercase
pub struct Pattern { pub app: String, pub label: String, pub process: Vec<String>, pub title_any: Vec<String>, pub title_regex: Option<String> }
pub struct PatternSet(Vec<Pattern>);
impl PatternSet { pub fn builtin() -> Self; pub fn from_toml(s: &str) -> Result<Self>; pub fn merge_user(&mut self, extra: PatternSet);
  pub fn match_windows(&self, windows: &[WindowInfo]) -> Option<Detected>; }
pub struct Detected { pub app: String, pub label: String, pub title: String, pub confidence: f32 } // 1.0 app match, 0.5 audio
pub struct Debouncer { .. }   // new(reemit_after: Duration)
impl Debouncer { pub fn observe(&mut self, now: Instant, current: Option<&Detected>) -> Option<Detected>; }
pub struct AudioActivity { .. } // new(threshold_db: f32, min_active: Duration, window: Duration)
impl AudioActivity { pub fn push(&mut self, now: Instant, mic_db: f32, sys_db: f32) -> bool; } // true when both active >= min_active within window
```
`patterns.toml` entries: zoom (`zoom.exe`,`zoom.us`,`caphost.exe`; title_any `Zoom Meeting`,`Zoom Webinar`), teams (`ms-teams.exe`,`teams.exe`,`Microsoft Teams`; title_regex `(?i)\| Microsoft Teams$`), meet (`chrome.exe`,`msedge.exe`,`firefox.exe`,`brave.exe`,`arc.exe`,`Google Chrome`,`Safari`; title_any `Meet - `,`meet.google.com`), webex, discord (title_regex `(?i)^#?.+ \| Discord$` — voice channels only), slack (`Huddle`), facetime (macOS `FaceTime`).

- [ ] **Step 1:** Failing tests:
```rust
#[test] fn matches_zoom_by_process_and_title() { let p = PatternSet::builtin();
  let w = [WindowInfo{process:"zoom.exe".into(), title:"Zoom Meeting".into()}];
  let d = p.match_windows(&w).unwrap(); assert_eq!(d.app,"zoom"); assert_eq!(d.confidence,1.0); }
#[test] fn browser_without_meet_title_does_not_match() { let p = PatternSet::builtin();
  let w = [WindowInfo{process:"chrome.exe".into(), title:"Inbox - Gmail".into()}]; assert!(p.match_windows(&w).is_none()); }
#[test] fn meet_tab_matches() { let p = PatternSet::builtin();
  let w = [WindowInfo{process:"msedge.exe".into(), title:"Meet - abc-defg-hij - Microsoft Edge".into()}]; assert_eq!(p.match_windows(&w).unwrap().app,"meet"); }
#[test] fn debouncer_emits_once_until_gone_for_reemit_window() {
  let mut d = Debouncer::new(Duration::from_secs(60)); let t0 = Instant::now();
  let det = Detected{app:"zoom".into(),label:"Zoom".into(),title:"Zoom Meeting".into(),confidence:1.0};
  assert!(d.observe(t0, Some(&det)).is_some());
  assert!(d.observe(t0+Duration::from_secs(2), Some(&det)).is_none());
  assert!(d.observe(t0+Duration::from_secs(10), None).is_none());
  assert!(d.observe(t0+Duration::from_secs(30), Some(&det)).is_none()); // came back within 60s: no re-emit
  assert!(d.observe(t0+Duration::from_secs(40), None).is_none());
  assert!(d.observe(t0+Duration::from_secs(110), Some(&det)).is_some()); // gone >= 60s then back
}
#[test] fn audio_activity_requires_both_sides_for_min_duration() {
  let mut a = AudioActivity::new(-45.0, Duration::from_secs(10), Duration::from_secs(20)); let t0 = Instant::now();
  for i in 0..5 { assert!(!a.push(t0+Duration::from_secs(i*2), -20.0, -20.0)); } // 8s active
  assert!(a.push(t0+Duration::from_secs(10), -20.0, -20.0));
  let mut b = AudioActivity::new(-45.0, Duration::from_secs(10), Duration::from_secs(20));
  for i in 0..8 { assert!(!b.push(t0+Duration::from_secs(i*2), -20.0, -80.0)); } // mic only
}
```
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement (matcher: process contains any of `process` case-insensitively AND (title contains any `title_any` OR regex matches); first pattern wins. Debouncer keeps `last_app`, `last_seen`, `emitted`; AudioActivity keeps a VecDeque of `(t, both_active)` trimmed to `window`, returns true when contiguous-active span ≥ `min_active`). **Step 4:** PASS. **Step 5:** Commit `feat(detect): app patterns, debounce, audio activity detector`.

---

### Task 4: `hark-detect` — platform window enumeration

**Files:**
- Create: `crates/hark-detect/src/windows_enum.rs`; deps `windows = { version = "0.62", features = ["Win32_UI_WindowsAndMessaging","Win32_System_Threading","Win32_System_ProcessStatus","Win32_Foundation"] }` (Windows), `core-graphics = "0.25"`, `core-foundation = "0.10"` (macOS)

**Interfaces (Produces):** `pub fn list_visible_windows() -> Vec<WindowInfo>` (titles non-empty, visible only; process = lowercase exe base name / macOS `kCGWindowOwnerName`).

- [ ] **Step 1:** Windows impl: `EnumWindows` → `IsWindowVisible` → `GetWindowTextW` → `GetWindowThreadProcessId` → `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` → `QueryFullProcessImageNameW` → base name lowercase. macOS impl: `CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements)` → `kCGWindowName`, `kCGWindowOwnerName`, layer 0 only.
- [ ] **Step 2:** Test `#[test] #[ignore] fn lists_something()` (needs desktop) + a smoke that it doesn't panic. Run `cargo test -p hark-detect`. Commit `feat(detect): visible window enumeration for windows/macos`.

---

### Task 5: `hark-capture` — devices, cpal streams, DSP, WAV writer, recorder

**Files:**
- Create: `crates/hark-capture/Cargo.toml` (cpal 0.18, hound 3.5, rubato 0.16 or 5, crossbeam-channel, parking_lot, serde)
- Create: `src/{lib.rs,devices.rs,stream.rs,dsp.rs,wav.rs,recorder.rs}`

**Interfaces (Produces):**
```rust
pub struct AudioDevice { pub id: String, pub name: String, pub is_default: bool }
pub fn input_devices() -> Vec<AudioDevice>;   // mics
pub fn output_devices() -> Vec<AudioDevice>;  // for loopback pick
pub mod dsp {
  pub fn to_mono(interleaved: &[f32], channels: usize) -> Vec<f32>;
  pub fn rms_db(samples: &[f32]) -> f32;                       // -100.0 for silence
  pub fn mix(a: &[f32], b: &[f32], gain_a: f32, gain_b: f32) -> Vec<f32>; // len = max, clamps to [-1,1]
  pub struct Resampler { .. } // new(from_hz, to_hz, channels=1) ; process(&mut self, &[f32]) -> Vec<f32>
}
pub struct WavWriter { .. } // create(path, sample_rate, channels) ; write(&mut self, &[f32]) ; flush every 1s of audio ; finish()
pub struct RecordConfig { pub dir: PathBuf, pub mic_device: Option<String>, pub loopback_device: Option<String>, pub capture_system: bool }
pub enum CaptureEvent { Levels { mic_db: f32, sys_db: f32 }, Pcm16k(Vec<f32>) /* mixed mono 16 kHz, ~100 ms blocks */, Error(String) }
pub struct Recorder { .. }
impl Recorder {
  pub fn start(cfg: RecordConfig, tx: crossbeam_channel::Sender<CaptureEvent>) -> Result<Recorder>;
  pub fn pause(&self); pub fn resume(&self); pub fn elapsed(&self) -> Duration;
  pub fn stop(self) -> Result<RecordOutput>;  // RecordOutput { mic_wav, sys_wav: Option<PathBuf>, mix_wav, duration }
}
```
Loopback: `cpal::default_host().default_output_device()` then `build_input_stream` on it (WASAPI/CoreAudio loopback). If it fails (macOS < 14.6), `sys_wav = None` and an `Error` event is sent once, recording continues mic-only.

- [ ] **Step 1:** Failing DSP/WAV tests:
```rust
#[test] fn mono_downmix_averages() { assert_eq!(dsp::to_mono(&[1.0,0.0, 0.5,0.5], 2), vec![0.5,0.5]); }
#[test] fn rms_db_silence_and_full_scale() { assert_eq!(dsp::rms_db(&[0.0;100]), -100.0); assert!((dsp::rms_db(&[1.0;100]) - 0.0).abs() < 0.01); }
#[test] fn mix_clamps() { assert_eq!(dsp::mix(&[0.9],&[0.9],1.0,1.0), vec![1.0]); assert_eq!(dsp::mix(&[0.5],&[],1.0,1.0), vec![0.5]); }
#[test] fn resampler_48k_to_16k_ratio() { let mut r = dsp::Resampler::new(48000,16000); let out = r.process(&vec![0.0;48000]); assert!((out.len() as i64 - 16000).abs() < 200); }
#[test] fn wav_writer_roundtrip() { let d = tempfile::tempdir().unwrap(); let p = d.path().join("a.wav");
  let mut w = WavWriter::create(&p, 48000, 1).unwrap(); w.write(&[0.25;4800]).unwrap(); w.finish().unwrap();
  let r = hound::WavReader::open(&p).unwrap(); assert_eq!(r.spec().sample_rate, 48000); assert_eq!(r.len(), 4800); }
```
- [ ] **Step 2:** FAIL. **Step 3:** Implement `dsp.rs` (rubato `FftFixedIn` or `SincFixedIn` mono), `wav.rs` (hound `WavWriter<BufWriter<File>>`, i16 spec, `flush()` after each second), `stream.rs` (`open_input(device, cb)` returning `cpal::Stream` with f32 conversion for i16/u16 formats; `open_loopback()`), `recorder.rs` (two streams push into `Arc<Mutex<RingBuf>>`s; a worker thread every 100 ms drains both, resamples to 48 k for WAVs and 16 k for ASR, computes levels, mixes and sends `CaptureEvent`s; `pause` sets an AtomicBool the worker respects; `stop` joins the worker and finishes WAVs). **Step 4:** PASS. Add `examples/record_5s.rs` that records 5 s to a temp dir and prints levels — run it once manually. **Step 5:** Commit `feat(capture): mic + loopback recording, dsp, wav writer`.

---

### Task 6: `hark-capture` — screen video via ffmpeg sidecar

**Files:**
- Create: `crates/hark-capture/src/video.rs`

**Interfaces (Produces):**
```rust
pub struct VideoTarget { pub kind: VideoTargetKind /* Monitor(index) | Window(title) */ }
pub fn ffmpeg_capture_args(target: &VideoTarget, out: &Path, fps: u32, max_height: u32) -> Vec<String>; // pure
pub fn ffmpeg_mux_args(video: &Path, audio_wav: &Path, out: &Path) -> Vec<String>;                      // pure
pub fn ffmpeg_mix_args(mic: &Path, sys: Option<&Path>, out_wav: &Path) -> Vec<String>;                    // pure (amix)
pub struct VideoRecorder { child: Child } // start(ffmpeg_path, target, out) ; stop(self) -> Result<()> (writes "q\n" to stdin, waits ≤ 10 s, else kill)
```
Windows args: `-f gdigrab -framerate {fps} -i desktop` (monitor) or `-i title={title}`; macOS: `-f avfoundation -framerate {fps} -i "{index}:none"`; common: `-vf scale=-2:'min({max_height},ih)'` `-c:v libx264 -preset veryfast -crf 28 -pix_fmt yuv420p -movflags +frag_keyframe+empty_moov+default_base_moof -f mp4 {out}`.

- [ ] **Step 1:** Tests assert the arg vectors for a monitor target on the current OS contain `-f gdigrab`/`-f avfoundation`, `-movflags`, and end with the output path; `ffmpeg_mix_args` with `sys=None` has no `amix`. **Step 2–4:** implement, PASS. **Step 5:** Commit `feat(capture): ffmpeg screen capture, mux and mix argument builders`.

---

### Task 7: `hark-models` — hardware probe, tiers, catalog, downloader

**Files:**
- Create: `crates/hark-models/Cargo.toml` (sysinfo 0.39, nvml-wrapper 0.13 on windows, reqwest 0.13 `stream` + rustls, sha2, tokio, serde_json, futures-util)
- Create: `catalog.json`, `src/{lib.rs,hardware.rs,tiers.rs,catalog.rs,download.rs}`

**Interfaces (Produces):**
```rust
pub struct Hardware { pub cpu_cores: usize, pub ram_gb: f32, pub gpu: Option<Gpu> }  pub struct Gpu { pub name: String, pub vram_gb: f32, pub backend: GpuBackend /* Cuda | Metal */ }
pub fn probe() -> Hardware;
pub enum Tier { CpuLow, CpuHigh, Gpu }
pub fn select_tier(h: &Hardware) -> Tier;           // Gpu if vram>=6, CpuHigh if ram>=12, else CpuLow
pub struct ModelSpec { pub id: String, pub kind: ModelKind /* AsrLive|AsrQuality|Llm|Embedding|Diarize */, pub file: String, pub url: String, pub sha256: String, pub size_bytes: u64 }
pub struct Catalog { .. } impl Catalog { pub fn builtin() -> Catalog; pub fn for_tier(&self, t: Tier) -> Vec<&ModelSpec>; pub fn get(&self, id:&str)->Option<&ModelSpec>; }
pub fn model_path(models_dir: &Path, spec: &ModelSpec) -> PathBuf;
pub fn is_present(models_dir: &Path, spec: &ModelSpec) -> bool;  // exists and size matches
pub async fn download(spec: &ModelSpec, models_dir: &Path, progress: impl Fn(u64,u64)) -> Result<PathBuf>; // resumable via Range, verifies sha256, writes .part then renames
pub fn verify_sha256(path: &Path, expected: &str) -> Result<bool>;
```
Catalog entries (ggml whisper from `ggerganov/whisper.cpp` on HF; GGUF from `Qwen/Qwen3-*-GGUF`): `whisper-base`, `whisper-small`, `whisper-medium`, `whisper-large-v3-turbo`, `qwen3-1.7b-q4`, `qwen3-4b-q4`, `qwen3-8b-q4`. SHA-256 values filled from the HF file pages at implementation time (fetch with `curl -sI`/LFS metadata); size_bytes likewise.

- [ ] **Step 1:** Tests: `select_tier` for the three cases; `Catalog::builtin().for_tier(Tier::Gpu)` includes `whisper-large-v3-turbo` and `qwen3-8b-q4`; `verify_sha256` on a temp file with known content (`"hello"` → `2cf24dba…`). **Step 2–4:** implement, PASS. **Step 5:** Commit `feat(models): hardware probe, tier selection, catalog, resumable verified downloads`.

---

### Task 8: `hark-asr` — chunker, whisper engine, live transcriber

**Files:**
- Create: `crates/hark-asr/Cargo.toml` (whisper-rs 0.16 with `features = []`, optional `cuda`/`metal` features forwarded; crossbeam-channel)
- Create: `src/{lib.rs,chunker.rs,whisper.rs,live.rs}`

**Interfaces (Produces):**
```rust
pub struct Chunker { .. } // new(sample_rate=16000, window: Duration(6s), overlap: Duration(1s))
impl Chunker { pub fn push(&mut self, pcm: &[f32]) -> Option<(f64 /*start_s*/, Vec<f32>)>; pub fn flush(&mut self) -> Option<(f64, Vec<f32>)>; }
pub struct Caption { pub start_ms: i64, pub end_ms: i64, pub text: String, pub is_final: bool }
pub struct WhisperEngine { .. }
impl WhisperEngine { pub fn load(model: &Path, use_gpu: bool) -> Result<Self>;
  pub fn transcribe(&self, pcm16k: &[f32], offset_ms: i64, lang: Option<&str>) -> Result<Vec<Caption>>; } // segments with timestamps
pub struct LiveTranscriber { .. }
impl LiveTranscriber { pub fn start(engine: Arc<WhisperEngine>, rx: Receiver<Vec<f32>>, tx: Sender<Caption>) -> Self; pub fn stop(self); }
```
- [ ] **Step 1:** Chunker tests: pushing 7 s of samples yields one 6 s chunk at start 0.0; next chunk starts at 5.0 (6 − 1 overlap); `flush` returns the remainder if ≥ 0.5 s.
- [ ] **Step 2:** `WhisperEngine::transcribe` uses `FullParams::new(SamplingStrategy::Greedy{best_of:1})`, `set_language`, `set_translate(false)`, `set_no_context(true)`, `set_single_segment(false)`, `set_token_timestamps(true)`, `set_n_threads(min(8, cores))`, `set_print_*`(false); iterates `full_n_segments` to `Caption`s offset by `offset_ms`. Integration test `#[ignore]` uses `HARK_TEST_MODEL` env path on a bundled `tests/fixtures/jfk.wav` (16 k) and asserts the text contains "country".
- [ ] **Step 3:** `LiveTranscriber` thread: feed Chunker, run engine per chunk, drop captions whose text is empty/only `[BLANK_AUDIO]`, send.
- [ ] **Step 4:** `cargo test -p hark-asr` PASS; run the ignored test with a downloaded `ggml-base.en.bin` once locally. Commit `feat(asr): whisper engine, chunker, live transcriber`.

---

### Task 9: `src-tauri` wiring — state, commands, events, tray, windows, detector loop, recorder orchestration

**Files:**
- Create: `src-tauri/src/{state.rs,events.rs,tray.rs,windows.rs,detector_loop.rs,recorder.rs,commands/mod.rs,commands/meetings.rs,commands/recording.rs,commands/settings.rs,commands/models.rs,commands/devices.rs}`
- Modify: `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml` (add crates, tauri features `tray-icon`, plugins `global-shortcut`, `shell`, `dialog`, `opener`, `notification`, `os`), `src-tauri/capabilities/default.json`, `tauri.conf.json` (`bundle.externalBin: ["binaries/ffmpeg"]`, windows `popup` 360x140 alwaysOnTop decorations:false skipTaskbar, `recordbar` 520x64 alwaysOnTop decorations:false, both `visible:false`)

**Interfaces (Produces, IPC):**
- Commands: `list_meetings() -> Vec<Meeting>`, `get_meeting(id) -> MeetingDetail { meeting, segments, media: { audio: String /*asset url*/, video: Option<String> } }`, `delete_meeting(id)`, `rename_meeting(id, title)`, `start_recording(opts: { title?: string, app?: string, video: bool, target?: VideoTarget })`, `pause_recording()`, `resume_recording()`, `stop_recording() -> Meeting`, `mark_highlight()`, `recording_status() -> { state: "idle"|"recording"|"paused", meeting_id?, elapsed_ms }`, `get_settings() -> Settings`, `set_settings(Settings)`, `list_audio_devices() -> { inputs, outputs }`, `probe_hardware() -> { hardware, tier }`, `list_models() -> [{spec, present, tier}]`, `download_model(id)` (emits progress), `dismiss_detection(app, mode: "now"|"never"|"snooze")`.
- Events: `detection` `{app,label,title}`, `levels` `{mic_db,sys_db}`, `caption` `{start_ms,end_ms,text}`, `recording_state` `{state, meeting_id, elapsed_ms}`, `model_progress` `{id, done, total}`, `processing` `{meeting_id, stage, progress}`.
- `Settings { user_name, mic_device, loopback_device, capture_system: true, video_enabled: true, video_target, tier_override, live_asr_model, quality_asr_model, llm_model, hotkey: "CmdOrCtrl+Shift+R", detection_enabled: true, never_apps: [], audio_activity_enabled: true }` stored via `hark_store` settings key `settings`.

- [ ] **Step 1:** `state.rs`: `AppState { store: Arc<Store>, settings: RwLock<Settings>, recording: Mutex<Option<ActiveRecording>>, engines: Mutex<Engines> }`. `recorder.rs`: `start()` creates meeting row (status Recording), starts `hark_capture::Recorder`, optional `VideoRecorder`, spawns a forwarder thread that emits `levels`/`caption` events and feeds `LiveTranscriber` if the live model is present; `stop()` stops both, runs ffmpeg mix → `mix.wav`, mux → `screen.mp4` (if video), sets status Processing, spawns `post_process(meeting_id)` (quality whisper pass → `replace_segments`, status Ready; on error status Failed) then emits `processing`.
- [ ] **Step 2:** `detector_loop.rs`: thread every 2 s while idle: `list_visible_windows()` → `PatternSet::match_windows` (minus `never_apps`, snooze) → `Debouncer` → emit `detection` + show `popup` window positioned bottom-right via `tauri_plugin_positioner`. Audio-activity fallback runs a lightweight level probe (open mic + loopback streams only for level metering, 1 s each 10 s) — implement as `hark_capture::LevelProbe::sample(Duration) -> (mic_db, sys_db)`.
- [ ] **Step 3:** `tray.rs`: icon + menu (Open Hark, Start/Stop Recording, Pause Detection, Quit); `windows.rs`: helpers `show_popup`, `hide_popup`, `show_recordbar`, `hide_recordbar`, `focus_main`. Global shortcut from settings toggles recording. Close-to-tray on main window.
- [ ] **Step 4:** Media serving: register `asset` protocol scope for the data dir in `tauri.conf.json` (`app.security.assetProtocol.enable = true, scope = ["$APPDATA/Hark/**", "$HOME/Library/Application Support/Hark/**"]`) and return `convertFileSrc`-compatible paths.
- [ ] **Step 5:** `cargo check -p hark` clean; manual: `pnpm tauri dev`, hotkey starts/stops a recording, files appear under data dir. Commit `feat(app): tauri wiring for detection, recording, meetings, settings, models`.

---

### Task 10: UI — shell, Onboarding, Library, Meeting, Settings, Popup, RecordBar

**Files:**
- Create: `app/src/lib/ipc.ts` (typed `invoke`/`listen` wrappers mirroring Task 9), `app/src/lib/format.ts` (`fmtDuration`, `fmtDate`), `app/src/components/{Shell.tsx,Sidebar.tsx,TopBar.tsx,LevelMeter.tsx,Transcript.tsx,Player.tsx,EmptyState.tsx}`, `app/src/routes/{Onboarding.tsx,Library.tsx,Meeting.tsx,Settings.tsx}`, `app/src/windows/{Popup.tsx,RecordBar.tsx}`, `app/src/router.tsx` (window label from `getCurrentWindow().label` chooses Popup/RecordBar/main), shadcn components `button, card, input, switch, select, tabs, dialog, badge, scroll-area, tooltip, progress`.
- Test: `app/src/components/Transcript.test.tsx`, `app/src/lib/format.test.ts`

- [ ] **Step 1:** Failing tests: `fmtDuration(65000) === "1:05"`, `fmtDuration(3725000) === "1:02:05"`; `<Transcript segments=[…] currentMs=1200 onSeek=fn/>` highlights the segment covering 1200 ms and clicking a segment calls `onSeek(start_ms)`.
- [ ] **Step 2:** Implement components. Design: Inter via `@fontsource-variable/inter`, Tailwind 4 theme tokens (`--background`, `--foreground`, `--accent: oklch(0.62 0.19 260)`), 8 px spacing, `prefers-color-scheme` dark. Shell = 240 px sidebar (Library, Settings; bottom: recording status pill) + content.
  - **Onboarding**: 3 steps — permissions check (mic/loopback probe via `list_audio_devices`; macOS shows System Settings hints), hardware + tier (from `probe_hardware`, override select), models (list with sizes, Download buttons, progress bars; "Skip for now"). Sets `settings.onboarded = true`.
  - **Library**: search box (calls `search` when ≥ 2 chars; else `list_meetings`), list rows: title (inline rename), app badge, date, duration, status pill (Recording/Processing/Ready/Failed). Big "Record now" button.
  - **Meeting**: `<Player>` (video if present else audio with `<audio>` + simple progress), `<Transcript>` with click-to-seek and auto-scroll, header with title/app/date/duration, Delete.
  - **Settings**: Audio (mic/loopback selects, capture system toggle, live meters), Detection (enabled, audio activity, never-list with remove, hotkey text), Models (tier, per-kind model selects, download/remove, disk usage), Data (data dir path, Open folder), About.
  - **Popup** (window `popup`): label "Zoom call detected", title text, toggles Video, buttons Record / Not now / Never for this app; auto-hides after 30 s.
  - **RecordBar** (window `recordbar`): red dot + elapsed, `<LevelMeter>` ×2, last 2 caption lines, Pause/Resume, Mark, Stop; draggable via `data-tauri-drag-region`.
- [ ] **Step 3:** `pnpm test` PASS; `pnpm build` clean; manual walkthrough in `pnpm tauri dev`: popup → Record → bar shows captions → Stop → Library shows Processing → Ready → Meeting page plays and seeks.
- [ ] **Step 4:** Commit `feat(ui): onboarding, library, meeting, settings, popup and record bar`.

---

### Task 11: M1 acceptance + docs stub

- [ ] **Step 1:** `README.md` (name, one-paragraph pitch, status "M1 — recording + live captions", dev setup: rustup, pnpm, `pnpm install`, `pnpm tauri dev`, ffmpeg placement under `src-tauri/binaries/ffmpeg-<target-triple>`), `LICENSE` (MIT), `docs/dev/architecture.md` (crate map from the spec).
- [ ] **Step 2:** `cargo test --workspace` and `pnpm test` green; `cargo clippy --workspace -- -D warnings` clean.
- [ ] **Step 3:** Commit `docs: readme, license, architecture; chore: clippy clean` and tag `m1`.

---

## Self-review

- **Spec coverage (M1 scope):** tray ✔ T9, detector + patterns + audio fallback ✔ T3/T4/T9, popup manual-only ✔ T9/T10, hotkey ✔ T9, mic + system audio separate tracks ✔ T5, optional screen video ✔ T6/T9, crash-safe writes ✔ T5 (1 s flush) / T6 (fragmented MP4), recording bar with live captions ✔ T8/T10, basic library + player ✔ T2/T10, model tiers + download ✔ T7, error handling: loopback unsupported → mic-only ✔ T5, GPU fallback → `WhisperEngine::load(use_gpu)` retried with `false` in T9, missing models → recording still works, transcription skipped ✔ T9.
- **Deferred to later plans:** diarization, cleanup pass, summaries/templates, chat, folders/tags, clips, share, export/import, installers/CI/site.
- **Type consistency:** `Meeting`, `Segment`, `Caption`, `CaptureEvent`, `RecordConfig`, `Detected`, `Settings` names are used identically in T2/T5/T8/T9/T10.
