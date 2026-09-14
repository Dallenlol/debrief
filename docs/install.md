# Installing Hark

## Windows

Download from the [Releases page](https://github.com/Dallenlol/hark/releases):

- **CUDA** installer if you have an NVIDIA GPU (much faster transcription and summaries).
- **CPU** installer otherwise. Works on any 64-bit Windows 10/11 machine with 8 GB RAM.

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
3. Download the models (one time, from Hugging Face, checksum-verified). You can record before they finish; transcripts appear once the speech model is in.

## Data location

- Windows: `%APPDATA%\Hark`
- macOS: `~/Library/Application Support/Hark`

Move it from Settings > Data > Change folder.
