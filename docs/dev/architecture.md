# Architecture

Hark is a Tauri 2 desktop app: a Rust core split into small crates, and a React/TypeScript UI. The crates know nothing about Tauri; `src-tauri/` wires them to IPC commands and events.

```
crates/
  hark-store     SQLite (+FTS5) schema, meetings, segments, settings; data-dir layout
  hark-detect    meeting detection: app/window patterns, debounce, audio-activity fallback, OS window enumeration
  hark-capture   cpal mic + system-audio loopback, DSP (resample/mix/levels), WAV writer, ffmpeg video helpers
  hark-models    hardware probe, model tiers, catalog.json, resumable SHA-256-verified downloads
  hark-asr       whisper.cpp engine, windowed chunker, live transcriber
src-tauri/
  src/lib.rs          app builder, plugins, idle engine unloader
  src/state.rs        AppState: store, settings, engines, active recording, ffmpeg path
  src/recorder.rs     start/stop/pause orchestration + post-call pipeline (mux, quality transcription)
  src/detector_loop.rs 2 s poll -> `detection` event + popup window (never records)
  src/tray.rs, windows.rs, shortcuts.rs
  src/commands/*.rs   IPC surface (meetings, recording, settings, models, devices)
app/
  src/lib/ipc.ts      typed commands + events (the only place that imports Tauri APIs)
  src/routes/*        Onboarding, Library, Meeting, Settings
  src/windows/*       Popup and RecordBar (separate always-on-top windows, `index.html?window=...`)
```

## Recording data flow

1. `detector_loop` matches visible windows against `patterns.toml` (or audio activity) -> `detection` event -> popup.
2. User clicks Record -> `recorder::start`: `hark_capture::Recorder` opens mic + loopback streams; a worker thread writes `mic.wav`, `sys.wav`, `mix.wav` (48 kHz) every 100 ms and emits levels + 16 kHz mixed PCM.
3. 16 kHz PCM feeds `hark_asr::LiveTranscriber` (6 s windows, 1 s overlap) -> `caption` events -> record bar.
4. Optional ffmpeg sidecar records the screen to a fragmented MP4 (`screen.raw.mp4`).
5. Stop -> ffmpeg muxes video + `mix.wav` -> `screen.mp4`; quality whisper pass over `mix.wav` -> `segments` table -> status Ready.

## Conventions

- Pure logic gets unit tests inside its crate; platform glue stays thin.
- Commands return `Result<T, String>`; the UI shows the string.
- Events are broadcast to all windows (`app.emit`); payload structs live in `src-tauri/src/events.rs`.
- Nothing side-effectful happens without a user action.
