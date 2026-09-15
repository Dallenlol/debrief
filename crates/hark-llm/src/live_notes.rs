//! Running notes for a meeting that is still going: the model updates its
//! previous notes with the transcript that arrived since, so the notes evolve
//! instead of being rewritten from scratch.

use crate::backend::{complete, ChatMessage, GenOptions, LlmBackend, LlmError};

pub const LIVE_NOTES_SYSTEM: &str = "You keep running notes for a meeting that is still in progress. You get your previous notes and the \
transcript that arrived since. Return the updated notes, in this shape and nothing else:\n\
**Discussing now:** one sentence on the current topic.\n\
**So far:** 3-8 short bullets of what has been covered, in order, merged with the previous notes (rewrite, do not append).\n\
**Decisions:** bullets, or '- none yet'.\n\
**Action items:** bullets with owner when stated, or '- none yet'.\n\
Be terse, keep names, numbers and dates, never invent, and never mention that the transcript is live or partial. \
Write in the same language as the transcript.";

/// Transcript characters per update (the most recent text wins).
pub const MAX_NEW_CHARS: usize = 9_000;

pub fn build_prompt(previous: &str, new_transcript: &str) -> Vec<ChatMessage> {
    let prev = if previous.trim().is_empty() { "(none yet - the meeting just started)".to_string() } else { previous.trim().to_string() };
    let new_text: String = if new_transcript.chars().count() > MAX_NEW_CHARS {
        let skip = new_transcript.chars().count() - MAX_NEW_CHARS;
        new_transcript.chars().skip(skip).collect()
    } else {
        new_transcript.to_string()
    };
    let user = format!("Previous notes:\n{prev}\n\nNew transcript since then:\n{new_text}\n\nUpdated notes:");
    vec![ChatMessage::system(LIVE_NOTES_SYSTEM), ChatMessage::user(user)]
}

pub fn update(backend: &dyn LlmBackend, previous: &str, new_transcript: &str) -> Result<String, LlmError> {
    let reply = complete(backend, &build_prompt(previous, new_transcript), &GenOptions { max_tokens: 450, temperature: 0.1, json: false })?;
    Ok(reply.trim().trim_start_matches("Updated notes:").trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_carries_previous_notes_and_caps_new_text() {
        let m = build_prompt("", "hello");
        assert!(m[1].content.contains("(none yet"));
        let long = "x".repeat(MAX_NEW_CHARS + 50);
        let m = build_prompt("**So far:**\n- a", &long);
        assert!(m[1].content.contains("**So far:**"));
        assert!(m[1].content.len() < MAX_NEW_CHARS + 300);
    }
}
