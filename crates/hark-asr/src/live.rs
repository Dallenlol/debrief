//! Live captions: a growing buffer of the current utterance is re-transcribed
//! every couple of seconds (the line refines in place), then committed when
//! the speaker pauses or the buffer gets long. The committed text is fed back
//! as whisper's prompt so names and phrasing stay consistent, near-silent
//! audio is never transcribed (whisper invents text on silence), and the
//! language is locked after the first confident detection.

use crate::whisper::{Caption, TranscribeOpts, WhisperEngine};
use crossbeam_channel::{Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;

pub const HZ: usize = 16_000;
/// Re-transcribe after this much new audio.
pub const STEP_S: f32 = 2.0;
/// Commit no matter what once the buffer is this long.
pub const MAX_S: f32 = 15.0;
/// A pause this long at the end of the buffer ends the utterance.
pub const PAUSE_S: f32 = 0.8;
/// Below this (dBFS) a window is treated as silence.
pub const GATE_DB: f32 = -50.0;
/// Committed text carried into the next window as the prompt.
pub const PROMPT_CHARS: usize = 220;
/// Segments whisper itself thinks are probably not speech.
pub const NO_SPEECH_MAX: f32 = 0.6;

pub struct LiveTranscriber {
    handle: Option<JoinHandle<()>>,
}

impl LiveTranscriber {
    /// Consumes `rx` until it is disconnected, sending captions on `tx`.
    /// The final partial window is flushed when the input closes.
    pub fn start(engine: Arc<WhisperEngine>, rx: Receiver<Vec<f32>>, tx: Sender<Caption>, lang: Option<String>) -> Self {
        let handle = std::thread::Builder::new()
            .name("hark-live-asr".into())
            .spawn(move || {
                let mut st = LiveState::new(lang);
                let mut since_step = 0usize;
                let step = (STEP_S * HZ as f32) as usize;
                for pcm in rx.iter() {
                    // Drain everything queued so we never fall further behind than one step.
                    let mut merged = pcm;
                    while let Ok(more) = rx.try_recv() {
                        merged.extend(more);
                    }
                    since_step += merged.len();
                    st.buf.extend(merged);
                    if since_step < step && st.buf.len() < (MAX_S * HZ as f32) as usize {
                        continue;
                    }
                    since_step = 0;
                    if run_pass(&engine, &mut st, false, &tx).is_err() {
                        return;
                    }
                }
                let _ = run_pass(&engine, &mut st, true, &tx);
            })
            .expect("spawn live asr thread");
        LiveTranscriber { handle: Some(handle) }
    }

    /// Wait for the thread to finish (drop the input sender first).
    pub fn join(mut self) {
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Everything the loop carries between passes. Pure apart from the engine call.
pub struct LiveState {
    pub buf: Vec<f32>,
    /// Absolute position (samples) of `buf[0]`.
    pub base: usize,
    pub prompt: String,
    /// Configured language, or the one locked after the first detection.
    pub lang: Option<String>,
    lang_locked: bool,
}

impl LiveState {
    pub fn new(lang: Option<String>) -> Self {
        LiveState { buf: Vec::with_capacity(HZ * 20), base: 0, prompt: String::new(), lang_locked: lang.is_some(), lang }
    }

    fn drop_buffer(&mut self) {
        self.base += self.buf.len();
        self.buf.clear();
    }
}

/// What to do with the current buffer, decided from its audio alone.
#[derive(Debug, PartialEq, Eq)]
pub enum Plan {
    /// Nothing worth sending to the model.
    Silence,
    /// Transcribe and show as provisional.
    Partial,
    /// Transcribe and commit (pause detected or buffer full).
    Commit,
}

pub fn plan(buf: &[f32], flush: bool) -> Plan {
    if buf.len() < HZ / 2 {
        return Plan::Silence;
    }
    if rms_db(buf) < GATE_DB {
        return Plan::Silence;
    }
    if flush || buf.len() >= (MAX_S * HZ as f32) as usize {
        return Plan::Commit;
    }
    let tail = (PAUSE_S * HZ as f32) as usize;
    if buf.len() > tail + HZ && rms_db(&buf[buf.len() - tail..]) < GATE_DB {
        return Plan::Commit;
    }
    Plan::Partial
}

fn run_pass(engine: &WhisperEngine, st: &mut LiveState, flush: bool, tx: &Sender<Caption>) -> Result<(), ()> {
    match plan(&st.buf, flush) {
        Plan::Silence => {
            // Long stretches of nothing are discarded so the next utterance starts fresh.
            if st.buf.len() >= (3.0 * HZ as f32) as usize || flush {
                st.drop_buffer();
            }
            Ok(())
        }
        p => {
            let commit = p == Plan::Commit;
            let offset_ms = (st.base as i64) * 1000 / HZ as i64;
            let opts = TranscribeOpts {
                offset_ms,
                lang: st.lang.as_deref(),
                is_final: commit,
                max_len: 0,
                prompt: Some(&st.prompt),
                drop_no_speech_above: Some(NO_SPEECH_MAX),
            };
            let t = match engine.transcribe_opts(&st.buf, &opts) {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!("live asr: {e}");
                    if commit {
                        st.drop_buffer();
                    }
                    return Ok(());
                }
            };
            if !st.lang_locked {
                if let Some(l) = t.lang.clone() {
                    st.lang = Some(l);
                    st.lang_locked = true;
                }
            }
            let caps: Vec<Caption> = t.captions.into_iter().filter(|c| !is_hallucination(&c.text)).collect();
            let end_ms = offset_ms + (st.buf.len() as i64) * 1000 / HZ as i64;
            if commit {
                for c in &caps {
                    if tx.send(c.clone()).is_err() {
                        return Err(());
                    }
                }
                let text: Vec<&str> = caps.iter().map(|c| c.text.as_str()).collect();
                st.prompt = tail_chars(&format!("{} {}", st.prompt, text.join(" ")), PROMPT_CHARS);
                st.drop_buffer();
            } else if !caps.is_empty() {
                // One provisional line for the whole utterance so the UI can replace it in place.
                let text: Vec<&str> = caps.iter().map(|c| c.text.as_str()).collect();
                let c = Caption { start_ms: caps[0].start_ms, end_ms, text: text.join(" "), is_final: false };
                if tx.send(c).is_err() {
                    return Err(());
                }
            }
            Ok(())
        }
    }
}

fn rms_db(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return -100.0;
    }
    let mean_sq = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
    if mean_sq <= 1e-10 {
        return -100.0;
    }
    (20.0 * mean_sq.sqrt().log10()).max(-100.0)
}

fn tail_chars(s: &str, n: usize) -> String {
    let s = s.trim();
    let count = s.chars().count();
    if count <= n {
        return s.to_string();
    }
    let mut chars = s.chars().skip(count - n - 1);
    let before = chars.next().unwrap_or(' ');
    let cut: String = chars.collect();
    // Start at a word boundary: if the cut landed mid-word, drop that partial word.
    if before.is_whitespace() {
        return cut;
    }
    match cut.find(' ') {
        Some(i) => cut[i + 1..].to_string(),
        None => cut,
    }
}

/// Phrases whisper produces from silence, music or applause rather than speech.
pub fn is_hallucination(text: &str) -> bool {
    let t = text.trim().trim_matches(|c: char| c.is_ascii_punctuation()).to_lowercase();
    if t.is_empty() {
        return true;
    }
    const KNOWN: &[&str] = &[
        "thank you",
        "thanks",
        "thank you for watching",
        "thanks for watching",
        "thank you so much for watching",
        "please subscribe",
        "subscribe to my channel",
        "like and subscribe",
        "see you in the next video",
        "see you next time",
        "bye",
        "you",
        "the end",
        "subtitles by the amara.org community",
        "subtitles by",
        "transcribed by",
        "copyright",
        "www.mooji.org",
    ];
    if KNOWN.contains(&t.as_str()) {
        return true;
    }
    // "you you you you" style repetition.
    let words: Vec<&str> = t.split_whitespace().collect();
    if words.len() >= 4 && words.iter().all(|w| *w == words[0]) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(secs: f32, amp: f32) -> Vec<f32> {
        (0..(secs * HZ as f32) as usize).map(|i| (i as f32 * 0.05).sin() * amp).collect()
    }

    #[test]
    fn silence_is_never_transcribed() {
        assert_eq!(plan(&vec![0.0; HZ * 4], false), Plan::Silence);
        assert_eq!(plan(&tone(4.0, 0.0005), false), Plan::Silence); // -66 dBFS
        assert_eq!(plan(&tone(0.2, 0.5), false), Plan::Silence); // too short
    }

    #[test]
    fn speech_is_partial_until_a_pause_or_the_cap() {
        assert_eq!(plan(&tone(4.0, 0.3), false), Plan::Partial);
        let mut paused = tone(4.0, 0.3);
        paused.extend(vec![0.0; HZ]); // 1 s of silence at the end
        assert_eq!(plan(&paused, false), Plan::Commit);
        assert_eq!(plan(&tone(MAX_S + 0.5, 0.3), false), Plan::Commit);
        assert_eq!(plan(&tone(2.0, 0.3), true), Plan::Commit); // flush at stop
    }

    #[test]
    fn hallucinations_are_dropped() {
        assert!(is_hallucination("Thank you."));
        assert!(is_hallucination(" thanks for watching! "));
        assert!(is_hallucination("you you you you you"));
        assert!(!is_hallucination("Thank you for the update on the budget."));
        assert!(!is_hallucination("we ship on the ninth"));
    }

    #[test]
    fn prompt_keeps_the_tail_on_a_word_boundary() {
        let s = "alpha beta gamma delta epsilon";
        assert_eq!(tail_chars(s, 100), s);
        assert_eq!(tail_chars(s, 13), "delta epsilon");
        assert_eq!(tail_chars(s, 15), "delta epsilon");
    }
}
