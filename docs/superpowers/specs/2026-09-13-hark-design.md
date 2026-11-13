# Hark — Design Spec

**Date:** 2026-09-13
**Status:** Approved by owner, pre-implementation

## 1. Summary

Hark is a free, open-source (MIT), local-only meeting recorder for Windows and macOS. It detects that a meeting is happening, offers a **manual** Record button (never records on its own), captures mic + system audio (+ optional screen video), transcribes with speaker identification, cleans the transcript with a local LLM, produces instant summaries from user-editable templates, and lets the user chat with a local AI over one meeting, a folder, or their whole library. All data stays on the machine; sharing is via LAN links or exported bundles.

Positioning: "the open-source, offline Fathom alternative."

### Non-goals
- No cloud, accounts, telemetry, or bots that join calls.
- No automatic recording, automatic speaker assignment, or automatic sharing. Every side-effectful action is a user click.
- No mobile apps, no Linux build in v1 (code should not preclude it).

## 2. Decisions taken during brainstorming

| Topic | Decision |
|---|---|
| Hardware | Support CPU-only (8 GB RAM minimum) and GPU; auto-select model sizes by detected hardware, user-overridable. |
| LLM | Bundled llama.cpp with a GGUF downloaded on first run; optional OpenAI-compatible endpoint (Ollama, LM Studio, etc.) in settings. |
| Capture | Audio always (mic + system audio as separate tracks); screen video optional toggle, default ON, remembered. |
| Sharing | LAN link served by the app while it runs + export bundle / standalone HTML. |
| Detection | Known-app/window detection (primary) + audio-activity fallback + global hotkey + tray. Popup only; nothing records without a click. |
| Stack | Tauri 2.11 (Rust core) + React/TypeScript UI. AI engines linked natively (whisper-rs 0.16, llama-cpp-2 0.1.x, sherpa-rs 0.6). cpal 0.18 for mic + loopback. ffmpeg sidecar for screen video, mixing and clips. |
| Platforms | Windows (`.exe` NSIS + `.msi`, CPU and CUDA variants), macOS (`.dmg`, universal, Metal). |
| Name | **Hark** (working name; can change before repo publication). |

## 3. Architecture

```
+--------------------------------------------------------------+
| Tauri 2 app (single process)                                  |
|                                                               |
|  UI (React/TS, Vite, Tailwind, shadcn/ui)                     |
|   - main window: Library, Meeting page, Chat, Settings        |
|   - popup window: "Call detected - Record?" (always-on-top)   |
|   - recording bar window: timer, live captions, stop, mark    |
|           ^ Tauri IPC (commands + events)                     |
|  Rust core (crates/)                                          |
|   - hark-detect    meeting detection (apps + audio activity)  |
|   - hark-capture   mic + system audio + screen video          |
|   - hark-asr       whisper.cpp streaming + batch              |
|   - hark-diarize   sherpa-onnx segmentation + embeddings      |
|   - hark-llm       llama.cpp / OpenAI-compatible client       |
|   - hark-store     SQLite + FTS5 + vector table, file layout  |
|   - hark-share     LAN HTTP server, export/import bundles     |
|   - hark-models    hardware probe, model catalog, downloads   |
+--------------------------------------------------------------+
Data dir: %APPDATA%\Hark  |  ~/Library/Application Support/Hark
  hark.db, models/, recordings/<meeting-id>/{mic.wav, sys.wav, mix.opus,
  screen.mp4, transcript.raw.json, transcript.clean.json, summary.md, chat.json}
```

Each crate exposes a small, testable Rust API and has no knowledge of Tauri. `src-tauri/` wires crates to IPC commands and events. The UI never touches files directly.

### Performance posture
- Idle: tray + detector only. Target < 60 MB RSS, ~0% CPU.
- Models load lazily and unload after 10 min idle (configurable).
- Recording writes to disk continuously (WAV chunks + MP4 fragments) - a crash loses < 5 s.
- Post-call processing runs in a background queue; UI shows progress and stays usable.

## 4. Meeting detection (`hark-detect`)

- Poll every 2 s (paused while recording).
- **App detection:** enumerate foreground and visible windows; match process name + window title patterns:
  - Zoom (`Zoom.exe` / `zoom.us`, title contains "Zoom Meeting"), Microsoft Teams (`ms-teams.exe`/`Teams`, title contains "| Microsoft Teams" and a call-state keyword), Google Meet (browser tab title `Meet - ` or `meet.google.com`), Webex, Discord (voice channel title), Slack huddle, FaceTime (macOS).
  - Pattern table lives in `detect/patterns.toml`; users can add patterns in settings.
