# Architecture

Hark is a Tauri 2 desktop app: a Rust core split into small crates, and a React/TypeScript UI. The crates know nothing about Tauri; `src-tauri/` wires them to IPC commands and events.

```
crates/
  hark-store     SQLite (+FTS5) schema, meetings, segments, settings; data-dir layout
  hark-detect    meeting detection: app/window patterns, debounce, audio-activity fallback, OS window enumeration
  hark-capture   cpal mic + system-audio loopback, Windows per-process loopback (WASAPI), DSP, WAV writer, ffmpeg video helpers
  hark-models    hardware probe, model tiers, catalog.json, resumable SHA-256-verified downloads
  hark-asr       whisper.cpp engine, live transcriber (utterance buffer with prompt carry-over, silence gate, hallucination filter)
  hark-diarize   pure diarization logic: turn->caption mapping, "me" detection, voice matching, cluster consolidation, slice planning/stitching
  hark-diarize-cli  sidecar binary: sherpa-onnx (pyannote + TitaNet) over a WAV window, JSON out
  hark-llm       llama.cpp + OpenAI-compatible backends; cleanup, summary (chunked), title, chat, follow-up email, live notes, embeddings
  hark-calendar  .ics parsing and event matching
  hark-share     LAN share server, .hark bundles, standalone HTML export
src-tauri/
  src/lib.rs          app builder, plugins, idle engine unloader (skips while `AppState::busy` > 0), orphan recovery on start
  src/state.rs        AppState: store, settings, engines, active recording, live feed, busy guard
  src/recorder.rs     start/stop/pause, stream reopen on device loss, orphan recovery, manual picker
  src/pipeline.rs     post-call pipeline: mux -> transcribe (parallel with sharded diarization) -> cleanup -> chunks/embeddings -> summary
  src/asr_pick.rs     per-machine measured whisper speed; picks the model that fits the recording on CPU
  src/live_feed.rs    captions + running notes for the recording in progress
  src/import.rs       audio/video file -> mix.wav -> pipeline
  src/detector_loop.rs 2 s poll -> `detection` event + popup window (never records)
  src/tray.rs, windows.rs, shortcuts.rs
  src/commands/*.rs   IPC surface (meetings, recording, settings, models, devices, ai, speakers, organize)
app/
  src/lib/ipc.ts      typed commands + events (the only place that imports Tauri APIs)
  src/routes/*        Onboarding, Library, Meeting (transcript / summary / chat, live view while recording), Ask, Settings
  src/components/*    Transcript, SpeakerChip, ChatPanel, LiveView, Player, ShareDialog, ...
  src/lib/export.ts, talktime.ts   pure helpers (txt/srt/md rendering, talk-time shares) with unit tests
  src/windows/*       Popup (the call prompt and the manual picker) and RecordBar (`index.html?window=...`)
```

## Recording data flow

1. `detector_loop` matches visible windows against `patterns.toml` (or audio activity) -> `detection` event -> popup. Manual starts (Record now, hotkey, tray) emit a synthetic `manual` detection so the same popup acts as the picker.
2. User picks a window and audio mode and clicks Record -> `recorder::start`: `hark_capture::Recorder` opens the mic and either a device loopback (everything) or a Windows process loopback (that app's process tree). Streams resample to 48 kHz in their callbacks; a worker thread writes `mic.wav`, `sys.wav`, `mix.wav` every 100 ms and emits levels + 16 kHz mixed PCM. A stalled or errored stream is reopened on the current device without stopping the recording; stream teardown never runs on the caller's thread.
3. 16 kHz PCM feeds `hark_asr::LiveTranscriber`: a growing utterance buffer re-transcribed every 2 s (provisional caption), committed on a 0.8 s pause or at 15 s, with the committed text carried as whisper's prompt. Captions go to the record bar and to `live_feed`, whose notes loop asks the LLM for updated running notes ~45 s after new lines (max every 2 min).
4. Optional ffmpeg sidecar records the screen to a fragmented MP4 (`screen.raw.mp4`), tied to the app with a kill-on-close job object.
5. Stop (off the main thread) -> `pipeline::post_process`: mux video; start sharded diarization (N sidecar processes over slices of `mix.wav`, N from the core count) and, in parallel, the quality whisper pass with the model `asr_pick` chose; map clusters onto segments, consolidate to <= 8 speakers, suggest known voices; then LLM cleanup, retrieval chunks + embeddings, summary + auto-title, webhook. Every stage fails soft and records `meeting.error`.
6. On launch, meetings still in `recording` or `processing` from a previous session are finished from the files on disk.

## Conventions

- Pure logic gets unit tests inside its crate; platform glue stays thin.
- Commands return `Result<T, String>`; the UI shows the string.
- Events are broadcast to all windows (`app.emit`); payload structs live in `src-tauri/src/events.rs`.
- Nothing side-effectful happens without a user action.
