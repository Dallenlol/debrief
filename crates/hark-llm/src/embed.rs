//! Sentence embeddings through llama.cpp (BERT-style GGUF such as bge-small),
//! used for semantic retrieval in Ask Hark.

use crate::backend::LlmError;
use crate::llama::backend;
use llama_cpp_2::context::params::{LlamaContextParams, LlamaPoolingType};
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use parking_lot::Mutex;
use std::num::NonZeroU32;
use std::path::Path;

/// Tokens per text; longer inputs are truncated (bge-small trains at 512).
const MAX_TOKENS: usize = 500;
const N_CTX: u32 = 2048;

pub struct EmbedEngine {
    model: LlamaModel,
    name: String,
    lock: Mutex<()>,
}

impl EmbedEngine {
    pub fn load(path: &Path) -> Result<Self, LlmError> {
        if !path.exists() {
            return Err(LlmError::ModelNotFound(path.display().to_string()));
        }
        let b = backend()?;
        let params = LlamaModelParams::default().with_n_gpu_layers(0);
        let model = LlamaModel::load_from_file(b, path, &params).map_err(|e| LlmError::Engine(e.to_string()))?;
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "embed".into());
        Ok(EmbedEngine { model, name, lock: Mutex::new(()) })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn dim(&self) -> usize {
        self.model.n_embd() as usize
    }

    /// L2-normalised mean-pooled embeddings, one per input text.
    pub fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, LlmError> {
        let _g = self.lock.lock();
        let b = backend()?;
        let n_threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8) as i32;
        let cparams = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(N_CTX))
            .with_n_batch(N_CTX)
            .with_n_ubatch(N_CTX)
            .with_n_threads(n_threads)
            .with_n_threads_batch(n_threads)
            .with_embeddings(true)
            .with_pooling_type(LlamaPoolingType::Mean);
        let mut ctx = self.model.new_context(b, cparams).map_err(|e| LlmError::Engine(e.to_string()))?;
        let mut out = Vec::with_capacity(texts.len());
        for text in texts {
            let mut tokens = self.model.str_to_token(text, AddBos::Always).map_err(|e| LlmError::Engine(e.to_string()))?;
            tokens.truncate(MAX_TOKENS);
            if tokens.is_empty() {
                out.push(vec![0.0; self.dim()]);
                continue;
            }
            let mut batch = LlamaBatch::new(N_CTX as usize, 1);
            batch.add_sequence(&tokens, 0, false).map_err(|e| LlmError::Engine(e.to_string()))?;
            ctx.clear_kv_cache();
            ctx.decode(&mut batch).map_err(|e| LlmError::Engine(e.to_string()))?;
            let v = ctx.embeddings_seq_ith(0).map_err(|e| LlmError::Engine(e.to_string()))?;
            out.push(normalize(v));
        }
        Ok(out)
    }
}

/// Unit-length copy (zero vector stays zero).
pub fn normalize(v: &[f32]) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm <= f32::EPSILON {
        return v.to_vec();
    }
    v.iter().map(|x| x / norm).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_unit_length() {
        let n = normalize(&[3.0, 4.0]);
        assert!((n[0] - 0.6).abs() < 1e-6 && (n[1] - 0.8).abs() < 1e-6);
        assert_eq!(normalize(&[0.0, 0.0]), vec![0.0, 0.0]);
    }

    /// Needs a real model: `HARK_EMBED_MODEL=path/to/bge-small-en-v1.5-q8_0.gguf`.
    #[test]
    #[ignore]
    fn real_model_embeds_similar_texts_closer() {
        let path = std::env::var("HARK_EMBED_MODEL").expect("HARK_EMBED_MODEL");
        let e = EmbedEngine::load(Path::new(&path)).unwrap();
        let v = e.embed(&["the price went up", "pricing increased", "my cat sleeps all day"]).unwrap();
        assert_eq!(v[0].len(), e.dim());
        let sim = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
        assert!(sim(&v[0], &v[1]) > sim(&v[0], &v[2]));
    }
}
