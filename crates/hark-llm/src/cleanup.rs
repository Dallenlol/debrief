//! The transcript cleanup pass: chunk -> prompt -> parse, tolerant of sloppy model output.

use crate::backend::{complete, ChatMessage, GenOptions, LlmBackend, LlmError};
use crate::prompts::{cleanup_line, CLEANUP_SYSTEM};

/// `(segment_id, speaker, raw_text)`
pub type RawSegment = (i64, Option<String>, String);

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    /// `(segment_id, speaker)` for every line in `text`, in order.
    pub items: Vec<(i64, Option<String>)>,
    pub text: String,
}

impl Chunk {
    pub fn segment_ids(&self) -> Vec<i64> {
        self.items.iter().map(|(id, _)| *id).collect()
    }
}

/// Group consecutive segments into prompts of roughly `max_chars`, never splitting a segment.
pub fn chunk_segments(segs: &[RawSegment], max_chars: usize) -> Vec<Chunk> {
    let mut out = Vec::new();
    let mut items = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    let mut size = 0usize;
    for (id, speaker, text) in segs {
        let line = cleanup_line(*id, speaker.as_deref(), text);
        if !lines.is_empty() && size + line.len() + 1 > max_chars {
            out.push(Chunk { items: std::mem::take(&mut items), text: lines.join("\n") });
            lines.clear();
            size = 0;
        }
        size += line.len() + 1;
        items.push((*id, speaker.clone()));
        lines.push(line);
    }
    if !lines.is_empty() {
        out.push(Chunk { items, text: lines.join("\n") });
    }
    out
}

/// Parse `[id] text` lines. Unknown ids, empty text and any prose the model
/// added are ignored. If the model echoed the known speaker name as a
/// "Name:" prefix, it is stripped.
pub fn parse_cleaned(output: &str, expected: &[(i64, Option<String>)]) -> Vec<(i64, String)> {
    let mut out = Vec::new();
    for raw in output.lines() {
        let line = raw.trim().trim_start_matches(['-', '*', ' ']).trim();
        let Some(rest) = line.strip_prefix('[') else { continue };
        let Some(close) = rest.find(']') else { continue };
        let Ok(id) = rest[..close].trim().parse::<i64>() else { continue };
        let Some((_, speaker)) = expected.iter().find(|(i, _)| *i == id) else { continue };
        let mut text = rest[close + 1..].trim().to_string();
        if let Some(sp) = speaker {
            let prefix = format!("{sp}:");
            if text.len() >= prefix.len() && text[..prefix.len()].eq_ignore_ascii_case(&prefix) {
                text = text[prefix.len()..].trim().to_string();
            }
        }
        if text.is_empty() {
            continue;
        }
        if let Some(existing) = out.iter_mut().find(|(i, _)| *i == id) {
            *existing = (id, text);
        } else {
            out.push((id, text));
        }
    }
    out
}

/// Run the cleanup over all segments. `progress` gets 0..1; `cancel` is polled between chunks.
pub fn run(
    backend: &dyn LlmBackend,
    segs: &[RawSegment],
    progress: impl Fn(f32),
    cancel: impl Fn() -> bool,
) -> Result<Vec<(i64, String)>, LlmError> {
    let chunks = chunk_segments(segs, 6000);
    let mut out = Vec::with_capacity(segs.len());
    for (i, chunk) in chunks.iter().enumerate() {
        if cancel() {
            return Err(LlmError::Cancelled);
        }
        let msgs = [ChatMessage::system(CLEANUP_SYSTEM), ChatMessage::user(chunk.text.clone())];
        let opts = GenOptions { max_tokens: (chunk.text.len() / 2).clamp(256, 4096), temperature: 0.0, json: false };
        match complete(backend, &msgs, &opts) {
            Ok(reply) => out.extend(parse_cleaned(&reply, &chunk.items)),
            Err(LlmError::Cancelled) => return Err(LlmError::Cancelled),
            Err(e) => tracing::warn!("cleanup chunk {i} failed: {e}"),
        }
        progress((i + 1) as f32 / chunks.len() as f32);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Mock;
    impl LlmBackend for Mock {
        fn chat(&self, msgs: &[ChatMessage], _: &GenOptions, on_token: &mut dyn FnMut(&str) -> bool) -> Result<String, LlmError> {
            // Echo the user lines with "um " removed and a bit of prose around them.
            let user = &msgs.last().unwrap().content;
            let mut s = String::from("Sure! Here is the cleaned transcript:\n");
            for l in user.lines() {
                s.push_str(&l.replace("um ", ""));
                s.push('\n');
            }
            s.push_str("Let me know if you need anything else.");
            on_token(&s);
            Ok(s)
        }
        fn name(&self) -> String {
            "mock".into()
        }
    }

    fn seg(id: i64, sp: &str, t: &str) -> RawSegment {
        (id, Some(sp.into()), t.into())
    }

    #[test]
    fn chunks_respect_limit_and_never_split() {
        let segs: Vec<RawSegment> = (0..10).map(|i| seg(i, "A", &"x".repeat(50))).collect();
        let chunks = chunk_segments(&segs, 130);
        assert!(chunks.len() > 1);
        assert_eq!(chunks.iter().map(|c| c.items.len()).sum::<usize>(), 10);
        assert!(chunks.iter().all(|c| c.text.len() <= 130 || c.items.len() == 1));
        let one = chunk_segments(&[seg(7, "A", &"y".repeat(500))], 100);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].segment_ids(), vec![7]);
    }

    #[test]
    fn parse_is_tolerant() {
        let out = "Here you go:\n[1] Sarah: Hello there.\n- [2] Bob: How are you?\n[99] ignored\n[3]\n[2] Bob: How are you doing?\nThanks!";
        let expected = vec![(1, Some("Sarah".to_string())), (2, Some("Bob".to_string())), (3, None)];
        let parsed = parse_cleaned(out, &expected);
        assert_eq!(parsed, vec![(1, "Hello there.".to_string()), (2, "How are you doing?".to_string())]);
        // A colon inside real text is kept; only the known speaker prefix is stripped.
        let p = parse_cleaned("[5] Note: the budget is 5k.", &[(5, Some("Sam".into()))]);
        assert_eq!(p[0].1, "Note: the budget is 5k.".to_string());
        let p = parse_cleaned("[6] sam: We said it clearly: go.", &[(6, Some("Sam".into()))]);
        assert_eq!(p[0].1, "We said it clearly: go.");
    }

    #[test]
    fn run_end_to_end_with_mock() {
        let segs = vec![seg(1, "Me", "um so um the plan is"), seg(2, "Sarah", "yes um agreed")];
        let progress = std::cell::RefCell::new(Vec::new());
        let out = run(&Mock, &segs, |p| progress.borrow_mut().push(p), || false).unwrap();
        assert_eq!(out, vec![(1, "so the plan is".to_string()), (2, "yes agreed".to_string())]);
        assert_eq!(progress.borrow().last().copied(), Some(1.0));
        assert!(matches!(run(&Mock, &segs, |_| {}, || true), Err(LlmError::Cancelled)));
    }
}
