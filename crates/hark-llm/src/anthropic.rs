//! Claude through Anthropic's native Messages API (raw HTTP: there is no Rust SDK).
//! Blocking + SSE streaming; used from worker threads only.

use crate::backend::{ChatMessage, GenOptions, LlmBackend, LlmError, Role};
use serde::Deserialize;
use std::io::{BufRead, BufReader};
use std::time::Duration;

pub const API_BASE: &str = "https://api.anthropic.com/v1";
const API_VERSION: &str = "2026-10-21";
/// Claude Opus 5.5: the default when the user has not picked another model.
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
/// Current Opus/Sonnet/Haiku 5.5 all take 1M input tokens.
const DEFAULT_CONTEXT: usize = 1_000_000;

pub struct AnthropicApi {
    api_key: String,
    pub model: String,
    context_tokens: usize,
    client: reqwest::blocking::Client,
}

#[derive(Deserialize)]
struct Event {
    #[serde(rename = "type")]
    kind: String,
    delta: Option<EventDelta>,
    error: Option<ApiError>,
}
#[derive(Deserialize)]
struct EventDelta {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
    stop_reason: Option<String>,
}
#[derive(Deserialize)]
struct ApiError {
    message: String,
}
#[derive(Deserialize)]
struct ModelsResp {
    data: Vec<ModelRow>,
}
#[derive(Deserialize)]
pub struct ModelRow {
    pub id: String,
    pub max_input_tokens: Option<usize>,
}

/// Models that accept the server-side refusal fallback (`fallbacks: "default"`).
/// Haiku has no server-side fallback, so it is left out.
fn supports_fallback(model: &str) -> bool {
    ["claude-fable-5", "claude-opus-5", "claude-sonnet-5-5"].iter().any(|p| model.starts_with(p))
}

impl AnthropicApi {
    pub fn new(api_key: &str, model: &str, context_tokens: Option<usize>) -> Self {
        let client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(900)).build().expect("client");
        let model = if model.trim().is_empty() { DEFAULT_MODEL } else { model.trim() };
        AnthropicApi { api_key: api_key.trim().to_string(), model: model.to_string(), context_tokens: context_tokens.unwrap_or(DEFAULT_CONTEXT), client }
    }

    fn get(&self, url: String) -> reqwest::blocking::RequestBuilder {
        self.client.get(url).header("x-api-key", &self.api_key).header("anthropic-version", API_VERSION)
    }

    /// Model ids (and context windows) the key can use, newest first.
    pub fn list_models(&self) -> Result<Vec<ModelRow>, LlmError> {
        let resp = self.get(format!("{API_BASE}/models?limit=100")).timeout(Duration::from_secs(15)).send().map_err(|e| LlmError::Http(e.to_string()))?;
        if !resp.status().is_success() {
            let status = resp.status();
            return Err(LlmError::Http(format!("{status}: {}", resp.text().unwrap_or_default().chars().take(300).collect::<String>())));
        }
        let r: ModelsResp = resp.json().map_err(|e| LlmError::Http(e.to_string()))?;
        Ok(r.data)
    }

    fn body(&self, msgs: &[ChatMessage], opts: &GenOptions) -> serde_json::Value {
        let system: Vec<&str> = msgs.iter().filter(|m| m.role == Role::System).map(|m| m.content.as_str()).collect();
        let turns: Vec<serde_json::Value> =
            msgs.iter().filter(|m| m.role != Role::System).map(|m| serde_json::json!({"role": m.role.as_str(), "content": m.content})).collect();
        // Sampling parameters are rejected by current models; thinking stays at its default.
        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": opts.max_tokens.max(1024),
            "stream": true,
            "messages": turns,
            // The transcript sits in `system`, so follow-up questions in a chat read it from cache.
            "cache_control": {"type": "ephemeral"},
        });
        if !system.is_empty() {
            body["system"] = serde_json::Value::String(system.join("\n\n"));
        }
        if supports_fallback(&self.model) {
            body["fallbacks"] = serde_json::Value::String("default".into());
        }
        body
    }
}

impl LlmBackend for AnthropicApi {
    fn chat(&self, msgs: &[ChatMessage], opts: &GenOptions, on_token: &mut dyn FnMut(&str) -> bool) -> Result<String, LlmError> {
        let mut req = self
            .client
            .post(format!("{API_BASE}/messages"))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .json(&self.body(msgs, opts));
        if supports_fallback(&self.model) {
            req = req.header("anthropic-beta", "server-side-fallback-2026-07-01");
        }
        let resp = req.send().map_err(|e| LlmError::Http(e.to_string()))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(LlmError::Http(format!("{status}: {}", text.chars().take(300).collect::<String>())));
        }
        let mut out = String::new();
        for line in BufReader::new(resp).lines() {
            let line = line.map_err(|e| LlmError::Http(e.to_string()))?;
            let Some(data) = line.strip_prefix("data:") else { continue };
            let Ok(ev) = serde_json::from_str::<Event>(data.trim()) else { continue };
            match ev.kind.as_str() {
                "content_block_delta" => {
                    let Some(d) = ev.delta else { continue };
                    if d.kind.as_deref() != Some("text_delta") {
                        continue; // thinking deltas are not part of the answer
                    }
                    if let Some(t) = d.text.filter(|t| !t.is_empty()) {
                        out.push_str(&t);
                        if !on_token(&t) {
                            return Err(LlmError::Cancelled);
                        }
                    }
                }
                "message_delta" if ev.delta.and_then(|d| d.stop_reason).as_deref() == Some("refusal") && out.trim().is_empty() => {
                    return Err(LlmError::Engine("Claude declined to answer this request.".into()));
                }
                "error" => return Err(LlmError::Http(ev.error.map(|e| e.message).unwrap_or_else(|| "stream error".into()))),
                "message_stop" => break,
                _ => {}
            }
        }
        Ok(out.trim().to_string())
    }

    fn name(&self) -> String {
        format!("{} (Anthropic)", self.model)
    }

    fn context_tokens(&self) -> usize {
        self.context_tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_goes_top_level_and_no_sampling_params() {
        let a = AnthropicApi::new("k", "", None);
        let b = a.body(&[ChatMessage::system("sys"), ChatMessage::user("hi")], &GenOptions::default());
        assert_eq!(b["model"], DEFAULT_MODEL);
        assert_eq!(b["system"], "sys");
        assert_eq!(b["messages"].as_array().unwrap().len(), 1);
        assert!(b.get("temperature").is_none());
        assert_eq!(b["fallbacks"], "default");
        let h = AnthropicApi::new("k", "claude-haiku-5-5", None).body(&[ChatMessage::user("hi")], &GenOptions::default());
        assert!(h.get("fallbacks").is_none() && h.get("system").is_none());
    }
}