- **Audio-activity fallback:** system audio output level > threshold AND mic input level > threshold for >= 10 s within a 20 s window -> `PossibleCall { app: Unknown }`.
- Emits `MeetingDetected { app, title, confidence }` once per candidate; re-emits only after the candidate disappears for >= 60 s ("debounce").
- Per-app suppression list ("Never for this app") and global "snooze 1 h".
- **Manual paths:** global hotkey (default `Ctrl/Cmd+Shift+R`) toggles recording; tray menu has Start/Stop.

## 5. Capture (`hark-capture`)

- **Mic:** `cpal`, default input device (selectable), 16 kHz mono float for ASR + 48 kHz for archive.
- **System audio:**
  - Windows: WASAPI loopback of the default render device (cpal builds an input stream on the output device).
  - macOS 14.6+: CoreAudio process-tap loopback, also via cpal on the default output device. Requires Audio Recording permission; app guides the user through granting it. macOS < 14.6: system audio unsupported (mic-only recording, documented).
- Tracks kept **separate** (`mic.wav`, `sys.wav`) and also mixed to `mix.opus` for playback/export. Mic track = "me"; this is passed to diarization as a hint.
- **Screen video:** bundled `ffmpeg` sidecar. Windows: `ddagrab`/`gdigrab` of chosen monitor or window; macOS: `avfoundation` screen device. 1080p max, 15 fps, H.264 fragmented MP4 (crash-safe). Audio is muxed in post from `mix.opus`.
- Recording bar: elapsed time, level meters, live captions (last 2 lines), Pause/Resume, Stop, Mark highlight (stores timestamp).
- Stop -> writes `meeting` row, finalizes MP4, enqueues post-call pipeline.

## 6. Transcription & speakers (`hark-asr`, `hark-diarize`)

- **Live:** whisper.cpp via `whisper-rs`, streaming on the mixed 16 kHz stream in 6 s windows with 1 s overlap; VAD (Silero via sherpa-onnx) gates chunks. Live model = the "small/fast" tier for the hardware.
- **Post-call:** full pass with the "quality" tier model over the mixed track with word timestamps.
- **Diarization:** sherpa-onnx speaker segmentation (pyannote-segmentation ONNX) + speaker embeddings (3D-Speaker / WeSpeaker ONNX) -> clustered speaker turns. Turns overlapping high mic-track energy are labelled with the user's name. Others get `Speaker 1..n`.
- **Merge:** words assigned to the speaker turn covering their midpoint -> `transcript.raw.json` (segments: start, end, speaker_id, text, words[]).
- **Naming:** UI rename applies to all segments of that speaker in the meeting. Embedding centroid stored in `speakers` table with the name. Future meetings: cosine similarity >= 0.72 -> suggestion chip "Sarah?" that the user confirms (never auto-applied).
- **Cleanup pass** (LLM): chunked (~1,500 tokens with 200 overlap) rewrite that fixes grammar, broken English, dropped/garbled words from bad audio, filler, and obvious mis-transcriptions, preserving speaker labels and timestamps -> `transcript.clean.json`. Raw always retained; UI toggles Raw/Clean. Segment-level "diff" view highlights what changed.

## 7. Local AI agent (`hark-llm`)

- **Backends:** `llama-cpp-2` crate (bundled) or any OpenAI-compatible `/v1/chat/completions` URL (Ollama, LM Studio). Selected in settings; bundled is default.
- **Model tiers** (first-run auto-pick, overridable):

  | Hardware | Live ASR | Quality ASR | LLM |
  |---|---|---|---|
  | CPU, < 12 GB RAM | whisper base | whisper small | Qwen3-1.7B Q4_K_M |
  | CPU, >= 12 GB RAM | whisper small | whisper medium | Qwen3-4B Q4_K_M |
  | GPU >= 6 GB VRAM (CUDA/Metal) | whisper small | whisper large-v3-turbo | Qwen3-8B Q4_K_M |

  Catalog in `models/catalog.json` (name, URL, SHA-256, size, tier). Downloads resumable, verified.
- **Instant post-call output** from the clean transcript using the meeting's template: Summary, Action items (owner + due if stated), Decisions, Open questions, Key moments (timestamped). Stored as `summary.md` + structured JSON.
- **Templates:** Markdown with placeholders `{{title}} {{date}} {{participants}} {{transcript}} {{highlights}}` and a system-instruction block. Built-ins: General, Sales call, Client discovery, 1:1, Standup, Interview. User templates stored in DB; a folder can set a default template; regenerate with a different template anytime.
- **Chat ("Ask Hark"):** scope = this meeting / this folder / all meetings. Retrieval: FTS5 keyword search + embedding search (small ONNX embedding model, e.g. `bge-small-en`, via ort) over transcript chunks -> top-k merged by reciprocal rank fusion -> LLM with citations `[mm:ss]` that seek the player. Chat history per scope persisted.
- Streaming tokens to UI via Tauri events; cancel supported.

