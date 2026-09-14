//! JSON contract between the app and the `hark-diarize` sidecar.

use crate::Turn;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Passed on the command line: `hark-diarize --seg <onnx> --emb <onnx> --wav <16k mono wav> [--max-embed-secs 60]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizeRequest {
    pub seg_model: String,
    pub emb_model: String,
    pub wav: String,
    #[serde(default = "default_max_embed")]
    pub max_embed_secs: usize,
}

fn default_max_embed() -> usize {
    60
}

/// Printed to stdout as one JSON document.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DiarizeOutput {
    pub turns: Vec<Turn>,
    /// cluster id -> L2-normalised voice embedding (clusters with < 1 s of audio are omitted)
    pub embeddings: BTreeMap<i32, Vec<f32>>,
    pub embedding_dim: usize,
    #[serde(default)]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_roundtrips_json() {
        let mut o = DiarizeOutput { turns: vec![Turn { start_ms: 0, end_ms: 10, cluster: 1 }], ..Default::default() };
        o.embeddings.insert(1, vec![0.5, 0.5]);
        o.embedding_dim = 2;
        let s = serde_json::to_string(&o).unwrap();
        assert_eq!(serde_json::from_str::<DiarizeOutput>(&s).unwrap(), o);
    }
}
