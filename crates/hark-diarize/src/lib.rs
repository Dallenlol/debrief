//! Who spoke when: the pure logic to attach diarization clusters to captions,
//! spot the local user, and match voices against speakers seen before.
//! The sherpa-onnx engine itself runs in the `hark-diarize` sidecar
//! (crate `hark-diarize-cli`); `protocol` defines its JSON output.

pub mod identify;
pub mod merge;
pub mod protocol;

pub use identify::{best_match, centroid, cosine, KnownSpeaker};
pub use merge::{assign_clusters, find_me_cluster, Turn};
pub use protocol::{DiarizeOutput, DiarizeRequest};
