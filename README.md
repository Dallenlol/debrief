# Hark

**A free, open-source meeting recorder that keeps everything on your computer.**

Hark notices when you're in a call, offers a **Record** button (it never records on its own), captures your mic and system audio (plus optional screen video), transcribes with speaker labels, and lets you ask a private, on-device AI about any meeting. No accounts. No cloud. No bot joining your calls.

> Think of it as an open-source, offline alternative to Fathom, Otter and Fireflies.

## What it does

- **Notices calls** in Zoom, Teams, Google Meet, Webex, Discord, Slack huddles, FaceTime, GoToMeeting (and anything else, via audio activity) and shows a small **Record?** popup. A global hotkey and tray menu work anytime.
- **Records** your mic and system audio as separate tracks, plus optional screen video. Writes to disk continuously, so a crash loses seconds, not the meeting.
- **Transcribes** with live captions during the call and a full pass after, with **speaker labels**. Name a speaker once; Hark recognises their voice next time and asks you to confirm.
- **Cleans up** messy transcripts with a local language model: bad mics, broken English, filler. The raw transcript is always kept.
- **Summarises** instantly from editable templates (general, sales call, client discovery, 1:1, standup, interview).
- **Answers questions** ("Ask Hark") about one meeting or your whole library, citing timestamps you can click.
- **Organises** with folders, tags, search, highlights and clips.
- **Shares** via links on your network, `.hark` bundles you can import on another computer, or a standalone web page.

## Install

Grab an installer from [Releases](https://github.com/Dallenlol/hark/releases): Windows (CPU or CUDA) and macOS (Apple Silicon or Intel). See [docs/install.md](docs/install.md).

## Status

**0.1.0** - first public build. Everything above works; polish and more meeting-app patterns are ongoing. See [CHANGELOG.md](CHANGELOG.md).

## Principles

- **Manual by design.** Detection only shows a popup. Recording, speaker names, and sharing all require a click.
- **Local only.** Recordings, transcripts, summaries and chat live in one folder you can back up or move. The only network call is downloading AI models from Hugging Face (verified by checksum).
- **Lightweight.** A Tauri app (Rust + React). Idle, it's a tray icon and a 2-second window poll. Models load only when needed.
- **Runs on real hardware.** Model sizes are picked for your machine: CPU-only laptops work; a GPU makes everything better.

## Building from source

Requirements: [Rust](https://rustup.rs) (stable), [Node 22+](https://nodejs.org) with [pnpm](https://pnpm.io), CMake, a C++ toolchain (MSVC Build Tools on Windows, Xcode CLT on macOS), and libclang (LLVM) for the whisper.cpp bindings.

```bash
pnpm install
node scripts/prepare-bundle.mjs --profile dev   # downloads ffmpeg, builds the diarization sidecar, collects runtime libs
pnpm tauri dev
```

Installers: `node scripts/prepare-bundle.mjs && pnpm tauri build` (add `--features cuda` on Windows with the CUDA toolkit, or `--features metal` on macOS).

Tests:

```bash
cargo test --workspace
pnpm test
```

## Data location

- Windows: `%APPDATA%\Hark`
- macOS: `~/Library/Application Support/Hark`

Inside: `hark.db` (SQLite), `models/`, and `recordings/<meeting-id>/` with `mic.wav`, `sys.wav`, `mix.wav` and `screen.mp4`.

## Docs

[Install](docs/install.md) - [Models and hardware](docs/models.md) - [Privacy](docs/privacy.md) - [FAQ](docs/faq.md) - [Architecture](docs/dev/architecture.md)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Issues and PRs welcome; please keep the "manual by design" and "local only" principles.

## License

MIT
