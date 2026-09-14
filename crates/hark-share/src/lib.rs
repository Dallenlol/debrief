//! Sharing without a cloud: `.hark` bundles (zip) for moving meetings between
//! computers, a standalone HTML export, and a small LAN web server for links.

pub mod bundle;
pub mod html;
pub mod server;

pub use bundle::{export_bundle, import_bundle, BundleManifest, ImportReport};
pub use html::render_standalone;
pub use server::{ShareServer, ShareServerHandle};

#[derive(Debug, thiserror::Error)]
pub enum ShareError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("store: {0}")]
    Store(#[from] hark_store::StoreError),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, ShareError>;
