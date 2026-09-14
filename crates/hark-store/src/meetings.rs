use crate::{Result, Store};
use chrono::{DateTime, Utc};
use rusqlite::{params, Row};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MeetingStatus {
    Recording,
    Processing,
    Ready,
    Failed,
}

impl MeetingStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Recording => "recording",
            Self::Processing => "processing",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }
    fn parse(s: &str) -> Self {
        match s {
            "recording" => Self::Recording,
            "processing" => Self::Processing,
            "ready" => Self::Ready,
            _ => Self::Failed,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub app: Option<String>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub duration_ms: i64,
    pub has_video: bool,
    pub status: MeetingStatus,
    pub folder_id: Option<String>,
    pub created_at: DateTime<Utc>,
    /// Last processing error (cleared when a re-run succeeds).
    #[serde(default)]
    pub error: Option<String>,
    /// True until the user renames the meeting; auto-titles only replace auto titles.
    #[serde(default = "default_true")]
    pub title_auto: bool,
    /// Attendee names seen in the meeting window or calendar event.
    #[serde(default)]
    pub participants: Vec<String>,
    #[serde(default)]
    pub calendar_uid: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Meeting {
    /// A fresh meeting that is being recorded right now.
    pub fn new_recording(title: &str, app: Option<&str>) -> Meeting {
        let now = Utc::now();
        Meeting {
            id: uuid::Uuid::new_v4().to_string(),
            title: title.to_string(),
            app: app.map(str::to_string),
            started_at: now,
            ended_at: None,
            duration_ms: 0,
            has_video: false,
            status: MeetingStatus::Recording,
            folder_id: None,
            created_at: now,
            error: None,
            title_auto: true,
            participants: Vec::new(),
            calendar_uid: None,
        }
    }

    fn from_row(r: &Row) -> rusqlite::Result<Meeting> {
        Ok(Meeting {
            id: r.get("id")?,
            title: r.get("title")?,
            app: r.get("app")?,
            started_at: r.get("started_at")?,
            ended_at: r.get("ended_at")?,
            duration_ms: r.get("duration_ms")?,
            has_video: r.get::<_, i64>("has_video")? != 0,
            status: MeetingStatus::parse(&r.get::<_, String>("status")?),
            folder_id: r.get("folder_id")?,
            created_at: r.get("created_at")?,
            error: r.get("error")?,
            title_auto: r.get::<_, i64>("title_auto")? != 0,
            participants: serde_json::from_str(&r.get::<_, String>("participants_json")?).unwrap_or_default(),
            calendar_uid: r.get("calendar_uid")?,
        })
    }
}

const COLS: &str = "id, title, app, started_at, ended_at, duration_ms, has_video, status, folder_id, created_at, error, title_auto, participants_json, calendar_uid";

impl Store {
    pub fn create_meeting(&self, m: &Meeting) -> Result<()> {
        self.conn.lock().execute(
            &format!("INSERT INTO meetings({COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)"),
            params![m.id, m.title, m.app, m.started_at, m.ended_at, m.duration_ms, m.has_video as i64,
                m.status.as_str(), m.folder_id, m.created_at, m.error, m.title_auto as i64,
                serde_json::to_string(&m.participants).unwrap_or_else(|_| "[]".into()), m.calendar_uid],
        )?;
        Ok(())
    }

    pub fn update_meeting(&self, m: &Meeting) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE meetings SET title=?2, app=?3, started_at=?4, ended_at=?5, duration_ms=?6, has_video=?7,
             status=?8, folder_id=?9, error=?10, title_auto=?11, participants_json=?12, calendar_uid=?13 WHERE id=?1",
            params![m.id, m.title, m.app, m.started_at, m.ended_at, m.duration_ms, m.has_video as i64,
                m.status.as_str(), m.folder_id, m.error, m.title_auto as i64,
                serde_json::to_string(&m.participants).unwrap_or_else(|_| "[]".into()), m.calendar_uid],
        )?;
        Ok(())
    }

    pub fn set_meeting_error(&self, id: &str, error: Option<&str>) -> Result<()> {
        self.conn.lock().execute("UPDATE meetings SET error=?2 WHERE id=?1", params![id, error])?;
        Ok(())
    }

    /// Replace the attendee list (deduplicated, order kept).
    pub fn set_participants(&self, id: &str, names: &[String]) -> Result<()> {
        let mut out: Vec<String> = Vec::new();
        for n in names {
            let n = n.trim();
            if !n.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(n)) {
                out.push(n.to_string());
            }
        }
        self.conn.lock().execute(
            "UPDATE meetings SET participants_json=?2 WHERE id=?1",
            params![id, serde_json::to_string(&out).unwrap_or_else(|_| "[]".into())],
        )?;
        Ok(())
    }

    /// Set the title. `by_user` pins it so auto-titling never overwrites it again;
    /// an automatic title only lands while the meeting still has an auto title.
    pub fn set_title(&self, id: &str, title: &str, by_user: bool) -> Result<bool> {
        let n = if by_user {
            self.conn.lock().execute("UPDATE meetings SET title=?2, title_auto=0 WHERE id=?1", params![id, title])?
        } else {
            self.conn.lock().execute("UPDATE meetings SET title=?2 WHERE id=?1 AND title_auto=1", params![id, title])?
        };
        Ok(n > 0)
    }

    pub fn get_meeting(&self, id: &str) -> Result<Option<Meeting>> {
        let conn = self.conn.lock();
        let mut st = conn.prepare(&format!("SELECT {COLS} FROM meetings WHERE id=?1"))?;
        let mut rows = st.query(params![id])?;
        Ok(match rows.next()? {
            Some(r) => Some(Meeting::from_row(r)?),
            None => None,
        })
    }

    /// Newest first.
    pub fn list_meetings(&self) -> Result<Vec<Meeting>> {
        let conn = self.conn.lock();
        let mut st = conn.prepare(&format!("SELECT {COLS} FROM meetings ORDER BY started_at DESC"))?;
        let rows = st.query_map([], Meeting::from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn delete_meeting(&self, id: &str) -> Result<()> {
        self.conn.lock().execute("DELETE FROM meetings WHERE id=?1", params![id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_participants_and_title_pinning() {
        let s = Store::open_in_memory().unwrap();
        let m = Meeting::new_recording("Zoom call", Some("zoom"));
        s.create_meeting(&m).unwrap();
        s.set_meeting_error(&m.id, Some("summary: boom")).unwrap();
        s.set_participants(&m.id, &["Ann".into(), " ann ".into(), "Bob".into()]).unwrap();
        assert!(s.set_title(&m.id, "Auto title", false).unwrap());
        let g = s.get_meeting(&m.id).unwrap().unwrap();
        assert_eq!(g.error.as_deref(), Some("summary: boom"));
        assert_eq!(g.participants, vec!["Ann", "Bob"]);
        assert_eq!(g.title, "Auto title");
        assert!(g.title_auto);
        assert!(s.set_title(&m.id, "Mine", true).unwrap());
        assert!(!s.set_title(&m.id, "Auto again", false).unwrap());
        let g = s.get_meeting(&m.id).unwrap().unwrap();
        assert_eq!(g.title, "Mine");
        assert!(!g.title_auto);
    }

    #[test]
    fn create_list_get_delete_meeting() {
        let s = Store::open_in_memory().unwrap();
        let m = Meeting::new_recording("Zoom call", Some("zoom"));
        s.create_meeting(&m).unwrap();
        assert_eq!(s.list_meetings().unwrap().len(), 1);
        let got = s.get_meeting(&m.id).unwrap().unwrap();
        assert_eq!(got.title, "Zoom call");
        assert_eq!(got.status, MeetingStatus::Recording);
        s.delete_meeting(&m.id).unwrap();
        assert!(s.get_meeting(&m.id).unwrap().is_none());
    }

    #[test]
    fn update_persists_status_and_duration() {
        let s = Store::open_in_memory().unwrap();
        let mut m = Meeting::new_recording("x", None);
        s.create_meeting(&m).unwrap();
        m.status = MeetingStatus::Ready;
        m.duration_ms = 4200;
        m.has_video = true;
        s.update_meeting(&m).unwrap();
        let got = s.get_meeting(&m.id).unwrap().unwrap();
        assert_eq!(got.status, MeetingStatus::Ready);
        assert_eq!(got.duration_ms, 4200);
        assert!(got.has_video);
    }
}
