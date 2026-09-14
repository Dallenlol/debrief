# Hark M2 — Speakers & Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Post-call transcripts get speaker labels ("Me", "Speaker 1"...), users can name speakers (remembered across meetings as confirmable suggestions), and a local LLM produces a cleaned transcript alongside the raw one.

**Architecture:** Two new Tauri-agnostic crates: `hark-diarize` (sherpa-onnx speaker segmentation + embeddings, pure merge/matching logic) and `hark-llm` (llama.cpp engine + OpenAI-compatible client behind one trait, pure chunking/parsing for the cleanup pass). The post-call pipeline in `src-tauri/src/recorder.rs` gains diarize -> label -> cleanup stages. Store gains `speakers`/`meeting_speakers` tables.

**Tech Stack:** sherpa-rs 0.6 (`download-binaries`), llama-cpp-2 0.1.x (`cuda`/`metal` optional features), reqwest for the HTTP backend.

## Global Constraints

- Speaker name suggestions are never auto-applied; the UI shows a chip the user confirms.
- Raw transcript is always kept; cleanup writes `segments.clean_text` only.
- Models: `pyannote-seg-3` (5.99 MB) + `titanet-small` (40.3 MB) are required for diarization on every tier; missing models => skip diarization, keep going.
- If no LLM is available (no model / endpoint unreachable), cleanup is skipped and the meeting still becomes Ready.

---

### Task 1: `hark-diarize` — pure logic (turn merge, "me" detection, speaker matching)

**Files:** `crates/hark-diarize/{Cargo.toml,src/lib.rs,src/merge.rs,src/identify.rs,src/engine.rs}`

**Interfaces (Produces):**
```rust
pub struct Turn { pub start_ms: i64, pub end_ms: i64, pub cluster: i32 }
pub struct Labelled<T> { pub item: T, pub cluster: Option<i32> }
/// Assign each caption to the cluster with the most time overlap; None when no turn overlaps.
pub fn assign_clusters(captions: &[(i64, i64)], turns: &[Turn]) -> Vec<Option<i32>>;
/// Which cluster is the local user: the one whose turns are mic-dominant for >= 60% of their duration.
pub fn find_me_cluster(turns: &[Turn], mic_db: &[(i64, f32)], sys_db: &[(i64, f32)], margin_db: f32) -> Option<i32>;
pub fn cosine(a: &[f32], b: &[f32]) -> f32;
pub fn centroid(embs: &[Vec<f32>]) -> Vec<f32>;
pub struct KnownSpeaker { pub id: String, pub name: String, pub embedding: Vec<f32> }
/// Best known speaker above `threshold` (0.72 default) for an embedding.
pub fn best_match<'a>(emb: &[f32], known: &'a [KnownSpeaker], threshold: f32) -> Option<(&'a KnownSpeaker, f32)>;
pub struct DiarizeEngine { .. }   // engine.rs: wraps sherpa_rs::diarize::Diarize + speaker_id::EmbeddingExtractor
impl DiarizeEngine {
  pub fn load(seg_model: &Path, emb_model: &Path) -> Result<Self, DiarizeError>;
  pub fn diarize(&mut self, pcm16k: &[f32], progress: impl Fn(f32)) -> Result<Vec<Turn>, DiarizeError>;
  pub fn embed(&mut self, pcm16k: &[f32]) -> Result<Vec<f32>, DiarizeError>;   // >= 1 s of audio
}
```
- Tests: overlap assignment picks max-overlap cluster and None for gaps; `find_me_cluster` with synthetic level series; cosine/centroid/best_match threshold behaviour.
- Commit `feat(diarize): turn assignment, me-detection, speaker matching, sherpa engine`.

### Task 2: Catalog + store schema for speakers

**Files:** `crates/hark-models/catalog.json` (+2 diarize models), `crates/hark-store/src/{migrations.rs,speakers.rs,segments.rs,lib.rs}`

