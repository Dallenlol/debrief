//! Long transcripts: condense each block into dense notes first, then let the
//! summary template run over the notes instead of a head+tail truncation.

use crate::backend::{complete, ChatMessage, GenOptions, LlmBackend, LlmError};
use crate::summary::{fmt_stamp, parse_stamp};

pub const CHUNK_NOTES_SYSTEM: &str = "You condense one part of a meeting transcript into dense notes for a later summary. \
Keep every decision, action item (with owner and deadline), number, name, date, objection and open question. \
Keep the [m:ss] timestamps of important moments. Output 8-25 terse bullet points, markdown only, no preamble. \
Write in the same language as the transcript.";

/// Split `text` on line boundaries into blocks of at most `max_chars`
/// (a single over-long line becomes its own block).
pub fn split_blocks(text: &str, max_chars: usize) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        if !cur.is_empty() && cur.len() + line.len() + 1 > max_chars {
            blocks.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push('\n');
        }
        cur.push_str(line);
    }
    if !cur.is_empty() {
        blocks.push(cur);
    }
    blocks
}

/// First and last `[m:ss]` stamp in a block, for the part header.
fn span(block: &str) -> (Option<i64>, Option<i64>) {
    let stamps: Vec<i64> = block
        .lines()
        .filter_map(|l| {
            let l = l.trim_start();
            let end = l.find(']')?;
            l.starts_with('[').then(|| parse_stamp(&l[1..end])).flatten()
        })
        .collect();
    (stamps.first().copied(), stamps.last().copied())
}

/// Ask the model for notes on each block. Returns the joined notes with
/// "Part N of M" headers. `on_progress(done, total)` after each block.
pub fn notes_for_blocks(
    backend: &dyn LlmBackend,
    transcript: &str,
    block_chars: usize,
    mut on_progress: impl FnMut(usize, usize),
) -> Result<String, LlmError> {
    let blocks = split_blocks(transcript, block_chars);
    let total = blocks.len();
    let mut out = String::new();
    for (i, block) in blocks.iter().enumerate() {
        let (a, b) = span(block);
        let range = match (a, b) {
            (Some(a), Some(b)) => format!(" ({}-{})", fmt_stamp(a), fmt_stamp(b)),
            _ => String::new(),
        };
        let user = format!("Part {} of {}{range}.\n\nTranscript:\n{}", i + 1, total, block);
        let msgs = [ChatMessage::system(CHUNK_NOTES_SYSTEM), ChatMessage::user(user)];
        let notes = complete(backend, &msgs, &GenOptions { max_tokens: 700, temperature: 0.1, json: false })?;
        out.push_str(&format!("### Part {} of {}{range}\n{}\n\n", i + 1, total, notes.trim()));
        on_progress(i + 1, total);
    }
    Ok(format!("[Condensed from {total} parts]\n\n{}", out.trim_end()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fake(AtomicUsize);
    impl LlmBackend for Fake {
        fn chat(&self, msgs: &[ChatMessage], _: &GenOptions, _: &mut dyn FnMut(&str) -> bool) -> Result<String, LlmError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(format!("- note from {}", msgs[1].content.lines().next().unwrap()))
        }
        fn name(&self) -> String {
            "fake".into()
        }
    }

    #[test]
    fn splits_on_lines_within_budget() {
        let text: String = (0..50).map(|i| format!("[{}] line {i}", fmt_stamp(i * 1000))).collect::<Vec<_>>().join("\n");
        let blocks = split_blocks(&text, 120);
        assert!(blocks.len() > 3);
        assert!(blocks.iter().all(|b| b.len() <= 120));
        assert!(blocks.iter().all(|b| b.lines().all(|l| l.starts_with('['))));
        assert_eq!(blocks.concat().replace('\n', ""), text.replace('\n', ""));
        assert_eq!(split_blocks("x".repeat(500).as_str(), 100).len(), 1);
    }

    #[test]
    fn notes_call_backend_once_per_block_with_ranges() {
        let text: String = (0..40).map(|i| format!("[{}] Speaker: words words words", fmt_stamp(i * 30_000))).collect::<Vec<_>>().join("\n");
        let fake = Fake(AtomicUsize::new(0));
        let mut seen = Vec::new();
        let notes = notes_for_blocks(&fake, &text, 300, |d, t| seen.push((d, t))).unwrap();
        let n = fake.0.load(Ordering::SeqCst);
        assert!(n >= 3);
        assert_eq!(seen.len(), n);
        assert_eq!(seen.last().unwrap(), &(n, n));
        assert!(notes.starts_with(&format!("[Condensed from {n} parts]")));
        assert!(notes.contains("### Part 1 of"));
        assert!(notes.contains("(0:00-"));
    }
}
