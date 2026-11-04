//! OpenAI-compatible `/v1/chat/completions` client (Ollama, LM Studio, vLLM...).
//! Blocking + SSE streaming; used from worker threads only.

use crate::backend::{ChatMessage, GenOptions, LlmBackend, LlmError};
use serde::Deserialize;
use std::io::{BufRead, BufReader};
use std::time::Duration;

pub struct OpenAiCompat {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    context_tokens: usize,
    client: reqwest::blocking::Client,
}

#[derive(Deserialize)]
struct Delta {
    content: Option<String>,
}
#[derive(Deserialize)]
struct Choice {
    delta: Option<Delta>,
    message: Option<Delta>,
}
#[derive(Deserialize)]
struct ChunkResp {
    choices: Vec<Choice>,
}
#[derive(Deserialize)]
struct ModelsResp {
    data: Vec<ModelRow>,
}
#[derive(Deserialize)]
struct ModelRow {
    id: String,
    /// OpenRouter reports each model's context window.
    context_length: Option<usize>,
}

impl OpenAiCompat {
    pub fn new(base_url: &str, api_key: Option<&str>, model: &str) -> Self {
        let client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(600)).build().expect("client");
        OpenAiCompat {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.map(str::to_string),
            model: model.to_string(),
            context_tokens: 8192,
            client,
        }
    }

    /// The model's context window; local servers (Ollama) default to 8k.
    pub fn with_context(mut self, tokens: usize) -> Self {
        self.context_tokens = tokens.max(2048);
        self
    }

    /// OpenAI's reasoning models (o-series, gpt-5) reject `max_tokens` and custom temperature.
    fn openai_reasoning(&self) -> bool {
        self.base_url.contains("api.openai.com") && (self.model.starts_with('o') || self.model.starts_with("gpt-5"))
    }

    /// List model ids the endpoint offers (used by the Settings "Test" button).
    pub fn list_models(&self) -> Result<Vec<String>, LlmError> {
        Ok(self.list_models_with_context()?.into_iter().map(|(id, _)| id).collect())
    }

    /// Model ids with their context window where the endpoint reports it.
    pub fn list_models_with_context(&self) -> Result<Vec<(String, Option<usize>)>, LlmError> {
        let mut req = self.client.get(format!("{}/models", self.base_url)).timeout(Duration::from_secs(10));
        if let Some(k) = &self.api_key {
            req = req.bearer_auth(k);
        }
        let resp: ModelsResp = req.send().map_err(|e| LlmError::Http(e.to_string()))?.error_for_status().map_err(|e| LlmError::Http(e.to_string()))?.json().map_err(|e| LlmError::Http(e.to_string()))?;
        Ok(resp.data.into_iter().map(|m| (m.id, m.context_length)).collect())
    }
}

impl LlmBackend for OpenAiCompat {
    fn chat(&self, msgs: &[ChatMessage], opts: &GenOptions, on_token: &mut dyn FnMut(&str) -> bool) -> Result<String, LlmError> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": msgs.iter().map(|m| serde_json::json!({"role": m.role.as_str(), "content": m.content})).collect::<Vec<_>>(),
            "stream": true,
        });
        if self.openai_reasoning() {
            body["max_completion_tokens"] = serde_json::json!(opts.max_tokens.max(4096));
        } else {
            body["temperature"] = serde_json::json!(opts.temperature);
            body["max_tokens"] = serde_json::json!(opts.max_tokens);
        }
        if opts.json {
            body["response_format"] = serde_json::json!({"type": "json_object"});
        }
        let mut req = self.client.post(format!("{}/chat/completions", self.base_url)).json(&body);
        if let Some(k) = &self.api_key {
            req = req.bearer_auth(k);
        }
        let resp = req.send().map_err(|e| LlmError::Http(e.to_string()))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(LlmError::Http(format!("{status}: {}", text.chars().take(300).collect::<String>())));
        }
        let mut out = String::new();
        let reader = BufReader::new(resp);
        for line in reader.lines() {
            let line = line.map_err(|e| LlmError::Http(e.to_string()))?;
            let Some(data) = line.strip_prefix("data:") else { continue };
            let data = data.trim();
            if data == "[DONE]" {
                break;
            }
            let Ok(chunk) = serde_json::from_str::<ChunkResp>(data) else { continue };
            for c in chunk.choices {
                let piece = c.delta.and_then(|d| d.content).or_else(|| c.message.and_then(|m| m.content));
                if let Some(p) = piece {
                    if p.is_empty() {
                        continue;
                    }
                    out.push_str(&p);
                    if !on_token(&p) {
                        return Err(LlmError::Cancelled);
                    }
                }
            }
        }
        Ok(crate::llama::strip_think(&out))
    }

    fn name(&self) -> String {
        format!("{} @ {}", self.model, self.base_url)
    }

    fn context_tokens(&self) -> usize {
        self.context_tokens
    }
}