**Interfaces (Produces):**
```rust
// migration v2
// speakers(id TEXT PK, name TEXT, embedding BLOB, dim INTEGER, created_at TEXT)
// meeting_speakers(meeting_id, label TEXT, speaker_id TEXT NULL, suggested_id TEXT NULL, suggested_score REAL NULL, PRIMARY KEY(meeting_id,label))
pub struct Speaker { id, name, embedding: Vec<f32> }
pub struct MeetingSpeaker { meeting_id, label, speaker_id: Option<String>, suggested_id: Option<String>, suggested_name: Option<String>, suggested_score: Option<f32> }
impl Store {
  pub fn list_speakers(&self) -> Result<Vec<Speaker>>;
  pub fn upsert_speaker(&self, s: &Speaker) -> Result<()>;
  pub fn meeting_speakers(&self, meeting_id) -> Result<Vec<MeetingSpeaker>>;
  pub fn set_meeting_speakers(&self, meeting_id, rows: &[MeetingSpeaker]) -> Result<()>;
  /// Rename label -> name in segments, link/create the speaker row, store the centroid embedding.
  pub fn rename_speaker(&self, meeting_id, label: &str, name: &str, embedding: Option<&[f32]>) -> Result<()>;
  pub fn set_clean_text(&self, meeting_id, updates: &[(i64 /*segment id*/, String)]) -> Result<()>;
}
```
- Catalog entries: `pyannote-seg-3` (url `https://huggingface.co/csukuangfj/sherpa-onnx-pyannote-segmentation-3-0/resolve/main/model.onnx`, sha256 `220ad67c...1079`, 5992913 B), `titanet-small` (url `https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/nemo_en_titanet_small.onnx`, sha256 `ad4a1802...789e`, 40257283 B), both `kind: diarize`, included in every tier via a new `"common": [...]` list.
- Tests: rename applies to all segments with that label and creates a speaker; embedding blob roundtrip; clean_text update.
- Commit `feat(store,models): speaker tables and diarization models`.

### Task 3: `hark-llm` — engine, HTTP backend, cleanup pass

**Files:** `crates/hark-llm/{Cargo.toml,src/lib.rs,src/backend.rs,src/llama.rs,src/openai.rs,src/cleanup.rs,src/prompts.rs}`

**Interfaces (Produces):**
```rust
pub struct ChatMessage { pub role: Role /*System|User|Assistant*/, pub content: String }
pub struct GenOptions { pub max_tokens: usize, pub temperature: f32, pub json: bool }
pub trait LlmBackend: Send + Sync {
  fn chat(&self, msgs: &[ChatMessage], opts: &GenOptions, on_token: &mut dyn FnMut(&str) -> bool /*false=cancel*/) -> Result<String, LlmError>;
  fn name(&self) -> String;
}
pub struct LlamaEngine { .. }  // llama.rs: load(path, n_gpu_layers, n_ctx=8192); one Mutex<LlamaContext> per engine, kv cleared per call
pub struct OpenAiCompat { base_url, api_key: Option<String>, model }  // openai.rs: POST /v1/chat/completions, stream=true SSE
pub mod cleanup {
  pub struct Chunk { pub segment_ids: Vec<i64>, pub text: String }        // "[id] Speaker: text" lines
  pub fn chunk_segments(segs: &[(i64, Option<String>, String)], max_chars: usize) -> Vec<Chunk>;  // ~6000 chars, never splits a segment
  pub fn parse_cleaned(output: &str, expected_ids: &[i64]) -> Vec<(i64, String)>;  // tolerant: "[id] text" lines; unknown ids dropped
  pub fn run(backend: &dyn LlmBackend, segs: &[(i64, Option<String>, String)], progress: impl Fn(f32), cancel: impl Fn() -> bool) -> Result<Vec<(i64, String)>, LlmError>;
}
```
- Prompt (prompts.rs `CLEANUP_SYSTEM`): fix grammar, broken English, dropped/garbled words from bad audio or connection, remove filler (um/uh/you know) and false starts, keep meaning, speaker, order and the `[id]` prefix on every line; never add content; output only lines.
- Tests: chunking respects limit and never splits; parse tolerates extra prose, missing ids, reordered lines; a `MockBackend` (echo with "um " stripped) drives `run` end to end.
- Ignored test: `HARK_TEST_LLM=<gguf>` loads `LlamaEngine`, asks "Reply with the single word PONG" and asserts "PONG" in the answer.
- Commit `feat(llm): llama.cpp engine, openai-compatible backend, transcript cleanup pass`.

