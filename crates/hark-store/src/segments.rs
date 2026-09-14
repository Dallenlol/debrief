use crate::{Result, Store};
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Segment {
    pub id: i64,
    pub meeting_id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker: Option<String>,
    pub text: String,
    pub clean_text: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct NewSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker: Option<String>,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SearchHit {
    pub meeting_id: String,
    pub meeting_title: String,
    pub segment_id: i64,
    pub start_ms: i64,
    pub snippet: String,
}

impl Store {
    /// Replace all segments of a meeting (used after each transcription pass).
    pub fn replace_segments(&self, meeting_id: &str, segs: &[NewSegment]) -> Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM segments WHERE meeting_id=?1", params![meeting_id])?;
        {
            let mut st = tx.prepare(
                "INSERT INTO segments(meeting_id, start_ms, end_ms, speaker, text) VALUES (?1,?2,?3,?4,?5)",
            )?;
            for s in segs {
                st.execute(params![meeting_id, s.start_ms, s.end_ms, s.speaker, s.text])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn segments(&self, meeting_id: &str) -> Result<Vec<Segment>> {
        let conn = self.conn.lock();
        let mut st = conn.prepare(
            "SELECT id, meeting_id, start_ms, end_ms, speaker, text, clean_text FROM segments
             WHERE meeting_id=?1 ORDER BY start_ms",
        )?;
        let rows = st.query_map(params![meeting_id], |r| {
            Ok(Segment {
                id: r.get(0)?,
                meeting_id: r.get(1)?,
                start_ms: r.get(2)?,
                end_ms: r.get(3)?,
                speaker: r.get(4)?,
                text: r.get(5)?,
                clean_text: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Full-text search over transcript segments (raw and clean text) and meeting titles.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let q = fts_query(query);
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock();
        let mut st = conn.prepare(
            "SELECT m.id, m.title, s.id, s.start_ms,
                    snippet(segments_fts, 0, '<b>', '</b>', '...', 12)
             FROM segments_fts
             JOIN segments s ON s.id = segments_fts.rowid
             JOIN meetings m ON m.id = s.meeting_id
             WHERE segments_fts MATCH ?1
             ORDER BY bm25(segments_fts) LIMIT ?2",
        )?;
        let mut hits: Vec<SearchHit> = st
            .query_map(params![q, limit as i64], |r| {
                Ok(SearchHit {
                    meeting_id: r.get(0)?,
                    meeting_title: r.get(1)?,
                    segment_id: r.get(2)?,
                    start_ms: r.get(3)?,
                    snippet: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        // Title matches (no segment).
        let mut st = conn.prepare("SELECT id, title FROM meetings WHERE title LIKE ?1 LIMIT ?2")?;
        let like = format!("%{}%", query.trim());
        let titles = st
            .query_map(params![like, limit as i64], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (id, title) in titles {
            if !hits.iter().any(|h| h.meeting_id == id) {
                hits.push(SearchHit { meeting_id: id, meeting_title: title.clone(), segment_id: 0, start_ms: 0, snippet: title });
            }
        }
        hits.truncate(limit);
        Ok(hits)
    }
}

/// Turn free text into a safe FTS5 query: each word quoted, prefix-matched, AND-ed.
fn fts_query(input: &str) -> String {
    input
        .split_whitespace()
        .map(|w| w.replace('"', ""))
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\"*"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Meeting;

    #[test]
    fn segments_roundtrip_and_fts() {
        let s = Store::open_in_memory().unwrap();
        let m = Meeting::new_recording("Budget review", None);
        s.create_meeting(&m).unwrap();
        s.replace_segments(
            &m.id,
            &[NewSegment { start_ms: 0, end_ms: 1500, speaker: None, text: "we need to cut the marketing budget".into() }],
        )
        .unwrap();
        assert_eq!(s.segments(&m.id).unwrap().len(), 1);
        let hits = s.search("marketing", 10).unwrap();
        assert_eq!(hits[0].meeting_id, m.id);
        assert!(hits[0].snippet.contains("<b>marketing</b>"));
        assert!(s.search("zebra", 10).unwrap().is_empty());
    }

    #[test]
    fn title_search_and_replace_clears_old() {
        let s = Store::open_in_memory().unwrap();
        let m = Meeting::new_recording("Quarterly planning", None);
        s.create_meeting(&m).unwrap();
        assert_eq!(s.search("quarterly", 5).unwrap().len(), 1);
        s.replace_segments(&m.id, &[NewSegment { start_ms: 0, end_ms: 1, speaker: None, text: "alpha".into() }]).unwrap();
        s.replace_segments(&m.id, &[NewSegment { start_ms: 0, end_ms: 1, speaker: None, text: "beta".into() }]).unwrap();
        assert!(s.search("alpha", 5).unwrap().is_empty());
        assert_eq!(s.search("beta", 5).unwrap().len(), 1);
    }

    #[test]
    fn fts_query_quotes_and_prefixes() {
        assert_eq!(fts_query("hello wor\"ld"), "\"hello\"* \"world\"*");
        assert_eq!(fts_query("   "), "");
    }
}
