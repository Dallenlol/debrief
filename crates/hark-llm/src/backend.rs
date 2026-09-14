use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

impl ChatMessage {
    pub fn system(s: impl Into<String>) -> Self {
        ChatMessage { role: Role::System, content: s.into() }
    }
    pub fn user(s: impl Into<String>) -> Self {
        ChatMessage { role: Role::User, content: s.into() }
    }
    pub fn assistant(s: impl Into<String>) -> Self {
        ChatMessage { role: Role::Assistant, content: s.into() }
    }
}

#[derive(Debug, Clone)]
pub struct GenOptions {
    pub max_tokens: usize,
    pub temperature: f32,
    /// Ask for strict JSON output where the backend supports it.
    pub json: bool,
}

impl Default for GenOptions {
    fn default() -> Self {
        GenOptions { max_tokens: 1024, temperature: 0.2, json: false }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("model not found: {0}")]
    ModelNotFound(String),
    #[error("llm: {0}")]
    Engine(String),
    #[error("network: {0}")]
    Http(String),
    #[error("cancelled")]
    Cancelled,
}

/// A chat model. Implementations must be safe to call from any thread; calls
/// are serialised internally where the engine requires it.
pub trait LlmBackend: Send + Sync {
    /// Generate a reply. `on_token` receives text as it is produced and returns
    /// `false` to cancel. The full reply is returned on success.
    fn chat(&self, msgs: &[ChatMessage], opts: &GenOptions, on_token: &mut dyn FnMut(&str) -> bool) -> Result<String, LlmError>;
    fn name(&self) -> String;
}

/// Convenience: run to completion without streaming.
pub fn complete(b: &dyn LlmBackend, msgs: &[ChatMessage], opts: &GenOptions) -> Result<String, LlmError> {
    b.chat(msgs, opts, &mut |_| true)
}
