//! sherpa-onnx wrappers. Models: pyannote segmentation 3.0 + a speaker
//! embedding model (NeMo TitaNet small by default).

use crate::Turn;
use sherpa_rs::diarize::{Diarize, DiarizeConfig};
use sherpa_rs::speaker_id::{EmbeddingExtractor, ExtractorConfig};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum DiarizeError {
    #[error("model not found: {0}")]
    ModelNotFound(String),
    #[error("diarization: {0}")]
    Engine(String),
}

pub struct DiarizeEngine {
    diarize: Diarize,
    extractor: EmbeddingExtractor,
    pub embedding_dim: usize,
}

// sherpa handles are plain pointers used from one thread at a time behind a Mutex upstream.
unsafe impl Send for DiarizeEngine {}

impl DiarizeEngine {
    pub fn load(seg_model: &Path, emb_model: &Path) -> Result<Self, DiarizeError> {
        for p in [seg_model, emb_model] {
            if !p.exists() {
                return Err(DiarizeError::ModelNotFound(p.display().to_string()));
            }
        }
        let cfg = DiarizeConfig {
            // Let clustering decide the speaker count from the threshold.
            num_clusters: Some(-1),
            threshold: Some(0.5),
            min_duration_on: Some(0.3),
            min_duration_off: Some(0.5),
            provider: None,
            debug: false,
        };
        let diarize = Diarize::new(seg_model, emb_model, cfg).map_err(|e| DiarizeError::Engine(e.to_string()))?;
        let extractor = EmbeddingExtractor::new(ExtractorConfig {
            model: emb_model.to_string_lossy().into_owned(),
            provider: None,
            num_threads: Some(2),
            debug: false,
        })
        .map_err(|e| DiarizeError::Engine(e.to_string()))?;
        let embedding_dim = extractor.embedding_size;
        Ok(DiarizeEngine { diarize, extractor, embedding_dim })
    }

    /// Speaker turns for 16 kHz mono audio, sorted by start time.
    pub fn diarize(&mut self, pcm16k: &[f32], progress: impl Fn(f32) + Send + 'static) -> Result<Vec<Turn>, DiarizeError> {
        if pcm16k.len() < 16_000 {
            return Ok(Vec::new());
        }
        let cb: Box<dyn Fn(i32, i32) -> i32 + Send + 'static> = Box::new(move |done, total| {
            progress(if total > 0 { done as f32 / total as f32 } else { 0.0 });
            0
        });
        let segs = self.diarize.compute(pcm16k.to_vec(), Some(cb)).map_err(|e| DiarizeError::Engine(e.to_string()))?;
        let mut turns: Vec<Turn> = segs
            .into_iter()
            .map(|s| Turn { start_ms: (s.start * 1000.0) as i64, end_ms: (s.end * 1000.0) as i64, cluster: s.speaker })
            .collect();
        turns.sort_by_key(|t| t.start_ms);
        Ok(turns)
    }

    /// Voice embedding for >= 1 s of 16 kHz mono audio.
    pub fn embed(&mut self, pcm16k: &[f32]) -> Result<Vec<f32>, DiarizeError> {
        if pcm16k.len() < 16_000 {
            return Err(DiarizeError::Engine("need at least 1 s of audio".into()));
        }
        self.extractor.compute_speaker_embedding(pcm16k.to_vec(), 16_000).map_err(|e| DiarizeError::Engine(e.to_string()))
    }
}