## 8. Library, search, share, export (`hark-store`, `hark-share`)

- **Schema (SQLite):** `meetings`, `folders` (nested via parent_id), `tags`, `meeting_tags`, `speakers` (name, embedding blob), `meeting_speakers`, `segments` (raw + clean text, FTS5 external-content index), `chunks` (embedding vectors), `summaries`, `templates`, `chats`, `chat_messages`, `highlights`, `shares`, `settings`.
- **Library UI:** folder tree sidebar, list/grid of meetings with duration, app icon, speakers, tags; sort/filter; global search box (FTS5 over titles, transcripts, summaries, chat). Drag meetings into folders.
- **Meeting page:** player (video if present, else waveform audio), transcript with click-to-seek and speaker colours, Raw/Clean toggle, rename speakers, Summary tab (regenerate/template picker), Chat tab, Highlights list, Share/Export buttons.
- **Clips:** select transcript range -> ffmpeg cuts MP4/MP3 with the snippet text and summary as sidecar.
- **LAN share:** `axum` server on `0.0.0.0:47123` (configurable), enabled per share; URL `http://<lan-ip>:47123/s/<32-char token>`; read-only page (player + transcript + summary). Revoke from the Shares list; server stops when there are no active shares.
- **Export:** `.hark` bundle (zip: media + transcripts + summary + chat + metadata JSON) for a meeting, folder, or all; standalone `meeting.html` (self-contained, media embedded or linked). **Import** merges by meeting UUID (no duplicates). Whole-data-folder move is also supported via Settings -> "Change data folder".

## 9. UI design language

- Modern SaaS: 8 px grid, Inter font, neutral greys with a single accent, subtle borders, generous whitespace, light/dark following OS. Keyboard-first (command palette `Ctrl/Cmd+K`).
- Screens: Onboarding (permissions, hardware probe, model download), Library, Meeting, Chat, Templates, Settings (Audio, Detection, Models/AI, Sharing, Data, Hotkeys), Popup, Recording bar.

## 10. Error handling

- Missing permissions (mic/screen) -> onboarding step with OS-specific instructions; recording button disabled until granted.
- Model download failure -> retry/resume; app fully usable for recording without models (transcription queued until models exist).
- Engine crash (whisper/llama) -> isolated in a worker thread with panic catch; job marked failed with "Retry"; recording never blocked by AI failures.
- Disk full -> recording stops gracefully, partial media kept, user notified.
- GPU init failure -> automatic CPU fallback with a notice.

## 11. Testing

- Rust unit tests per crate (pattern matching, debounce logic, segment/speaker merge, chunking, template rendering, bundle import/export round-trip, share token lifecycle).
- Integration test: fixture WAV (two speakers) -> transcript with >= 2 speakers and expected keywords; runs in CI on CPU with the base model.
- UI: Vitest component tests for transcript view, speaker rename, template editor; Playwright smoke on the built app for Library -> Meeting navigation.
- CI matrix: windows-latest (CPU + CUDA build), macos-latest (universal).

## 12. Packaging, repository, SEO

- Tauri bundler: Windows NSIS `.exe` + `.msi` (variants `hark-x64-cpu`, `hark-x64-cuda`), macOS `.dmg` universal, notarization via CI secrets when available; unsigned builds documented.
- GitHub: public repo, MIT, README (hero GIF, features, install, privacy statement, comparison table), `docs/` (install, models & hardware, privacy, architecture, contributing, FAQ), issue/PR templates, CODE_OF_CONDUCT, SECURITY.md, CHANGELOG, release workflow on tags, `good first issue` labels, repo topics (`meeting-recorder`, `fathom-alternative`, `whisper`, `llama-cpp`, `tauri`, `local-ai`, `offline`, `privacy`).
- Landing site (Astro, GitHub Pages): pages - Home, Download, vs Fathom / Otter / Fireflies, Privacy, Docs, FAQ, Changelog. Titles/meta/OG images, JSON-LD `SoftwareApplication` + `FAQPage`, sitemap.xml, robots.txt, canonical URLs, fast static output. Target queries: "open source fathom alternative", "local ai meeting recorder", "offline meeting transcription", "private meeting notes app", "meeting recorder no bot".

## 13. Milestones

1. **M1 Capture & detect** - tray, detector, popup, hotkey, mic+system audio, optional screen video, recording bar, live captions, basic Library list & player.
2. **M2 Speakers & cleanup** - post-call quality ASR, diarization, speaker naming + memory, LLM cleanup pass, Raw/Clean view.
3. **M3 AI** - templates, instant summary, chat with retrieval and citations, model catalog/downloader, endpoint option.
4. **M4 Library** - folders, tags, search, highlights, clips, LAN share, export/import, data-folder move.
5. **M5 Ship** - installers (CPU/CUDA/mac), CI, repo docs, landing site, SEO.
