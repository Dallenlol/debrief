//! Who spoke when: sherpa-onnx speaker segmentation + embeddings, plus the
//! pure logic to attach clusters to captions, spot the local user, and match
//! voices against speakers seen in earlier meetings.

pub mod engine;
pub mod identify;
pub mod merge;

pub use engine::{DiarizeEngine, DiarizeError};
pub use identify::{best_match, centroid, cosine, KnownSpeaker};
pub use merge::{assign_clusters, find_me_cluster, Turn};
