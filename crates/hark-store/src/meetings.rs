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
        })
    }
}

const COLS: &str = "id, title, app, started_at, ended_at, duration_ms, has_video, status, folder_id, created_at";

impl Store {
    pub fn create_meeting(&self, m: &Meeting) -> Result<()> {
        self.conn.lock().execute(
            &format!("INSERT INTO meetings({COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"),
            params![m.id, m.title, m.app, m.started_at, m.ended_at, m.duration_ms, m.has_video as i64,
                m.status.as_str(), m.folder_id, m.created_at],
        )?;
        Ok(())
    }

    pub fn update_meeting(&self, m: &Meeting) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE meetings SET title=?2, app=?3, started_at=?4, ended_at=?5, duration_ms=?6, has_video=?7,
             status=?8, folder_id=?9 WHERE id=?1",
            params![m.id, m.title, m.app, m.started_at, m.ended_at, m.duration_ms, m.has_video as i64,
                m.status.as_str(), m.folder_id],
        )?;
        Ok(())
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
