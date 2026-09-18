# Installing Hark

## Windows

Download from the [Releases page](https://github.com/Dallenlol/hark/releases):

- **CUDA** installer if you have an NVIDIA GPU (GTX 10-series or newer, 6 GB+ VRAM, a recent driver). Everything it needs is inside the installer; no CUDA toolkit to install. Transcription and summaries run several times faster: an 88-minute call is fully processed in about 10 minutes.
- **CPU** installer otherwise. Works on any 64-bit Windows 10/11 machine with 8 GB RAM and a CPU from 2013 or later (AVX2). Hark picks lighter speech models so long recordings still finish in reasonable time.

Run the `.exe` (per-user install, no admin needed). Windows SmartScreen may warn because the build is not code-signed yet; choose "More info > Run anyway". If you prefer, verify the SHA-256 listed on the release.

## macOS

Download the `.dmg` for **Apple Silicon** (M1 and later, uses Metal) or **Intel**. Drag Hark to Applications.

The build is not notarized yet: on first launch right-click the app > Open, or run `xattr -dr com.apple.quarantine /Applications/Hark.app`.

Permissions Hark asks for, and why:

- **Microphone**: your side of the call.
- **Audio recording (system audio)**: the other participants. Requires macOS 14.6 or later; on older versions Hark records mic only.
- **Screen recording**: only if you turn on screen video.

## First run

1. Check the mic and system audio meters move.
2. Hark picks model sizes for your hardware; override if you like.
3. Download the models (one time, from Hugging Face, checksum-verified). Start with the essentials and record right away; the bigger models finish in the background and Hark catches up on clean-up and summaries by itself.

## Recording

Every way to start - the call popup, **Record now**, the hotkey (`Ctrl/Cmd+Shift+R`), the tray - opens the same picker:

- **Screen / Window**: the meeting window is preselected when Hark detected the call; pick any window or display. Turn "Screen on" off to record audio only.
- **Audio**: *This app only* (Windows) records just the chosen window's application plus your mic; *Everything playing* records all system audio plus your mic. On macOS all system audio is recorded.

While recording, open the meeting from the Library to watch captions and running notes. Stop takes a few seconds (last captions, video trailer), then the full transcript, speakers, summary and Ask Hark arrive.

You can also **Import** an audio or video file you already have (Library > Import): it goes through the same pipeline.

## Data location

- Windows: `%APPDATA%\Hark`
- macOS: `~/Library/Application Support/Hark`

Move it from Settings > Data > Change folder.
