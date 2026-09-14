//! "Ask Hark": build a grounded prompt from retrieved transcript chunks and
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

pub const CHAT_SYSTEM: &str = "You are Hark, a private assistant that answers questions about the user's recorded meetings.\n\
Answer only from the transcript passages provided. If the passages do not contain the answer, say so plainly and do not guess.\n\
Be concise and direct. When you rely on a passage, cite it with its timestamp in square brackets exactly as shown in the passage header, e.g. [12:34]. \
When passages come from several meetings, cite as [12:34 @ Meeting title]. Do not invent timestamps.";

/// Assemble system + context + prior turns + question.
pub fn build_messages(question: &str, history: &[(Role, String)], passages: &[Passage], multi_meeting: bool) -> Vec<ChatMessage> {
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
    let mut msgs = vec![ChatMessage::system(format!("{CHAT_SYSTEM}\n\nTranscript passages:\n{context}"))];
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
