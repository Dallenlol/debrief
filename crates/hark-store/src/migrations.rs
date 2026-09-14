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
    // v3: templates, summaries, chats, retrieval chunks
    r#"
CREATE TABLE templates(
  id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL DEFAULT '', body TEXT NOT NULL,
  builtin INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL
);
CREATE TABLE summaries(
  meeting_id TEXT PRIMARY KEY REFERENCES meetings(id) ON DELETE CASCADE,
  template_id TEXT NOT NULL, markdown TEXT NOT NULL, structured_json TEXT NOT NULL, model TEXT NOT NULL, created_at TEXT NOT NULL
);
CREATE TABLE chats(
  id TEXT PRIMARY KEY, scope_kind TEXT NOT NULL, scope_id TEXT, title TEXT NOT NULL, created_at TEXT NOT NULL
);
CREATE INDEX chats_scope ON chats(scope_kind, scope_id);
CREATE TABLE chat_messages(
  id TEXT PRIMARY KEY, chat_id TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
  role TEXT NOT NULL, content TEXT NOT NULL, citations_json TEXT NOT NULL DEFAULT '[]', created_at TEXT NOT NULL
);
CREATE INDEX chat_messages_chat ON chat_messages(chat_id, created_at);
CREATE TABLE chunks(
  id INTEGER PRIMARY KEY, meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL, text TEXT NOT NULL
);
CREATE INDEX chunks_meeting ON chunks(meeting_id, start_ms);
CREATE VIRTUAL TABLE chunks_fts USING fts5(text, content='chunks', content_rowid='id');
CREATE TRIGGER chunks_ai AFTER INSERT ON chunks BEGIN INSERT INTO chunks_fts(rowid, text) VALUES (new.id, new.text); END;
CREATE TRIGGER chunks_ad AFTER DELETE ON chunks BEGIN INSERT INTO chunks_fts(chunks_fts, rowid, text) VALUES ('delete', old.id, old.text); END;
"#,
    // v4: folders, tags, shares
    r#"
CREATE TABLE folders(
  id TEXT PRIMARY KEY, name TEXT NOT NULL, parent_id TEXT, default_template_id TEXT, created_at TEXT NOT NULL
);
CREATE TABLE tags(id TEXT PRIMARY KEY, name TEXT NOT NULL);
CREATE UNIQUE INDEX tags_name ON tags(name COLLATE NOCASE);
CREATE TABLE meeting_tags(
  meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  tag_id TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY(meeting_id, tag_id)
);
CREATE TABLE shares(
  token TEXT PRIMARY KEY, meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  kind TEXT NOT NULL, start_ms INTEGER, end_ms INTEGER, enabled INTEGER NOT NULL DEFAULT 1, created_at TEXT NOT NULL
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