### Task 4: Pipeline + commands + settings

**Files:** `src-tauri/src/{recorder.rs,state.rs,settings.rs,commands/meetings.rs,commands/speakers.rs,commands/mod.rs,lib.rs,events.rs}`, `src-tauri/Cargo.toml`

- `Settings` += `llm_backend: "bundled" | "openai"`, `llm_endpoint: String` (default `http://localhost:11434/v1`), `llm_endpoint_model: String`, `llm_api_key: Option<String>`, `cleanup_enabled: bool` (true), `diarize_enabled: bool` (true).
- `AppState` += `diarize: Mutex<Option<Arc<Mutex<DiarizeEngine>>>>`, `llm: Mutex<Option<Arc<dyn LlmBackend>>>`, `fn llm() -> Option<Arc<dyn LlmBackend>>` (bundled: load `llm_model` if present; openai: construct client), `fn diarizer()`.
- `post_process` stages: `transcribe` (quality whisper, `max_len 80`, `split_on_word`) -> `diarize` (turns from `mix.wav` 16 k; per-turn mic/sys dB from `mic.wav`/`sys.wav` at 500 ms hops; `assign_clusters`; `find_me_cluster` -> user_name; other clusters -> `Speaker 1..n` by first appearance; per-cluster embedding = `embed()` on concatenated turn audio (cap 60 s); `best_match` vs `list_speakers` -> `meeting_speakers.suggested_*`) -> `cleanup` (LLM; `set_clean_text`) -> Ready. Each stage emits `processing {stage, progress}`; a failing optional stage logs + notices and continues.
- Commands: `meeting_speakers(id) -> Vec<MeetingSpeaker>`, `rename_speaker(id, label, name)` (uses stored centroid from a `speaker_embeddings:<meeting>` setting), `accept_speaker_suggestion(id, label)`, `list_known_speakers()`, `rerun_cleanup(id)`, `test_llm_endpoint(url, key, model) -> String`.
- Commit `feat(app): diarization, speaker naming/suggestions, llm cleanup in post-call pipeline`.

### Task 5: UI

**Files:** `app/src/lib/ipc.ts`, `app/src/components/Transcript.tsx`, `app/src/routes/Meeting.tsx`, `app/src/routes/Settings.tsx`, `app/src/components/SpeakerChip.tsx`

- Transcript: speaker label becomes a `SpeakerChip` (click -> inline rename input; Enter saves via `rename_speaker`; suggestion chip "Sarah? Yes / No" when `suggested_name` exists); Raw / Clean segmented toggle (default Clean when any clean_text exists); a subtle dotted underline on cleaned segments with title "edited by cleanup" on hover.
- Settings: "AI" section gains backend radio (Bundled / OpenAI-compatible endpoint + URL/model/key + Test button), toggles for diarization and cleanup.
- Tests: `SpeakerChip` rename calls handler with trimmed name; Transcript toggle shows clean text.
- Commit `feat(ui): speaker naming, suggestions, raw/clean transcript, llm backend settings`.

### Task 6: Verify

- `cargo test --workspace`, `pnpm test`, `cargo clippy -- -D warnings`.
- Smoke: `HARK_SMOKE=1` with the JFK clip playing prints segments with a speaker label and, when `qwen3-*` is present, clean text. Tag `m2`.
