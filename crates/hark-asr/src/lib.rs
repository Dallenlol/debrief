//! Speech-to-text via whisper.cpp: batch transcription, live windowed
//! captions, and a helper to read WAV files into 16 kHz mono.

pub mod chunker;
pub mod live;
pub mod whisper;

pub use chunker::Chunker;
pub use live::LiveTranscriber;
pub use whisper::{AsrError, Caption, WhisperEngine};
