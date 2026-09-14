use crate::{migrations, Result};
use parking_lot::Mutex;
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// Root of all Hark data. `%APPDATA%\Hark` on Windows,
/// `~/Library/Application Support/Hark` on macOS, `~/.local/share/hark` elsewhere.
/// Created on first call.
pub fn data_dir() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = if cfg!(target_os = "linux") { base.join("hark") } else { base.join("Hark") };
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub struct Store {
    pub(crate) conn: Mutex<Connection>,
    root: PathBuf,
}

impl Store {
    /// Open (or create) the database at `path` and run migrations.
    /// `path`'s parent directory is treated as the data root.
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let conn = Connection::open(path)?;
        let root = path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        Self::init(conn, root)
    }

    pub fn open_in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?, std::env::temp_dir().join("hark-test"))
    }

    fn init(conn: Connection, root: PathBuf) -> Result<Store> {
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        migrations::run(&conn)?;
        Ok(Store { conn: Mutex::new(conn), root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn models_dir(&self) -> PathBuf {
        let d = self.root.join("models");
        let _ = std::fs::create_dir_all(&d);
        d
    }

    /// `<root>/recordings/<meeting_id>`, created if missing.
    pub fn recordings_dir(&self, meeting_id: &str) -> PathBuf {
        let d = self.root.join("recordings").join(meeting_id);
        let _ = std::fs::create_dir_all(&d);
        d
    }
}
