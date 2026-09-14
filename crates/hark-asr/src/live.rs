//! Background thread turning a 16 kHz PCM feed into live captions.

use crate::chunker::Chunker;
use crate::whisper::{Caption, WhisperEngine};
use crossbeam_channel::{Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

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
                let mut chunker = Chunker::new(16_000, Duration::from_secs(6), Duration::from_secs(1));
                let emit = |start_s: f64, pcm: Vec<f32>| {
                    match engine.transcribe(&pcm, (start_s * 1000.0) as i64, lang.as_deref(), false) {
                        Ok(caps) => {
                            for c in caps {
                                if tx.send(c).is_err() {
                                    return;
                                }
                            }
                        }
                        Err(e) => tracing::warn!("live asr: {e}"),
                    }
                };
                for pcm in rx.iter() {
                    // Drain everything queued so we never fall further behind than one window.
                    let mut merged = pcm;
                    while let Ok(more) = rx.try_recv() {
                        merged.extend(more);
                    }
                    if let Some((s, chunk)) = chunker.push(&merged) {
                        emit(s, chunk);
                    }
                }
                if let Some((s, chunk)) = chunker.flush() {
                    emit(s, chunk);
                }
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
