//! Follow-up email drafted from a meeting's notes (or its transcript when there are none).

use crate::backend::{complete, ChatMessage, GenOptions, LlmBackend, LlmError};

pub const FOLLOWUP_SYSTEM: &str = "You write short follow-up emails after meetings. Using only the notes you are given, write a plain-text email \
with: a one-line greeting, a one or two sentence recap, the decisions, the action items with who owns each, the next step, and a sign-off. \
Keep it under 200 words, friendly and direct. Write in the same language as the notes. No subject line, no placeholders like [Name], \
no markdown, no preamble or commentary - output only the email body.";

/// Transcript characters used when a meeting has no summary yet.
pub const TRANSCRIPT_FALLBACK_CHARS: usize = 12_000;

pub struct FollowupInput<'a> {
    pub title: &'a str,
    pub date: &'a str,
    pub participants: &'a str,
    /// Summary markdown when available, otherwise the (truncated) transcript.
    pub notes: &'a str,
    pub notes_are_transcript: bool,
    /// The user's name for the sign-off; empty means "let the model pick a neutral sign-off".
    pub sender: &'a str,
}

pub fn build_prompt(input: &FollowupInput) -> Vec<ChatMessage> {
    let kind = if input.notes_are_transcript { "Transcript (may be cut short)" } else { "Meeting notes" };
    let signoff = if input.sender.trim().is_empty() {
        "\nNo sender name is known: end with a plain closing line such as \"Thanks,\" and nothing after it.".to_string()
    } else {
        format!("\nSign the email as {}.", input.sender.trim())
    };
    let user = format!(
        "Meeting: {}\nDate: {}\nParticipants: {}{signoff}\n\n{kind}:\n{}\n\nWrite the follow-up email now.",
        input.title, input.date, input.participants, input.notes
    );
    vec![ChatMessage::system(FOLLOWUP_SYSTEM), ChatMessage::user(user)]
}

pub fn generate(backend: &dyn LlmBackend, input: &FollowupInput) -> Result<String, LlmError> {
    let reply = complete(backend, &build_prompt(input), &GenOptions { max_tokens: 700, temperature: 0.3, json: false })?;
    Ok(strip_wrapping(&reply))
}

/// Drop a leading "Subject:" line or "Here is..." preamble and surrounding code fences.
pub fn strip_wrapping(raw: &str) -> String {
    let mut lines: Vec<&str> = raw.trim().lines().collect();
    if lines.first().map(|l| l.trim_start().starts_with("```")).unwrap_or(false) {
        lines.remove(0);
    }
    if lines.last().map(|l| l.trim().starts_with("```")).unwrap_or(false) {
        lines.pop();
    }
    while let Some(first) = lines.first() {
        let t = first.trim();
        let preamble = t.is_empty() || t.starts_with("Subject:") || t.starts_with("Here is") || t.starts_with("Here's") || t.ends_with("email:");
        if preamble {
            lines.remove(0);
        } else {
            break;
        }
    }
    lines.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_subject_and_fences() {
        assert_eq!(strip_wrapping("```\nSubject: Recap\n\nHi team,\nThanks.\n```"), "Hi team,\nThanks.");
        assert_eq!(strip_wrapping("Here is the email:\n\nHi Sam,\nBye"), "Hi Sam,\nBye");
        assert_eq!(strip_wrapping("Hi all,\nok"), "Hi all,\nok");
    }

    #[test]
    fn prompt_carries_notes_and_sender() {
        let msgs = build_prompt(&FollowupInput { title: "Sync", date: "Sep 15", participants: "Sam, Priya", notes: "## Decisions\n- ship", notes_are_transcript: false, sender: "Alex" });
        assert_eq!(msgs.len(), 2);
        assert!(msgs[1].content.contains("Sign the email as Alex."));
        assert!(msgs[1].content.contains("Meeting notes:\n## Decisions"));
        let t = build_prompt(&FollowupInput { title: "", date: "", participants: "", notes: "x", notes_are_transcript: true, sender: "" });
        assert!(t[1].content.contains("Transcript"));
        assert!(!t[1].content.contains("Sign the email"));
    }
}
