use rusqlite::Connection;

const MIGRATIONS: &[&str] = &[
    // v1
    r#"
CREATE TABLE meetings(
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  app TEXT,
  started_at TEXT NOT NULL,
  ended_at TEXT,
  duration_ms INTEGER NOT NULL DEFAULT 0,
  has_video INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL,
  folder_id TEXT,
  created_at TEXT NOT NULL
);
CREATE TABLE segments(
  id INTEGER PRIMARY KEY,
  meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  start_ms INTEGER NOT NULL,
  end_ms INTEGER NOT NULL,
  speaker TEXT,
  text TEXT NOT NULL,
  clean_text TEXT
);
CREATE INDEX segments_meeting ON segments(meeting_id, start_ms);
CREATE VIRTUAL TABLE segments_fts USING fts5(text, clean_text, content='segments', content_rowid='id');
CREATE TRIGGER segments_ai AFTER INSERT ON segments BEGIN
  INSERT INTO segments_fts(rowid, text, clean_text) VALUES (new.id, new.text, new.clean_text);
END;
CREATE TRIGGER segments_ad AFTER DELETE ON segments BEGIN
  INSERT INTO segments_fts(segments_fts, rowid, text, clean_text) VALUES ('delete', old.id, old.text, old.clean_text);
END;
CREATE TRIGGER segments_au AFTER UPDATE ON segments BEGIN
  INSERT INTO segments_fts(segments_fts, rowid, text, clean_text) VALUES ('delete', old.id, old.text, old.clean_text);
  INSERT INTO segments_fts(rowid, text, clean_text) VALUES (new.id, new.text, new.clean_text);
END;
CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
"#,
    // v2: speakers
    r#"
CREATE TABLE speakers(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  embedding BLOB NOT NULL,
  dim INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX speakers_name ON speakers(name COLLATE NOCASE);
CREATE TABLE meeting_speakers(
  meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  label TEXT NOT NULL,
  speaker_id TEXT,
  suggested_id TEXT,
  suggested_score REAL,
  PRIMARY KEY(meeting_id, label)
);
"#,
];

pub fn run(conn: &Connection) -> rusqlite::Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let v = (i + 1) as i64;
        if v > current {
            conn.execute_batch(sql)?;
            conn.pragma_update(None, "user_version", v)?;
        }
    }
    Ok(())
}
