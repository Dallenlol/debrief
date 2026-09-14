//! Templates, summaries, chats and retrieval chunks.

use crate::{Result, Store};
use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub description: String,
    pub body: String,
    pub builtin: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Summary {
    pub meeting_id: String,
    pub template_id: String,
    pub markdown: String,
    pub structured: serde_json::Value,
    pub model: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Chat {
    pub id: String,
    /// "meeting" | "all" (folders arrive in M4)
    pub scope_kind: String,
    pub scope_id: Option<String>,
    pub title: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ChatMessage {
    pub id: String,
    pub chat_id: String,
    pub role: String,
    pub content: String,
    pub citations: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ChunkHit {
    pub meeting_id: String,
    pub meeting_title: String,
    pub started_at: DateTime<Utc>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub rank: f64,
}

/// Window length for retrieval chunks.
pub const CHUNK_MS: i64 = 45_000;

impl Store {
    // ---- templates ----
    pub fn list_templates(&self) -> Result<Vec<Template>> {
        let conn = self.conn.lock();
        let mut st = conn.prepare("SELECT id, name, description, body, builtin FROM templates ORDER BY builtin DESC, name")?;
        let rows = st.query_map([], |r| {
            Ok(Template { id: r.get(0)?, name: r.get(1)?, description: r.get(2)?, body: r.get(3)?, builtin: r.get::<_, i64>(4)? != 0 })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_template(&self, id: &str) -> Result<Option<Template>> {
        let conn = self.conn.lock();
        Ok(conn
            .query_row("SELECT id, name, description, body, builtin FROM templates WHERE id = ?1", params![id], |r| {
                Ok(Template { id: r.get(0)?, name: r.get(1)?, description: r.get(2)?, body: r.get(3)?, builtin: r.get::<_, i64>(4)? != 0 })
            })
            .optional()?)
    }

    pub fn upsert_template(&self, t: &Template) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO templates(id, name, description, body, builtin, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, description = excluded.description, body = excluded.body",
            params![t.id, t.name, t.description, t.body, t.builtin as i64, Utc::now()],
        )?;
        Ok(())
    }

    /// Insert built-ins only if missing (so user edits to them survive upgrades).
    pub fn seed_templates(&self, builtins: &[Template]) -> Result<()> {
        let conn = self.conn.lock();
        for t in builtins {
            conn.execute(
                "INSERT OR IGNORE INTO templates(id, name, description, body, builtin, created_at) VALUES (?1, ?2, ?3, ?4, 1, ?5)",
                params![t.id, t.name, t.description, t.body, Utc::now()],
            )?;
        }
        Ok(())
    }

    pub fn delete_template(&self, id: &str) -> Result<()> {
        self.conn.lock().execute("DELETE FROM templates WHERE id = ?1 AND builtin = 0", params![id])?;
        Ok(())
    }

    // ---- summaries ----
    pub fn set_summary(&self, s: &Summary) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO summaries(meeting_id, template_id, markdown, structured_json, model, created_at) VALUES (?1,?2,?3,?4,?5,?6)
             ON CONFLICT(meeting_id) DO UPDATE SET template_id = excluded.template_id, markdown = excluded.markdown,
             structured_json = excluded.structured_json, model = excluded.model, created_at = excluded.created_at",
            params![s.meeting_id, s.template_id, s.markdown, s.structured.to_string(), s.model, s.created_at],
        )?;
        Ok(())
    }

    pub fn get_summary(&self, meeting_id: &str) -> Result<Option<Summary>> {
        let conn = self.conn.lock();
        Ok(conn
            .query_row(
                "SELECT meeting_id, template_id, markdown, structured_json, model, created_at FROM summaries WHERE meeting_id = ?1",
                params![meeting_id],
                |r| {
                    Ok(Summary {
                        meeting_id: r.get(0)?,
                        template_id: r.get(1)?,
                        markdown: r.get(2)?,
                        structured: serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or(serde_json::Value::Null),
                        model: r.get(4)?,
                        created_at: r.get(5)?,
                    })
                },
            )
            .optional()?)
    }

    // ---- chats ----
    pub fn create_chat(&self, scope_kind: &str, scope_id: Option<&str>, title: &str) -> Result<Chat> {
        let c = Chat { id: uuid::Uuid::new_v4().to_string(), scope_kind: scope_kind.into(), scope_id: scope_id.map(str::to_string), title: title.into(), created_at: Utc::now() };
        self.conn.lock().execute(
            "INSERT INTO chats(id, scope_kind, scope_id, title, created_at) VALUES (?1,?2,?3,?4,?5)",
            params![c.id, c.scope_kind, c.scope_id, c.title, c.created_at],
        )?;
        Ok(c)
    }

    pub fn list_chats(&self, scope_kind: &str, scope_id: Option<&str>) -> Result<Vec<Chat>> {
        let conn = self.conn.lock();
        let mut st = conn.prepare(
            "SELECT id, scope_kind, scope_id, title, created_at FROM chats WHERE scope_kind = ?1 AND (scope_id IS ?2) ORDER BY created_at DESC",
        )?;
        let rows = st.query_map(params![scope_kind, scope_id], |r| {
            Ok(Chat { id: r.get(0)?, scope_kind: r.get(1)?, scope_id: r.get(2)?, title: r.get(3)?, created_at: r.get(4)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_chat(&self, id: &str) -> Result<Option<Chat>> {
        let conn = self.conn.lock();
        Ok(conn
            .query_row("SELECT id, scope_kind, scope_id, title, created_at FROM chats WHERE id = ?1", params![id], |r| {
                Ok(Chat { id: r.get(0)?, scope_kind: r.get(1)?, scope_id: r.get(2)?, title: r.get(3)?, created_at: r.get(4)? })
            })
            .optional()?)
    }

    pub fn rename_chat(&self, id: &str, title: &str) -> Result<()> {
        self.conn.lock().execute("UPDATE chats SET title = ?2 WHERE id = ?1", params![id, title])?;
        Ok(())
    }

    pub fn delete_chat(&self, id: &str) -> Result<()> {
        self.conn.lock().execute("DELETE FROM chats WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn add_message(&self, chat_id: &str, role: &str, content: &str, citations: &serde_json::Value) -> Result<ChatMessage> {
        let m = ChatMessage { id: uuid::Uuid::new_v4().to_string(), chat_id: chat_id.into(), role: role.into(), content: content.into(), citations: citations.clone(), created_at: Utc::now() };
        self.conn.lock().execute(
            "INSERT INTO chat_messages(id, chat_id, role, content, citations_json, created_at) VALUES (?1,?2,?3,?4,?5,?6)",
            params![m.id, m.chat_id, m.role, m.content, m.citations.to_string(), m.created_at],
        )?;
        Ok(m)
    }

    pub fn update_message(&self, id: &str, content: &str, citations: &serde_json::Value) -> Result<()> {
        self.conn.lock().execute("UPDATE chat_messages SET content = ?2, citations_json = ?3 WHERE id = ?1", params![id, content, citations.to_string()])?;
        Ok(())
    }

    pub fn messages(&self, chat_id: &str) -> Result<Vec<ChatMessage>> {
        let conn = self.conn.lock();
        let mut st = conn.prepare(
            "SELECT id, chat_id, role, content, citations_json, created_at FROM chat_messages WHERE chat_id = ?1 ORDER BY created_at, rowid",
        )?;
        let rows = st.query_map(params![chat_id], |r| {
            Ok(ChatMessage {
                id: r.get(0)?,
                chat_id: r.get(1)?,
                role: r.get(2)?,
                content: r.get(3)?,
                citations: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or(serde_json::Value::Array(vec![])),
                created_at: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ---- chunks (retrieval) ----
    /// Rebuild ~45 s retrieval chunks from the meeting's segments (clean text when available).
    pub fn rebuild_chunks(&self, meeting_id: &str) -> Result<usize> {
        let segs = self.segments(meeting_id)?;
        let mut chunks: Vec<(i64, i64, String)> = Vec::new();
        let mut cur_start: Option<i64> = None;
        let mut cur_end = 0;
        let mut cur_text = String::new();
        for s in &segs {
            let text = s.clean_text.clone().unwrap_or_else(|| s.text.clone());
            let line = match &s.speaker {
                Some(sp) => format!("{sp}: {text}"),
                None => text,
            };
            match cur_start {
                Some(st) if s.end_ms - st > CHUNK_MS => {
                    chunks.push((st, cur_end, std::mem::take(&mut cur_text)));
                    cur_start = Some(s.start_ms);
                }
                None => cur_start = Some(s.start_ms),
                _ => {}
            }
            if !cur_text.is_empty() {
                cur_text.push('\n');
            }
            cur_text.push_str(&line);
            cur_end = s.end_ms;
        }
        if let Some(st) = cur_start {
            if !cur_text.is_empty() {
                chunks.push((st, cur_end, cur_text));
            }
        }
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM chunks WHERE meeting_id = ?1", params![meeting_id])?;
        {
            let mut st = tx.prepare("INSERT INTO chunks(meeting_id, start_ms, end_ms, text) VALUES (?1,?2,?3,?4)")?;
            for (a, b, t) in &chunks {
                st.execute(params![meeting_id, a, b, t])?;
            }
        }
        tx.commit()?;
        Ok(chunks.len())
    }

    /// BM25 search over chunks; `scope` limits to one meeting.
    pub fn search_chunks(&self, query: &str, scope: Option<&str>, limit: usize) -> Result<Vec<ChunkHit>> {
        let q = crate::segments::fts_query_or(query);
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock();
        let mut st = conn.prepare(
            "SELECT c.meeting_id, m.title, m.started_at, c.start_ms, c.end_ms, c.text, bm25(chunks_fts) AS rank
             FROM chunks_fts JOIN chunks c ON c.id = chunks_fts.rowid JOIN meetings m ON m.id = c.meeting_id
             WHERE chunks_fts MATCH ?1 AND (?2 IS NULL OR c.meeting_id = ?2)
             ORDER BY rank LIMIT ?3",
        )?;
        let rows = st.query_map(params![q, scope, limit as i64], |r| {
            Ok(ChunkHit {
                meeting_id: r.get(0)?,
                meeting_title: r.get(1)?,
                started_at: r.get(2)?,
                start_ms: r.get(3)?,
                end_ms: r.get(4)?,
                text: r.get(5)?,
                rank: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// All chunks of one meeting in order (for small meetings we can pass everything).
    pub fn chunks(&self, meeting_id: &str) -> Result<Vec<ChunkHit>> {
        let conn = self.conn.lock();
        let mut st = conn.prepare(
            "SELECT c.meeting_id, m.title, m.started_at, c.start_ms, c.end_ms, c.text FROM chunks c JOIN meetings m ON m.id = c.meeting_id
             WHERE c.meeting_id = ?1 ORDER BY c.start_ms",
        )?;
        let rows = st.query_map(params![meeting_id], |r| {
            Ok(ChunkHit { meeting_id: r.get(0)?, meeting_title: r.get(1)?, started_at: r.get(2)?, start_ms: r.get(3)?, end_ms: r.get(4)?, text: r.get(5)?, rank: 0.0 })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Meeting, NewSegment};

    fn seg(s: i64, t: &str) -> NewSegment {
        NewSegment { start_ms: s, end_ms: s + 10_000, speaker: Some("A".into()), text: t.into() }
    }

    #[test]
    fn chunks_window_and_scoped_search() {
        let st = Store::open_in_memory().unwrap();
        let m1 = Meeting::new_recording("Budget", None);
        let m2 = Meeting::new_recording("Hiring", None);
        st.create_meeting(&m1).unwrap();
        st.create_meeting(&m2).unwrap();
        let segs: Vec<NewSegment> = (0..12).map(|i| seg(i * 10_000, if i == 7 { "we agreed to cut marketing" } else { "filler talk" })).collect();
        st.replace_segments(&m1.id, &segs).unwrap();
        st.replace_segments(&m2.id, &[seg(0, "marketing hire approved")]).unwrap();
        let n = st.rebuild_chunks(&m1.id).unwrap();
        assert!(n >= 3, "expected several 45 s chunks, got {n}");
        st.rebuild_chunks(&m2.id).unwrap();
        let all = st.search_chunks("marketing", None, 10).unwrap();
        assert_eq!(all.len(), 2);
        let scoped = st.search_chunks("marketing", Some(&m1.id), 10).unwrap();
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].start_ms, 40_000);
        assert!(scoped[0].text.contains("A: we agreed"));
    }

    #[test]
    fn summary_and_chat_roundtrip() {
        let st = Store::open_in_memory().unwrap();
        let m = Meeting::new_recording("x", None);
        st.create_meeting(&m).unwrap();
        st.seed_templates(&[Template { id: "general".into(), name: "General".into(), description: "d".into(), body: "b".into(), builtin: true }]).unwrap();
        st.seed_templates(&[Template { id: "general".into(), name: "Changed".into(), description: "d".into(), body: "b".into(), builtin: true }]).unwrap();
        assert_eq!(st.list_templates().unwrap()[0].name, "General");
        st.delete_template("general").unwrap();
        assert_eq!(st.list_templates().unwrap().len(), 1, "built-ins cannot be deleted");

        let s = Summary { meeting_id: m.id.clone(), template_id: "general".into(), markdown: "## Summary\nhi".into(), structured: serde_json::json!({"summary": "hi"}), model: "test".into(), created_at: Utc::now() };
        st.set_summary(&s).unwrap();
        assert_eq!(st.get_summary(&m.id).unwrap().unwrap().structured["summary"], "hi");

        let c = st.create_chat("meeting", Some(&m.id), "Questions").unwrap();
        st.add_message(&c.id, "user", "what?", &serde_json::json!([])).unwrap();
        let a = st.add_message(&c.id, "assistant", "", &serde_json::json!([])).unwrap();
        st.update_message(&a.id, "answer [0:05]", &serde_json::json!([{"ms": 5000}])).unwrap();
        let msgs = st.messages(&c.id).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].content, "answer [0:05]");
        assert_eq!(st.list_chats("meeting", Some(&m.id)).unwrap().len(), 1);
        assert!(st.list_chats("all", None).unwrap().is_empty());
        st.delete_chat(&c.id).unwrap();
        assert!(st.messages(&c.id).unwrap().is_empty());
    }
}
