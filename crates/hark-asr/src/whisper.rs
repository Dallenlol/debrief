//! whisper.cpp wrapper. One `WhisperEngine` per loaded model; `transcribe`
//! creates a fresh decoding state per call so it is safe to share behind `Arc`.

use serde::{Deserialize, Serialize};
use std::path::Path;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Caption {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    /// False for live (streaming) results that may be revised by the post-call pass.
    pub is_final: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum AsrError {
    #[error("whisper: {0}")]
    Whisper(String),
    #[error("model not found: {0}")]
    ModelNotFound(String),
}

pub struct WhisperEngine {
    ctx: WhisperContext,
    threads: i32,
    pub gpu: bool,
    /// Catalog id of the loaded model ("" when loaded from an arbitrary path).
    pub model_id: String,
}

impl WhisperEngine {
    /// Load a ggml model. With `use_gpu`, tries the GPU build first and falls
    /// back to CPU if initialisation fails.
    pub fn load(model: &Path, use_gpu: bool) -> Result<Self, AsrError> {
        if !model.exists() {
            return Err(AsrError::ModelNotFound(model.display().to_string()));
        }
        whisper_rs::install_logging_hooks();
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8) as i32;
        let try_load = |gpu: bool| {
            let mut p = WhisperContextParameters::default();
            p.use_gpu(gpu);
            WhisperContext::new_with_params(model, p)
        };
        if use_gpu {
            if let Ok(ctx) = try_load(true) {
                return Ok(WhisperEngine { ctx, threads, gpu: true, model_id: String::new() });
            }
            tracing::warn!("whisper GPU init failed; falling back to CPU");
        }
        let ctx = try_load(false).map_err(|e| AsrError::Whisper(e.to_string()))?;
        Ok(WhisperEngine { ctx, threads, gpu: false, model_id: String::new() })
    }

    /// Tag the engine with the catalog id it was loaded from.
    pub fn tagged(mut self, model_id: &str) -> Self {
        self.model_id = model_id.to_string();
        self
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Transcribe 16 kHz mono f32 audio. Timestamps are shifted by `offset_ms`.
    /// `lang` is an ISO code like "en"; `None` = auto-detect.
    pub fn transcribe(&self, pcm16k: &[f32], offset_ms: i64, lang: Option<&str>, is_final: bool) -> Result<Vec<Caption>, AsrError> {
        self.transcribe_with(pcm16k, offset_ms, lang, is_final, 0)
    }

    /// Like `transcribe`, but splits segments at word boundaries so no segment
    /// exceeds `max_len` characters (0 = whisper's default segmentation).
    /// Short segments make speaker assignment much more precise.
    pub fn transcribe_with(&self, pcm16k: &[f32], offset_ms: i64, lang: Option<&str>, is_final: bool, max_len: i32) -> Result<Vec<Caption>, AsrError> {
        Ok(self.transcribe_opts(pcm16k, &TranscribeOpts { offset_ms, lang, is_final, max_len, ..TranscribeOpts::default() })?.captions)
    }

    /// Full-control transcription: optional prompt (text that came just before
    /// this audio, for continuity), a no-speech cut-off, and the language whisper
    /// settled on (useful to lock the language for the rest of a live session).
    pub fn transcribe_opts(&self, pcm16k: &[f32], o: &TranscribeOpts) -> Result<Transcribed, AsrError> {
        if pcm16k.len() < 1600 {
            return Ok(Transcribed::default());
        }
        let mut state = self.ctx.create_state().map_err(|e| AsrError::Whisper(e.to_string()))?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(self.threads);
        params.set_language(o.lang);
        params.set_translate(false);
        params.set_no_context(true);
        params.set_single_segment(false);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        params.set_token_timestamps(true);
        params.set_max_len(o.max_len);
        params.set_split_on_word(o.max_len > 0);
        if let Some(p) = o.prompt.filter(|p| !p.trim().is_empty()) {
            params.set_initial_prompt(p);
        }
        state.full(params, pcm16k).map_err(|e| AsrError::Whisper(e.to_string()))?;

        let lang = whisper_rs::get_lang_str(state.full_lang_id_from_state()).map(str::to_string);
        let mut out = Vec::new();
        for seg in state.as_iter() {
            let text = seg.to_str_lossy().map(|c| c.trim().to_string()).unwrap_or_default();
            if text.is_empty() || is_noise_marker(&text) {
                continue;
            }
            if let Some(max) = o.drop_no_speech_above {
                if seg.no_speech_probability() > max {
                    continue;
                }
            }
            // whisper timestamps are in 10 ms units.
            out.push(Caption {
                start_ms: seg.start_timestamp() * 10 + o.offset_ms,
                end_ms: seg.end_timestamp() * 10 + o.offset_ms,
                text,
                is_final: o.is_final,
            });
        }
        Ok(Transcribed { captions: out, lang })
    }
}

/// Options for [`WhisperEngine::transcribe_opts`].
#[derive(Debug, Clone, Default)]
pub struct TranscribeOpts<'a> {
    pub offset_ms: i64,
    /// ISO code like "en"; `None` = auto-detect.
    pub lang: Option<&'a str>,
    pub is_final: bool,
    /// Max characters per segment (0 = whisper's own segmentation).
    pub max_len: i32,
    /// Text spoken just before this audio; keeps names and phrasing consistent.
    pub prompt: Option<&'a str>,
    /// Drop segments whose no-speech probability exceeds this (hallucination guard).
    pub drop_no_speech_above: Option<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct Transcribed {
    pub captions: Vec<Caption>,
    /// Language whisper decided on (auto-detect) or was told.
    pub lang: Option<String>,
}

/// whisper emits bracketed markers for non-speech; drop them.
pub fn is_noise_marker(text: &str) -> bool {
    let t = text.trim();
    (t.starts_with('[') && t.ends_with(']')) || (t.starts_with('(') && t.ends_with(')')) || t == "..."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_markers_detected() {
        assert!(is_noise_marker("[BLANK_AUDIO]"));
        assert!(is_noise_marker(" (music) "));
        assert!(!is_noise_marker("hello [there]"));
    }
}
