use crate::{migrations, Result};
use parking_lot::Mutex;
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// Default location: `%APPDATA%\Debrief` on Windows, `~/Library/Application Support/Debrief`
/// on macOS, the platform data dir elsewhere; an old `Hark` folder is moved there on first start.
pub fn default_data_dir() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    let (new, old) = if cfg!(target_os = "linux") { ("debrief", "hark") } else { ("Debrief", "Hark") };
    migrate_data_dir(&base.join(old), &base.join(new))
}

/// The app was called Hark: move its folder (database, recordings, models, and the
/// pointer to a custom data folder) to the new name once. If the move fails (the old
/// app still running, say), keep using the old folder so nothing goes missing.
fn migrate_data_dir(old: &Path, new: &Path) -> PathBuf {
    if new.exists() || !old.exists() {
        return new.to_path_buf();
    }
    match std::fs::rename(old, new) {
        Ok(()) => new.to_path_buf(),
        Err(_) => old.to_path_buf(),
    }
}

const POINTER_FILE: &str = "data-dir.txt";

/// Root of all Debrief data. The default location may hold a `data-dir.txt`
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
        let d = self.recordings_path(meeting_id);
        let _ = std::fs::create_dir_all(&d);
        d
    }

    /// Where a meeting's media lives, without creating it. The id becomes one path component
    /// with anything but ASCII letters, digits and `-` replaced, so an id from an imported
    /// bundle (`..\..\Startup`, `C:\x`) can never point outside the recordings folder.
    pub fn recordings_path(&self, meeting_id: &str) -> PathBuf {
        self.root.join("recordings").join(safe_component(meeting_id))
    }
}

/// `id` as a single, harmless file-name component.
pub fn safe_component(id: &str) -> String {
    let s: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
    if s.is_empty() {
        "_".into()
    } else {
        s
    }
}

#[cfg(test)]
mod path_tests {
    use super::safe_component;

    #[test]
    fn meeting_ids_cannot_escape_the_recordings_folder() {
        assert_eq!(safe_component("2f1c5a7e-0d24-4c25-adfa-75f312e330ea"), "2f1c5a7e-0d24-4c25-adfa-75f312e330ea");
        assert_eq!(safe_component(r"..\..\Startup"), "______Startup");
        assert_eq!(safe_component("../../etc"), "______etc");
        assert_eq!(safe_component(r"C:\Windows"), "C__Windows");
        assert_eq!(safe_component(""), "_");
    }
}
