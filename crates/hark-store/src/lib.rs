//! Hark local storage: SQLite (+ FTS5) and the on-disk recording layout.
//!
//! The crate knows nothing about Tauri. Everything is synchronous; callers
//! wrap it in `Arc` and call from whatever thread they like.

mod ai;
mod db;
mod meetings;
mod migrations;
mod organize;
mod segments;
mod settings;
mod speakers;

pub use ai::{Chat, ChatMessage, ChunkHit, Summary, Template, CHUNK_MS};
pub use db::{data_dir, Store};
pub use meetings::{Meeting, MeetingStatus};
pub use organize::{Folder, MeetingFilter, Share, Tag};
pub use segments::{NewSegment, SearchHit, Segment};
pub use speakers::{MeetingSpeaker, Speaker};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, StoreError>;
