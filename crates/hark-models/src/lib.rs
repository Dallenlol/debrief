//! Hardware probing, model tiers, the model catalog and verified downloads.

pub mod catalog;
pub mod download;
pub mod hardware;

pub use catalog::{is_present, model_path, select_tier, Catalog, ModelKind, ModelSpec, Tier, TierModels};
pub use download::{download, sha256_file, verify_sha256, DownloadError};
pub use hardware::{probe, Gpu, GpuBackend, Hardware};
