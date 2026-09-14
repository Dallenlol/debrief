use crate::{migrations, Result};
use parking_lot::Mutex;
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// Default location: `%APPDATA%\Hark` on Windows,
/// `~/Library/Application Support/Hark` on macOS, `~/.local/share/hark` elsewhere.
pub fn default_data_dir() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "linux") { base.join("hark") } else { base.join("Hark") }
}

const POINTER_FILE: &str = "data-dir.txt";

/// Root of all Hark data. The default location may hold a `data-dir.txt`
/// pointer to a user-chosen folder (see `write_data_dir_pointer`). Created on first call.
pub fn data_dir() -> PathBuf {
    let default = default_data_dir();
    let dir = std::fs::read_to_string(default.join(POINTER_FILE))
        .ok()
        .map(|s| PathBuf::from(s.trim()))
        .filter(|p| !p.as_os_str().is_empty() && p.is_dir())
        .unwrap_or(default);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Point future launches at `dir` (pass the default dir to clear the override).
pub fn write_data_dir_pointer(dir: &Path) -> std::io::Result<()> {
    let default = default_data_dir();
    std::fs::create_dir_all(&default)?;
    if dir == default {
        let _ = std::fs::remove_file(default.join(POINTER_FILE));
        Ok(())
    } else {
        std::fs::write(default.join(POINTER_FILE), dir.to_string_lossy().as_bytes())
    }
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

    /// Fold the WAL into the main database file (before copying it elsewhere).
    pub fn checkpoint(&self) -> Result<()> {
        self.conn.lock().execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
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
