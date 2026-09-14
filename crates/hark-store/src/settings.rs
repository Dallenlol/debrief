use crate::{Result, Store};
use rusqlite::{params, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};

impl Store {
    pub fn get_setting<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let conn = self.conn.lock();
        let raw: Option<String> = conn
            .query_row("SELECT value FROM settings WHERE key=?1", params![key], |r| r.get(0))
            .optional()?;
        Ok(match raw {
            Some(s) => Some(serde_json::from_str(&s)?),
            None => None,
        })
    }

    pub fn set_setting<T: Serialize>(&self, key: &str, v: &T) -> Result<()> {
        let json = serde_json::to_string(v)?;
        self.conn.lock().execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, json],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_json_roundtrip() {
        let s = Store::open_in_memory().unwrap();
        s.set_setting("video_enabled", &true).unwrap();
        assert_eq!(s.get_setting::<bool>("video_enabled").unwrap(), Some(true));
        assert_eq!(s.get_setting::<bool>("missing").unwrap(), None);
        s.set_setting("video_enabled", &false).unwrap();
        assert_eq!(s.get_setting::<bool>("video_enabled").unwrap(), Some(false));
    }
}
