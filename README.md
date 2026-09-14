# Hark

**A free, open-source meeting recorder that keeps everything on your computer.**

Hark notices when you're in a call, offers a **Record** button (it never records on its own), captures your mic and system audio (plus optional screen video), transcribes with speaker labels, and lets you ask a private, on-device AI about any meeting. No accounts. No cloud. No bot joining your calls.

> Think of it as an open-source, offline alternative to Fathom, Otter and Fireflies.

## Status

Early development. Current milestone: **M1 - recording, detection and live captions**.

| Milestone | What it adds | Status |
|---|---|---|
| M1 | Call detection popup, hotkey, mic + system audio, optional screen video, live captions, library and player | In progress |
| M2 | Speaker identification, naming speakers, AI cleanup of messy transcripts | Planned |
| M3 | Instant summaries, custom templates, "Ask Hark" chat over meetings | Planned |
| M4 | Folders, tags, search, clips, LAN share links, export/import | Planned |
| M5 | Signed installers (Windows / macOS), website | Planned |

## Principles

- **Manual by design.** Detection only shows a popup. Recording, speaker names, and sharing all require a click.
- **Local only.** Recordings, transcripts, summaries and chat live in one folder you can back up or move. The only network call is downloading AI models from Hugging Face (verified by checksum).
- **Lightweight.** A Tauri app (Rust + React). Idle, it's a tray icon and a 2-second window poll. Models load only when needed.
- **Runs on real hardware.** Model sizes are picked for your machine: CPU-only laptops work; a GPU makes everything better.

## Building from source

Requirements: [Rust](https://rustup.rs) (stable), [Node 22+](https://nodejs.org) with [pnpm](https://pnpm.io), CMake, a C++ toolchain (MSVC Build Tools on Windows, Xcode CLT on macOS), and libclang (LLVM) for the whisper.cpp bindings.

```bash
pnpm install
# put an ffmpeg binary at src-tauri/binaries/ffmpeg-<target-triple>[.exe]
#   e.g. src-tauri/binaries/ffmpeg-x86_64-pc-windows-msvc.exe
#        src-tauri/binaries/ffmpeg-aarch64-apple-darwin
pnpm tauri dev
```

Tests:

```bash
cargo test --workspace
pnpm test
```

GPU builds: `pnpm tauri build --features cuda` (Windows, needs CUDA toolkit) or `--features metal` (macOS).

## Data location

- Windows: `%APPDATA%\Hark`
- macOS: `~/Library/Application Support/Hark`

Inside: `hark.db` (SQLite), `models/`, and `recordings/<meeting-id>/` with `mic.wav`, `sys.wav`, `mix.wav` and `screen.mp4`.

## Contributing

See [docs/dev/architecture.md](docs/dev/architecture.md) for the crate map. Issues and PRs welcome; please keep the "manual by design" and "local only" principles.

## License

MIT
