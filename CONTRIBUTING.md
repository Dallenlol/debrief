# Contributing to Hark

Thanks for helping. Hark is small on purpose; the best contributions keep it that way.

## Ground rules

1. **Manual by design.** Nothing records, names a speaker, or shares without a user click. PRs that add automation must keep an explicit confirmation step.
2. **Local only.** No telemetry, no accounts, no network calls except model downloads and optional user-configured endpoints.
3. **Cross-platform.** Windows and macOS are first-class. Keep platform code behind `cfg` and small.

## Setup

See the README for toolchain requirements. Then:

```bash
pnpm install
node scripts/prepare-bundle.mjs --profile dev   # ffmpeg + diarize sidecar + runtime libs
pnpm tauri dev
```

Tests:

```bash
cargo test --workspace --exclude hark
cargo clippy --workspace --all-targets -- -D warnings
pnpm test && pnpm typecheck
```

Run the headless smoke (records 8 s, transcribes, diarizes, summarises) after building the app with `pnpm tauri build --no-bundle` (a plain `cargo build` produces a dev binary that expects the Vite dev server):

```bash
HARK_SMOKE=1 HARK_SMOKE_CHAT=1 ./target/release/hark
```

## Where things live

`docs/dev/architecture.md` has the crate map. Pure logic goes in crates with unit tests; Tauri glue stays in `src-tauri/`; the UI never touches files directly.

To drive the real built app for an end-to-end check, launch it with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` and attach with `playwright-core`'s `connectOverCDP`; the main window is the `tauri.localhost` page without `?window=`. Native dialogs cannot be driven that way - call the command (`import_recording`, `start_recording`, ...) through `window.__TAURI_INTERNALS__.invoke` instead.

Build hygiene: the CPU build must stay portable (`.cargo/config.toml` pins ggml to AVX2 - never `GGML_NATIVE`); `cargo clean -p <crate>` only clears the dev profile, add `--release` when a build-script input changed. A second variant (CUDA) should use its own `CARGO_TARGET_DIR`; `scripts/prepare-bundle.mjs` honours it and clears stale runtime libraries before collecting.

## Pull requests

- One change per PR, with a test where the logic is testable.
- Match the surrounding style (`cargo fmt`, Prettier defaults).
- Describe what you verified on which OS.

## Good first issues

Look for the `good first issue` label. Model catalog additions, detection patterns for more meeting apps, and summary templates are great starting points.
