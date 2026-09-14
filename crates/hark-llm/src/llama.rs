//! Bundled llama.cpp backend. One model, one context (serialised by a mutex),
//! KV cache cleared per request.

use crate::backend::{ChatMessage, GenOptions, LlmBackend, LlmError};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use parking_lot::Mutex;
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::OnceLock;

static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();

pub(crate) fn backend() -> Result<&'static LlamaBackend, LlmError> {
    if let Some(b) = BACKEND.get() {
        return Ok(b);
    }
    let mut b = LlamaBackend::init().map_err(|e| LlmError::Engine(e.to_string()))?;
    b.void_logs();
    let _ = BACKEND.set(b);
    Ok(BACKEND.get().expect("set"))
}

pub struct LlamaEngine {
    model: LlamaModel,
    n_ctx: u32,
    n_threads: i32,
    name: String,
    // The context borrows the model; we recreate it lazily inside the lock.
    ctx: Mutex<()>,
}

impl LlamaEngine {
    /// `n_gpu_layers`: 0 for CPU, `u32::MAX`/999 to offload everything.
    pub fn load(path: &Path, n_gpu_layers: u32, n_ctx: u32) -> Result<Self, LlmError> {
        if !path.exists() {
            return Err(LlmError::ModelNotFound(path.display().to_string()));
        }
        let b = backend()?;
        let params = LlamaModelParams::default().with_n_gpu_layers(n_gpu_layers);
        let model = LlamaModel::load_from_file(b, path, &params).map_err(|e| LlmError::Engine(e.to_string()))?;
        let n_ctx = n_ctx.min(model.n_ctx_train().max(2048));
        let n_threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8) as i32;
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "llama".into());
        Ok(LlamaEngine { model, n_ctx, n_threads, name, ctx: Mutex::new(()) })
    }

    fn render(&self, msgs: &[ChatMessage]) -> Result<String, LlmError> {
        let chat: Vec<LlamaChatMessage> = msgs
            .iter()
            .map(|m| LlamaChatMessage::new(m.role.as_str().to_string(), m.content.clone()).map_err(|e| LlmError::Engine(e.to_string())))
            .collect::<Result<_, _>>()?;
        let tmpl = self.model.chat_template(None).map_err(|e| LlmError::Engine(format!("no chat template: {e}")))?;
        let mut prompt = self.model.apply_chat_template(&tmpl, &chat, true).map_err(|e| LlmError::Engine(e.to_string()))?;
        // Qwen3: skip the thinking block for fast, deterministic answers.
        if prompt.contains("<|im_start|>assistant") && !prompt.contains("<think>") {
            prompt.push_str("<think>\n\n</think>\n\n");
        }
        Ok(prompt)
    }
}

impl LlmBackend for LlamaEngine {
    fn chat(&self, msgs: &[ChatMessage], opts: &GenOptions, on_token: &mut dyn FnMut(&str) -> bool) -> Result<String, LlmError> {
        let _guard = self.ctx.lock();
        let b = backend()?;
        let prompt = self.render(msgs)?;
        let tokens = self.model.str_to_token(&prompt, AddBos::Never).map_err(|e| LlmError::Engine(e.to_string()))?;
        let max_new = opts.max_tokens.max(16);
        let needed = (tokens.len() + max_new + 16) as u32;
        let n_ctx = needed.clamp(2048, self.n_ctx);
        if tokens.len() as u32 + 16 > n_ctx {
            return Err(LlmError::Engine(format!("prompt too long ({} tokens, context {})", tokens.len(), n_ctx)));
        }
        let cparams = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(n_ctx))
            .with_n_batch(2048)
            .with_n_threads(self.n_threads)
            .with_n_threads_batch(self.n_threads);
        let mut ctx: LlamaContext = self.model.new_context(b, cparams).map_err(|e| LlmError::Engine(e.to_string()))?;

        // Prompt in batches of n_batch.
        let n_batch = 2048usize;
        let mut pos: i32 = 0;
        for (i, chunk) in tokens.chunks(n_batch).enumerate() {
            let mut batch = LlamaBatch::new(n_batch, 1);
            let last_chunk = (i + 1) * n_batch >= tokens.len();
            for (j, t) in chunk.iter().enumerate() {
                let is_last = last_chunk && j + 1 == chunk.len();
                batch.add(*t, pos, &[0], is_last).map_err(|e| LlmError::Engine(e.to_string()))?;
                pos += 1;
            }
            ctx.decode(&mut batch).map_err(|e| LlmError::Engine(e.to_string()))?;
        }

        let mut sampler = if opts.temperature <= 0.0 {
            LlamaSampler::greedy()
        } else {
            LlamaSampler::chain_simple([LlamaSampler::top_p(0.9, 1), LlamaSampler::temp(opts.temperature), LlamaSampler::dist(42)])
        };

        let mut out = String::new();
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut batch = LlamaBatch::new(1, 1);
        for _ in 0..max_new {
            let token = sampler.sample(&ctx, -1);
            sampler.accept(token);
            if self.model.is_eog_token(token) {
                break;
            }
            let bytes = self.model.token_to_piece_bytes(token, 512, false, None).map_err(|e| LlmError::Engine(e.to_string()))?;
            let mut piece = String::with_capacity(bytes.len() + 4);
            let _ = decoder.decode_to_string(&bytes, &mut piece, false);
            if !piece.is_empty() {
                out.push_str(&piece);
                if !on_token(&piece) {
                    return Err(LlmError::Cancelled);
                }
            }
            batch.clear();
            batch.add(token, pos, &[0], true).map_err(|e| LlmError::Engine(e.to_string()))?;
            pos += 1;
            if pos as u32 >= n_ctx - 1 {
                break;
            }
            ctx.decode(&mut batch).map_err(|e| LlmError::Engine(e.to_string()))?;
        }
        Ok(strip_think(&out))
    }

    fn name(&self) -> String {
        self.name.clone()
    }
}

/// Remove a `<think>...</think>` block some models emit.
pub fn strip_think(s: &str) -> String {
    match (s.find("<think>"), s.find("</think>")) {
        (Some(a), Some(b)) if b > a => format!("{}{}", &s[..a], &s[b + 8..]).trim().to_string(),
        _ => s.trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_think_removes_block() {
        assert_eq!(strip_think("<think>\nhmm\n</think>\n\nPONG"), "PONG");
        assert_eq!(strip_think("PONG"), "PONG");
    }

    #[test]
    #[ignore = "needs a GGUF; set HARK_TEST_LLM"]
    fn generates_pong() {
        let p = std::env::var("HARK_TEST_LLM").expect("HARK_TEST_LLM");
        let e = LlamaEngine::load(Path::new(&p), 999, 4096).unwrap();
        let out = crate::backend::complete(
            &e,
            &[ChatMessage::system("You are terse."), ChatMessage::user("Reply with the single word PONG.")],
            &GenOptions { max_tokens: 16, temperature: 0.0, json: false },
        )
        .unwrap();
        println!("reply: {out:?}");
        assert!(out.to_uppercase().contains("PONG"));
    }
}
