# Hark 0.3.0 - E2E pass, parity gaps, installer

**Goal:** verify every 0.2.0 feature end to end on Windows, fix what breaks, close the three highest-value gaps found against Fathom / Otter / Fireflies / tl;dv / Granola / Fellow / Krisp / Notta, and ship a fresh CPU installer that any Windows user can run.

**Principles unchanged:** local-only, manual by design, every stage fails soft.

## 1. Bug fix: renamed speakers must reach Ask Hark

`rename_speaker` updates `segments.speaker` but the retrieval chunks (built by `rebuild_chunks`, which bakes `"<speaker>: <text>"` into each chunk) and their embeddings are left stale. After "Speaker 2" -> "Sarah", a question about Sarah retrieves passages labelled "Speaker 2" and the model cannot connect them.

Fix: `rename_speaker` and `accept_speaker_suggestion` commands rebuild chunks + re-embed in a background thread (`pipeline::reembed`) after the store update. Same for `rerun`-style label edits. Store test: rename then `chunks()` contains the new name.

## 2. Import a recording

**Command** `import_recording(path: String, title: Option<String>) -> Meeting`
- Requires ffmpeg (bundled sidecar); error otherwise.
- Meeting: `Meeting::new_recording(title or file stem, None)`, `status = Processing`, `started_at` = file modified time (fallback now), `app = None`.
- `ffmpeg -i <path> -vn -ac 1 -ar 16000 -c:a pcm_s16le recordings/<id>/mix.wav`; on failure delete the meeting dir + row and return the ffmpeg stderr.
- If the input has a video stream (`ffmpeg -i` stderr mentions `Video:`), try `-c:v copy -c:a aac screen.mp4`; on failure continue audio-only (`fable:` no transcode, keep it fast).
- `duration_ms` from the WAV sample count; `ended_at = started_at + duration`.
- Then `std::thread::spawn(post_process)`; the existing pipeline handles the rest. No `mic.wav` means `me_cluster` returns `None`, so all voices become "Speaker N" (no "Me").

**UI** Library header: "Import" button (`plugin-dialog` `open` with audio/video filters, multiple files allowed, each imported sequentially). Also shown in the empty state. Navigates to the new meeting on single import.

## 3. Follow-up email + Copy / Save

**Command** `draft_followup(id) -> String` (blocking thread): prompt = system `FOLLOWUP_SYSTEM` (write a short, friendly follow-up email from the meeting notes: greeting, 1-2 line recap, decisions, action items with owners, next step, sign-off with the user's name; same language as the notes; plain text, no subject line placeholders) + user = title, date, participants, summary markdown (or the first 12k chars of transcript when no summary). `max_tokens 700`. Returns trimmed text; error when no LLM.

**UI** Summary tab: "Follow-up email" button next to Regenerate -> panel with the draft in a textarea, "Copy" and "Regenerate", "Close". Draft is kept in component state only (`fable:` persist under settings key `followup:<id>` if people ask).

**Copy / Save** New "Export" `Menu` on the meeting header (replaces nothing): Copy transcript, Copy summary, Save transcript as .txt / .md / .srt, Save summary as .md. Text is rendered client-side from `detail.segments` (clean text when the Cleaned view is active) - pure functions in `app/src/lib/export.ts` with unit tests (SRT numbering + timestamps, speaker prefixes).

## 4. Talk time

Pure function `talkTime(segments) -> { name, ms, pct }[]` in `app/src/lib/talktime.ts` (unit-tested): sum `end - start` per speaker label, ignore `null` speakers, sort desc. Meeting header shows a single stacked bar with a legend ("Sarah 54%, Me 31%, Speaker 3 15%") when there are >= 2 speakers. Updates live after rename because it reads segment labels.

## 5. E2E pass

1. `cargo test --workspace`, `pnpm test`, `pnpm build`, `cargo clippy`.
2. Launch the release build; drive the real app: onboarding, model download, manual Record (mic + system audio) with two distinct voices via a played-back sample, stop, wait for Ready, rename two speakers, confirm the suggestion on a second recording, summary, follow-up email, Ask Hark asks "what did <name> say" and gets a cited answer, import an .mp3, talk-time bar, export .srt, share bundle, settings pages.
3. `HARK_SMOKE=1 HARK_SMOKE_CHAT=1` run as the scripted regression.

## 6. Installer

`node scripts/prepare-bundle.mjs && pnpm tauri build` (CPU). Output `target/release/bundle/nsis/Hark_0.3.0_x64-setup.exe`. Version bump 0.3.0 in `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`; CHANGELOG entry.

## Out of scope this pass
Custom vocabulary, a cross-meeting Tasks view, typed-notes merge, CRM/Slack sync, sentiment/coaching.
