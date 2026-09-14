//! Local language models for Hark: a bundled llama.cpp engine or any
//! OpenAI-compatible endpoint (Ollama, LM Studio), plus the transcript
//! cleanup pass built on top.

pub mod backend;
pub mod chat;
pub mod chunked;
pub mod cleanup;
pub mod llama;
pub mod openai;
pub mod prompts;
pub mod summary;
pub mod title;

pub use backend::{ChatMessage, GenOptions, LlmBackend, LlmError, Role};
pub use llama::LlamaEngine;
pub use openai::OpenAiCompat;
