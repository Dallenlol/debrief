//! Folders, tags and share tokens.

use crate::{Meeting, Result, Store};
use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub default_template_id: Option<String>,
    pub meeting_count: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub meeting_count: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Share {
    pub token: String,
    pub meeting_id: String,
    pub meeting_title: String,
    /// "meeting" | "clip"
    pub kind: String,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

/// Filter for `list_meetings_filtered`.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct MeetingFilter {
    /// `Some(None)` = unfiled meetings only; `Some(Some(id))` = folder subtree; `None` = everything.
    pub folder: Option<Option<String>>,
    pub tag_id: Option<String>,
}

impl Store {
    // ---- folders ----
    pub fn list_folders(&self) -> Result<Vec<Folder>> {
        let conn = self.conn.lock();
        let mut st = conn.prepare(
            "SELECT f.id, f.name, f.parent_id, f.default_template_id, (SELECT COUNT(*) FROM meetings m WHERE m.folder_id = f.id)
             FROM folders f ORDER BY f.name",
        )?;
        let rows = st.query_map([], |r| {
            Ok(Folder { id: r.get(0)?, name: r.get(1)?, parent_id: r.get(2)?, default_template_id: r.get(3)?, meeting_count: r.get(4)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn create_folder(&self, name: &str, parent_id: Option<&str>) -> Result<Folder> {
        let f = Folder { id: uuid::Uuid::new_v4().to_string(), name: name.trim().to_string(), parent_id: parent_id.map(str::to_string), default_template_id: None, meeting_count: 0 };
        self.conn.lock().execute(
            "INSERT INTO folders(id, name, parent_id, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![f.id, f.name, f.parent_id, Utc::now()],
        )?;
        Ok(f)
    }

    pub fn update_folder(&self, id: &str, name: &str, parent_id: Option<&str>, default_template_id: Option<&str>) -> Result<()> {
        if parent_id == Some(id) {
            return Ok(());
        }
        self.conn.lock().execute(
            "UPDATE folders SET name = ?2, parent_id = ?3, default_template_id = ?4 WHERE id = ?1",
            params![id, name.trim(), parent_id, default_template_id],
        )?;
        Ok(())
    }

    /// Delete a folder; its meetings and subfolders move to its parent.
    pub fn delete_folder(&self, id: &str) -> Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let parent: Option<String> = tx.query_row("SELECT parent_id FROM folders WHERE id = ?1", params![id], |r| r.get(0)).optional()?.flatten();
        tx.execute("UPDATE meetings SET folder_id = ?2 WHERE folder_id = ?1", params![id, parent])?;
        tx.execute("UPDATE folders SET parent_id = ?2 WHERE parent_id = ?1", params![id, parent])?;
        tx.execute("DELETE FROM folders WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }

    /// The folder and all its descendants.
    pub fn folder_subtree(&self, id: &str) -> Result<Vec<String>> {
        let folders = self.list_folders()?;
        let mut out = vec![id.to_string()];
        let mut i = 0;
        while i < out.len() {
            let cur = out[i].clone();
            for f in folders.iter().filter(|f| f.parent_id.as_deref() == Some(cur.as_str())) {
                if !out.contains(&f.id) {
                    out.push(f.id.clone());
                }
            }
            i += 1;
        }
        Ok(out)
    }

    pub fn move_meeting(&self, meeting_id: &str, folder_id: Option<&str>) -> Result<()> {
        self.conn.lock().execute("UPDATE meetings SET folder_id = ?2 WHERE id = ?1", params![meeting_id, folder_id])?;
        Ok(())
    }

    pub fn list_meetings_filtered(&self, filter: &MeetingFilter) -> Result<Vec<Meeting>> {
        let all = self.list_meetings()?;
        let subtree = match &filter.folder {
            Some(Some(id)) => Some(self.folder_subtree(id)?),
            _ => None,
        };
        let tagged: Option<Vec<String>> = match &filter.tag_id {
            Some(t) => Some(self.meetings_with_tag(t)?),
            None => None,
        };
        Ok(all
            .into_iter()
            .filter(|m| match &filter.folder {
                None => true,
                Some(None) => m.folder_id.is_none(),
                Some(Some(_)) => m.folder_id.as_ref().map(|f| subtree.as_ref().unwrap().contains(f)).unwrap_or(false),
            })
            .filter(|m| tagged.as_ref().map(|t| t.contains(&m.id)).unwrap_or(true))
            .collect())
    }

    // ---- tags ----
    pub fn list_tags(&self) -> Result<Vec<Tag>> {
        let conn = self.conn.lock();
        let mut st = conn.prepare("SELECT t.id, t.name, (SELECT COUNT(*) FROM meeting_tags mt WHERE mt.tag_id = t.id) FROM tags t ORDER BY t.name")?;
        let rows = st.query_map([], |r| Ok(Tag { id: r.get(0)?, name: r.get(1)?, meeting_count: r.get(2)? }))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Attach a tag by name (created if new). Returns the tag.
    pub fn tag_meeting(&self, meeting_id: &str, name: &str) -> Result<Tag> {
        let name = name.trim();
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let id: Option<String> = tx.query_row("SELECT id FROM tags WHERE name = ?1 COLLATE NOCASE", params![name], |r| r.get(0)).optional()?;
        let id = match id {
            Some(id) => id,
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                tx.execute("INSERT INTO tags(id, name) VALUES (?1, ?2)", params![id, name])?;
                id
            }
        };
        tx.execute("INSERT OR IGNORE INTO meeting_tags(meeting_id, tag_id) VALUES (?1, ?2)", params![meeting_id, id])?;
        let count: i64 = tx.query_row("SELECT COUNT(*) FROM meeting_tags WHERE tag_id = ?1", params![id], |r| r.get(0))?;
        let tag_name: String = tx.query_row("SELECT name FROM tags WHERE id = ?1", params![id], |r| r.get(0))?;
        tx.commit()?;
        Ok(Tag { id, name: tag_name, meeting_count: count })
    }

    pub fn untag_meeting(&self, meeting_id: &str, tag_id: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM meeting_tags WHERE meeting_id = ?1 AND tag_id = ?2", params![meeting_id, tag_id])?;
        conn.execute("DELETE FROM tags WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM meeting_tags WHERE tag_id = ?1)", params![tag_id])?;
        Ok(())
    }

    pub fn meeting_tags(&self, meeting_id: &str) -> Result<Vec<Tag>> {
        let conn = self.conn.lock();
        let mut st = conn.prepare(
            "SELECT t.id, t.name, (SELECT COUNT(*) FROM meeting_tags x WHERE x.tag_id = t.id) FROM tags t
             JOIN meeting_tags mt ON mt.tag_id = t.id WHERE mt.meeting_id = ?1 ORDER BY t.name",
        )?;
        let rows = st.query_map(params![meeting_id], |r| Ok(Tag { id: r.get(0)?, name: r.get(1)?, meeting_count: r.get(2)? }))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn meetings_with_tag(&self, tag_id: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock();
        let mut st = conn.prepare("SELECT meeting_id FROM meeting_tags WHERE tag_id = ?1")?;
        let rows = st.query_map(params![tag_id], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ---- shares ----
    pub fn create_share(&self, meeting_id: &str, kind: &str, start_ms: Option<i64>, end_ms: Option<i64>) -> Result<Share> {
        let token: String = uuid::Uuid::new_v4().simple().to_string();
        let title: String = self.conn.lock().query_row("SELECT title FROM meetings WHERE id = ?1", params![meeting_id], |r| r.get(0))?;
        let s = Share { token, meeting_id: meeting_id.into(), meeting_title: title, kind: kind.into(), start_ms, end_ms, enabled: true, created_at: Utc::now() };
        self.conn.lock().execute(
            "INSERT INTO shares(token, meeting_id, kind, start_ms, end_ms, enabled, created_at) VALUES (?1,?2,?3,?4,?5,1,?6)",
            params![s.token, s.meeting_id, s.kind, s.start_ms, s.end_ms, s.created_at],
        )?;
        Ok(s)
    }

    pub fn list_shares(&self) -> Result<Vec<Share>> {
        let conn = self.conn.lock();
        let mut st = conn.prepare(
            "SELECT s.token, s.meeting_id, m.title, s.kind, s.start_ms, s.end_ms, s.enabled, s.created_at FROM shares s
             JOIN meetings m ON m.id = s.meeting_id ORDER BY s.created_at DESC",
        )?;
        let rows = st.query_map([], |r| {
            Ok(Share { token: r.get(0)?, meeting_id: r.get(1)?, meeting_title: r.get(2)?, kind: r.get(3)?, start_ms: r.get(4)?, end_ms: r.get(5)?, enabled: r.get::<_, i64>(6)? != 0, created_at: r.get(7)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_share(&self, token: &str) -> Result<Option<Share>> {
        Ok(self.list_shares()?.into_iter().find(|s| s.token == token && s.enabled))
    }

    pub fn set_share_enabled(&self, token: &str, enabled: bool) -> Result<()> {
        self.conn.lock().execute("UPDATE shares SET enabled = ?2 WHERE token = ?1", params![token, enabled as i64])?;
        Ok(())
    }

    pub fn delete_share(&self, token: &str) -> Result<()> {
        self.conn.lock().execute("DELETE FROM shares WHERE token = ?1", params![token])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_nest_and_delete_reparents() {
        let st = Store::open_in_memory().unwrap();
        let root = st.create_folder("Clients", None).unwrap();
        let sub = st.create_folder("Acme", Some(&root.id)).unwrap();
        let m = Meeting::new_recording("call", None);
        st.create_meeting(&m).unwrap();
        st.move_meeting(&m.id, Some(&sub.id)).unwrap();
        assert_eq!(st.folder_subtree(&root.id).unwrap().len(), 2);
        let in_root = st.list_meetings_filtered(&MeetingFilter { folder: Some(Some(root.id.clone())), tag_id: None }).unwrap();
        assert_eq!(in_root.len(), 1);
        let unfiled = st.list_meetings_filtered(&MeetingFilter { folder: Some(None), tag_id: None }).unwrap();
        assert!(unfiled.is_empty());
        st.delete_folder(&sub.id).unwrap();
        assert_eq!(st.get_meeting(&m.id).unwrap().unwrap().folder_id.as_deref(), Some(root.id.as_str()));
        assert_eq!(st.list_folders().unwrap()[0].meeting_count, 1);
    }

    #[test]
    fn tags_and_shares() {
        let st = Store::open_in_memory().unwrap();
        let m = Meeting::new_recording("call", None);
        st.create_meeting(&m).unwrap();
        let t = st.tag_meeting(&m.id, " Sales ").unwrap();
        assert_eq!(t.name, "Sales");
        let same = st.tag_meeting(&m.id, "sales").unwrap();
        assert_eq!(same.id, t.id);
        assert_eq!(st.list_tags().unwrap().len(), 1);
        assert_eq!(st.list_meetings_filtered(&MeetingFilter { folder: None, tag_id: Some(t.id.clone()) }).unwrap().len(), 1);
        st.untag_meeting(&m.id, &t.id).unwrap();
        assert!(st.list_tags().unwrap().is_empty(), "orphan tags are removed");

        let s = st.create_share(&m.id, "clip", Some(1000), Some(5000)).unwrap();
        assert_eq!(s.token.len(), 32);
        assert!(st.get_share(&s.token).unwrap().is_some());
        st.set_share_enabled(&s.token, false).unwrap();
        assert!(st.get_share(&s.token).unwrap().is_none());
        st.delete_share(&s.token).unwrap();
        assert!(st.list_shares().unwrap().is_empty());
    }
}
