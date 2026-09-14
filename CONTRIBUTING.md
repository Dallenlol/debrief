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

Run the headless smoke (records 8 s, transcribes, diarizes, summarises) after building the app:

```bash
HARK_SMOKE=1 HARK_SMOKE_CHAT=1 ./target/debug/hark
```

## Where things live

`docs/dev/architecture.md` has the crate map. Pure logic goes in crates with unit tests; Tauri glue stays in `src-tauri/`; the UI never touches files directly.

## Pull requests

- One change per PR, with a test where the logic is testable.
- Match the surrounding style (`cargo fmt`, Prettier defaults).
- Describe what you verified on which OS.

## Good first issues

Look for the `good first issue` label. Model catalog additions, detection patterns for more meeting apps, and summary templates are great starting points.
