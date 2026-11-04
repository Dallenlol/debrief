//! "Ask Debrief": build a grounded prompt from retrieved transcript chunks and
//! parse the `[m:ss]` citations the model emits.

use crate::backend::{ChatMessage, Role};
use crate::summary::{fmt_stamp, parse_stamp};
use serde::{Deserialize, Serialize};

/// One retrieved passage handed to the model.
#[derive(Debug, Clone, PartialEq)]
pub struct Passage {
    pub meeting_id: String,
    pub meeting_title: String,
    pub meeting_date: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Citation {
    pub ms: i64,
    pub meeting_id: Option<String>,
    pub label: String,
}

pub const CHAT_SYSTEM: &str = "You are Debrief, a private assistant that answers questions about the user's recorded meetings.
Answer from the transcript material provided below. It is either the whole transcript, the most relevant excerpts, or notes extracted from every part of the meeting; the header says which. Use all of it: when asked for a list, collect every matching item from the whole material rather than stopping at the first few. If the material genuinely does not cover the question, say what it does cover instead of guessing.
Be direct, and answer in the language the question is asked in. When you rely on a passage, cite it with its timestamp in square brackets exactly as shown, e.g. [12:34]. When passages come from several meetings, cite as [12:34 @ Meeting title]. Do not invent timestamps.";

/// Instruction for one part of a long meeting when it does not fit the model at once.
pub const EXTRACT_SYSTEM: &str = "You read one part of a meeting transcript and pull out everything relevant to the user's question. Reply with a bullet list; start each bullet with the timestamp of the line it comes from, e.g. [12:34], and keep the speaker's meaning. Include every relevant item, even small ones. If nothing in this part is relevant, reply with exactly NONE.";

/// Questions about the meeting as a whole ("list every change", "summarise") need all of it,
/// not the few best-matching excerpts.
pub fn is_global(question: &str) -> bool {
    let q = question.to_lowercase();
    const CUES: [&str; 22] = [
        "every", "all ", "all the", "list", "each", "summar", "overall", "throughout", "whole", "entire", "how many", "what were",
        "what are the", "action item", "decision", "takeaway", "next step", "recap", "everything", "key point", "main point", "follow up",
    ];
    CUES.iter().any(|c| q.contains(c))
}

/// Messages for the map step over one block of transcript.
pub fn extract_messages(question: &str, block: &str) -> Vec<ChatMessage> {
    vec![ChatMessage::system(EXTRACT_SYSTEM), ChatMessage::user(format!("Question: {question}\n\nTranscript part:\n{block}"))]
}

/// Header line for the material block, so the model knows how complete it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Material {
    Full,
    Excerpts,
    Extracts,
}

/// Assemble system + context + prior turns + question.
pub fn build_messages(question: &str, history: &[(Role, String)], passages: &[Passage], multi_meeting: bool) -> Vec<ChatMessage> {
    build_messages_with(question, history, passages, multi_meeting, Material::Excerpts, None)
}

/// As [`build_messages`], saying how complete the material is and adding the meeting summary.
pub fn build_messages_with(
    question: &str,
    history: &[(Role, String)],
    passages: &[Passage],
    multi_meeting: bool,
    material: Material,
    summary: Option<&str>,
) -> Vec<ChatMessage> {
    let mut context = String::new();
    for (i, p) in passages.iter().enumerate() {
        let head = if multi_meeting {
            format!("[{} @ {}] ({}, {})", fmt_stamp(p.start_ms), p.meeting_title, p.meeting_date, p.meeting_id)
        } else {
            format!("[{}]", fmt_stamp(p.start_ms))
        };
        context.push_str(&format!("Passage {}: {head}\n{}\n\n", i + 1, p.text.trim()));
    }
    if context.is_empty() {
        context.push_str("(no matching passages were found)\n");
    }
    let head = match material {
        Material::Full => "The complete transcript, in order",
        Material::Excerpts => "Excerpts that best match the question (not the whole meeting)",
        Material::Extracts => "Notes extracted from every part of the meeting for this question",
    };
    let summary = summary.filter(|s| !s.trim().is_empty()).map(|s| format!("Meeting summary:\n{}\n\n", s.trim())).unwrap_or_default();
    let mut msgs = vec![ChatMessage::system(format!("{CHAT_SYSTEM}\n\n{summary}{head}:\n{context}"))];
    for (role, content) in history.iter().rev().take(8).rev() {
        msgs.push(ChatMessage { role: *role, content: content.clone() });
    }
    msgs.push(ChatMessage::user(question.to_string()));
    msgs
}

/// Find `[m:ss]`, `[h:mm:ss]` and `[m:ss @ Title]` citations. Titles are
/// resolved to meeting ids via `passages` when possible.
pub fn parse_citations(answer: &str, passages: &[Passage]) -> Vec<Citation> {
    let mut out: Vec<Citation> = Vec::new();
    let bytes = answer.as_bytes();
    let mut i = 0;
    while let Some(open) = answer[i..].find('[') {
        let start = i + open;
        let Some(close_rel) = answer[start..].find(']') else { break };
        let end = start + close_rel;
        let inner = &answer[start + 1..end];
        let (stamp, title) = match inner.split_once('@') {
            Some((s, t)) => (s.trim(), Some(t.trim())),
            None => (inner.trim(), None),
        };
        if let Some(ms) = parse_stamp(stamp) {
            let meeting_id = match title {
                Some(t) => passages
                    .iter()
                    .find(|p| p.meeting_title.eq_ignore_ascii_case(t) || t.starts_with(&p.meeting_id[..8.min(p.meeting_id.len())]))
                    .map(|p| p.meeting_id.clone()),
                None => {
                    // Single-meeting scope: the passage whose window contains the stamp.
                    passages.iter().find(|p| p.start_ms <= ms && ms <= p.end_ms + 60_000).map(|p| p.meeting_id.clone())
                }
            };
            let c = Citation { ms, meeting_id, label: inner.trim().to_string() };
            if !out.contains(&c) {
                out.push(c);
            }
        }
        i = end + 1;
        if i >= bytes.len() {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: &str, title: &str, start: i64) -> Passage {
        Passage { meeting_id: id.into(), meeting_title: title.into(), meeting_date: "Sep 1".into(), start_ms: start, end_ms: start + 45_000, text: "t".into() }
    }

    #[test]
    fn builds_grounded_prompt() {
        let msgs = build_messages("what?", &[(Role::User, "hi".into()), (Role::Assistant, "hello".into())], &[p("m1", "Budget", 65_000)], false);
        assert_eq!(msgs.len(), 4);
        assert!(msgs[0].content.contains("Passage 1: [1:05]"));
        assert_eq!(msgs[3].content, "what?");
        let multi = build_messages("q", &[], &[p("m1", "Budget", 0)], true);
        assert!(multi[0].content.contains("[0:00 @ Budget]"));
        let none = build_messages("q", &[], &[], false);
        assert!(none[0].content.contains("no matching passages"));
    }

    #[test]
    fn parses_citations_and_resolves_meetings() {
        let passages = [p("m1", "Budget review", 60_000), p("m2", "Hiring", 0)];
        let c = parse_citations("They agreed [1:05] and later [1:02:05 @ Hiring]. Not a cite [foo]. Again [1:05].", &passages);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0], Citation { ms: 65_000, meeting_id: Some("m1".into()), label: "1:05".into() });
        assert_eq!(c[1].ms, 3_725_000);
        assert_eq!(c[1].meeting_id.as_deref(), Some("m2"));
    }
}
