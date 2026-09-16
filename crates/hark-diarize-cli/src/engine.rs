//! sherpa-onnx wrappers. Models: pyannote segmentation 3.0 + a speaker
//! embedding model (NeMo TitaNet small by default).
//!
//! The diarization pipeline talks to the C API directly: `sherpa_rs::Diarize`
//! pins both models to a single thread, which made a 90-minute call take ten
//! minutes; onnxruntime scales well across cores for these models.

use hark_diarize::Turn;
use sherpa_rs::speaker_id::{EmbeddingExtractor, ExtractorConfig};
use std::ffi::CString;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum DiarizeError {
    #[error("model not found: {0}")]
    ModelNotFound(String),
    #[error("diarization: {0}")]
    Engine(String),
}

pub struct DiarizeEngine {
    sd: *const sherpa_rs_sys::SherpaOnnxOfflineSpeakerDiarization,
    extractor: EmbeddingExtractor,
    pub embedding_dim: usize,
    #[allow(dead_code)]
    pub threads: i32,
}

// sherpa handles are plain pointers used from one thread at a time behind a Mutex upstream.
unsafe impl Send for DiarizeEngine {}

impl Drop for DiarizeEngine {
    fn drop(&mut self) {
        if !self.sd.is_null() {
            unsafe { sherpa_rs_sys::SherpaOnnxDestroyOfflineSpeakerDiarization(self.sd) };
        }
    }
}

/// Threads for the ONNX models: physical cores, capped where onnxruntime stops scaling.
pub fn default_threads() -> i32 {
    let logical = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    // Most desktop CPUs report 2 logical per physical core; the models are compute-bound.
    (logical / 2).clamp(2, 8) as i32
}

type ProgressBox = Box<dyn Fn(i32, i32) -> i32 + Send>;

unsafe extern "C" fn progress_trampoline(done: i32, total: i32, arg: *mut std::ffi::c_void) -> i32 {
    let cb = &*(arg as *const ProgressBox);
    cb(done, total)
}

impl DiarizeEngine {
    /// `provider` is an onnxruntime execution provider name ("cpu", "cuda", "directml");
    /// anything the bundled runtime lacks makes creation fail, so callers fall back to "cpu".
    pub fn load_with(seg_model: &Path, emb_model: &Path, threads: i32, provider: &str) -> Result<Self, DiarizeError> {
        for p in [seg_model, emb_model] {
            if !p.exists() {
                return Err(DiarizeError::ModelNotFound(p.display().to_string()));
            }
        }
        let seg_c = CString::new(seg_model.to_string_lossy().into_owned()).map_err(|e| DiarizeError::Engine(e.to_string()))?;
        let emb_c = CString::new(emb_model.to_string_lossy().into_owned()).map_err(|e| DiarizeError::Engine(e.to_string()))?;
        let prov_c = CString::new(provider).map_err(|e| DiarizeError::Engine(e.to_string()))?;
        let config = sherpa_rs_sys::SherpaOnnxOfflineSpeakerDiarizationConfig {
            segmentation: sherpa_rs_sys::SherpaOnnxOfflineSpeakerSegmentationModelConfig {
                pyannote: sherpa_rs_sys::SherpaOnnxOfflineSpeakerSegmentationPyannoteModelConfig { model: seg_c.as_ptr() },
                num_threads: threads,
                debug: 0,
                provider: prov_c.as_ptr(),
            },
            embedding: sherpa_rs_sys::SherpaOnnxSpeakerEmbeddingExtractorConfig {
                model: emb_c.as_ptr(),
                num_threads: threads,
                debug: 0,
                provider: prov_c.as_ptr(),
            },
            // Let clustering decide the speaker count from the threshold; the app
            // consolidates slivers afterwards (hark_diarize::consolidate).
            clustering: sherpa_rs_sys::SherpaOnnxFastClusteringConfig { num_clusters: -1, threshold: 0.5 },
            min_duration_on: 0.3,
            min_duration_off: 0.5,
        };
        let sd = unsafe { sherpa_rs_sys::SherpaOnnxCreateOfflineSpeakerDiarization(&config) };
        if sd.is_null() {
            return Err(DiarizeError::Engine(format!("failed to initialise speaker diarization (provider {provider})")));
        }
        let extractor = EmbeddingExtractor::new(ExtractorConfig {
            model: emb_model.to_string_lossy().into_owned(),
            provider: None,
            num_threads: Some(threads as usize),
            debug: false,
        })
        .map_err(|e| DiarizeError::Engine(e.to_string()))?;
        let embedding_dim = extractor.embedding_size;
        Ok(DiarizeEngine { sd, extractor, embedding_dim, threads })
    }

    /// Speaker turns for 16 kHz mono audio, sorted by start time.
    pub fn diarize(&mut self, pcm16k: &[f32], progress: impl Fn(f32) + Send + 'static) -> Result<Vec<Turn>, DiarizeError> {
        if pcm16k.len() < 16_000 {
            return Ok(Vec::new());
        }
        let cb: ProgressBox = Box::new(move |done, total| {
            progress(if total > 0 { done as f32 / total as f32 } else { 0.0 });
            0
        });
        let mut turns = Vec::new();
        unsafe {
            let result = sherpa_rs_sys::SherpaOnnxOfflineSpeakerDiarizationProcessWithCallback(
                self.sd,
                pcm16k.as_ptr(),
                pcm16k.len() as i32,
                Some(progress_trampoline),
                &cb as *const ProgressBox as *mut std::ffi::c_void,
            );
            if result.is_null() {
                return Err(DiarizeError::Engine("diarization returned no result".into()));
            }
            let n = sherpa_rs_sys::SherpaOnnxOfflineSpeakerDiarizationResultGetNumSegments(result);
            let segs = sherpa_rs_sys::SherpaOnnxOfflineSpeakerDiarizationResultSortByStartTime(result);
            if !segs.is_null() && n > 0 {
                for s in std::slice::from_raw_parts(segs, n as usize) {
                    turns.push(Turn { start_ms: (s.start * 1000.0) as i64, end_ms: (s.end * 1000.0) as i64, cluster: s.speaker });
                }
                sherpa_rs_sys::SherpaOnnxOfflineSpeakerDiarizationDestroySegment(segs);
            }
            sherpa_rs_sys::SherpaOnnxOfflineSpeakerDiarizationDestroyResult(result);
        }
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
