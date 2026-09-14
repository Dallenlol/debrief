//! Auto-title: a short, specific name for a meeting from the start of its transcript.

use crate::backend::{complete, ChatMessage, GenOptions, LlmBackend, LlmError};

pub const TITLE_SYSTEM: &str = "You name meetings. Given the opening of a transcript, reply with one specific title of at most 8 words: \
what the meeting was about, in the language of the transcript. No quotes, no trailing period, no preamble, no explanation.";

pub const TITLE_HEAD_CHARS: usize = 6_000;
pub const MAX_TITLE_CHARS: usize = 80;

/// Ask the model for a title. Returns `None` when the model produced nothing usable.
pub fn generate(backend: &dyn LlmBackend, transcript: &str, app_label: Option<&str>) -> Result<Option<String>, LlmError> {
    let head: String = transcript.chars().take(TITLE_HEAD_CHARS).collect();
    if head.trim().len() < 40 {
        return Ok(None);
    }
    let hint = app_label.map(|a| format!(" (recorded in {a})")).unwrap_or_default();
    let user = format!("Transcript opening{hint}:\n\n{head}\n\nTitle:");
    let msgs = [ChatMessage::system(TITLE_SYSTEM), ChatMessage::user(user)];
    let reply = complete(backend, &msgs, &GenOptions { max_tokens: 40, temperature: 0.2, json: false })?;
    Ok(sanitize_title(&reply))
}

/// Strip quotes, "Title:" prefixes, markdown and trailing punctuation; cap the length.
pub fn sanitize_title(raw: &str) -> Option<String> {
    let first = raw.lines().map(str::trim).find(|l| !l.is_empty())?;
    let mut t = first.trim_start_matches(['#', '*', '-']).trim();
    for prefix in ["Title:", "title:", "Meeting title:", "Meeting:"] {
        if let Some(rest) = t.strip_prefix(prefix) {
            t = rest.trim();
        }
    }
    let t = t.trim_matches(|c| matches!(c, '"' | '\'' | '“' | '”' | '‘' | '’' | '*' | '`')).trim_end_matches(['.', '!', ':']).trim();
    if t.is_empty() || t.eq_ignore_ascii_case("untitled") || t.eq_ignore_ascii_case("meeting") {
        return None;
    }
    let mut out: String = t.chars().take(MAX_TITLE_CHARS).collect();
    if t.chars().count() > MAX_TITLE_CHARS {
        if let Some(sp) = out.rfind(' ') {
            out.truncate(sp);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_model_output() {
        assert_eq!(sanitize_title("\"Q3 pricing review with Acme.\"").as_deref(), Some("Q3 pricing review with Acme"));
        assert_eq!(sanitize_title("Title: **Hiring plan sync**\nextra").as_deref(), Some("Hiring plan sync"));
        assert_eq!(sanitize_title("\n\n  Onboarding kickoff!  ").as_deref(), Some("Onboarding kickoff"));
        assert_eq!(sanitize_title(""), None);
        assert_eq!(sanitize_title("Untitled"), None);
        let long = sanitize_title(&"word ".repeat(40)).unwrap();
        assert!(long.chars().count() <= MAX_TITLE_CHARS);
        assert!(!long.ends_with(' '));
    }

    struct Fake;
    impl LlmBackend for Fake {
        fn chat(&self, msgs: &[ChatMessage], _: &GenOptions, _: &mut dyn FnMut(&str) -> bool) -> Result<String, LlmError> {
            assert!(msgs[1].content.contains("recorded in Zoom"));
            Ok("'Website redesign scope'".into())
        }
        fn name(&self) -> String {
            "fake".into()
        }
    }

    #[test]
    fn generates_and_skips_short_transcripts() {
        assert_eq!(generate(&Fake, "hi", Some("Zoom")).unwrap(), None);
        let t = "[0:00] Me: so today we want to lock the scope for the website redesign and the timeline for it".repeat(2);
        assert_eq!(generate(&Fake, &t, Some("Zoom")).unwrap().as_deref(), Some("Website redesign scope"));
    }
}
