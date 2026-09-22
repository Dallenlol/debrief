//! Speakers known across meetings, and the per-meeting label -> speaker links.

use crate::{Result, Store};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Speaker {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub embedding: Vec<f32>,
}

/// How much one speaker label actually says in a meeting, for the speaker list.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SpeakerStat {
    pub label: String,
    pub segments: i64,
    /// Total speaking time in milliseconds.
    pub ms: i64,
    pub first_ms: i64,
}

/// What a rename actually did, so the UI can say "merged" rather than "renamed".
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RenameOutcome {
    /// The person this label now belongs to. `None` when the label is still a
    /// placeholder ("Speaker 2"), which is not worth remembering as a person.
    pub speaker: Option<Speaker>,
    /// The label every affected segment now carries.
    pub name: String,
    /// The label that disappeared, when the new name already labelled other
    /// segments in this meeting (i.e. two detected voices became one person).
    pub merged_from: Option<String>,
    /// Segments that changed label.
    pub moved_segments: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MeetingSpeaker {
    pub meeting_id: String,
    /// Label as it appears in `segments.speaker` ("Speaker 1", "Me", or a name once renamed).
    pub label: String,
    pub speaker_id: Option<String>,
    pub suggested_id: Option<String>,
    pub suggested_name: Option<String>,
    pub suggested_score: Option<f32>,
}

fn blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// "Speaker 1", "Speaker 12" - a diarization placeholder, not someone's name.
pub fn is_placeholder(name: &str) -> bool {
    let rest = match name.trim().strip_prefix("Speaker ").or_else(|| name.trim().strip_prefix("speaker ")) {
        Some(r) => r.trim(),
        None => return false,
    };
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

fn unblob(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect()
}

impl Store {
    pub fn list_speakers(&self) -> Result<Vec<Speaker>> {
        let conn = self.conn.lock();
        let mut st = conn.prepare("SELECT id, name, embedding FROM speakers ORDER BY name")?;
        let rows = st.query_map([], |r| {
            Ok(Speaker { id: r.get(0)?, name: r.get(1)?, embedding: unblob(&r.get::<_, Vec<u8>>(2)?) })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn upsert_speaker(&self, s: &Speaker) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO speakers(id, name, embedding, dim, created_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, embedding = excluded.embedding, dim = excluded.dim",
            params![s.id, s.name, blob(&s.embedding), s.embedding.len() as i64, chrono::Utc::now()],
        )?;
        Ok(())
    }

    pub fn delete_speaker(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("UPDATE meeting_speakers SET speaker_id = NULL WHERE speaker_id = ?1", params![id])?;
        conn.execute("UPDATE meeting_speakers SET suggested_id = NULL, suggested_score = NULL WHERE suggested_id = ?1", params![id])?;
        conn.execute("DELETE FROM speakers WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn meeting_speakers(&self, meeting_id: &str) -> Result<Vec<MeetingSpeaker>> {
        let conn = self.conn.lock();
        let mut st = conn.prepare(
            "SELECT ms.meeting_id, ms.label, ms.speaker_id, ms.suggested_id, s.name, ms.suggested_score
             FROM meeting_speakers ms LEFT JOIN speakers s ON s.id = ms.suggested_id
             WHERE ms.meeting_id = ?1 ORDER BY ms.label",
        )?;
        let rows = st.query_map(params![meeting_id], |r| {
            Ok(MeetingSpeaker {
                meeting_id: r.get(0)?,
                label: r.get(1)?,
                speaker_id: r.get(2)?,
                suggested_id: r.get(3)?,
                suggested_name: r.get(4)?,
                suggested_score: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_meeting_speakers(&self, meeting_id: &str, rows: &[MeetingSpeaker]) -> Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM meeting_speakers WHERE meeting_id = ?1", params![meeting_id])?;
        for r in rows {
            tx.execute(
                "INSERT INTO meeting_speakers(meeting_id, label, speaker_id, suggested_id, suggested_score) VALUES (?1,?2,?3,?4,?5)",
                params![meeting_id, r.label, r.speaker_id, r.suggested_id, r.suggested_score],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Rename a speaker label everywhere in one meeting and link it to a known
    /// speaker (created if no speaker with `name` exists). `embedding` (if given)
    /// becomes / refreshes that speaker's voiceprint.
    /// Naming a label the same as another label in the meeting merges the two:
    /// every segment ends up under one name and one `meeting_speakers` row.
    pub fn rename_speaker(&self, meeting_id: &str, label: &str, name: &str, embedding: Option<&[f32]>) -> Result<RenameOutcome> {
        let name = name.trim();
        if name.is_empty() {
            return Err(crate::StoreError::Other("name cannot be empty".into()));
        }
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let existing: Option<(String, String, Vec<u8>)> = tx
            .query_row("SELECT id, name, embedding FROM speakers WHERE name = ?1 COLLATE NOCASE", params![name], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?;
        // Reuse the canonical spelling of an existing speaker.
        let name: &str = existing.as_ref().map(|e| e.1.as_str()).unwrap_or(name);
        // Merging into a still-unnamed row is fine, but "Speaker 2" is not a person:
        // rewrite the labels without teaching voice memory a bogus name.
        let speaker = if is_placeholder(name) {
            None
        } else {
            Some(match existing.clone() {
            Some((id, _, old)) => {
                let emb = match embedding {
                    Some(e) if !e.is_empty() => {
                        // Blend old and new voiceprints so the speaker improves over time.
                        let old_v = unblob(&old);
                        if old_v.len() == e.len() {
                            old_v.iter().zip(e).map(|(a, b)| (a + b) / 2.0).collect()
                        } else {
                            e.to_vec()
                        }
                    }
                    _ => unblob(&old),
                };
                tx.execute("UPDATE speakers SET embedding = ?2, dim = ?3 WHERE id = ?1", params![id, blob(&emb), emb.len() as i64])?;
                Speaker { id, name: name.to_string(), embedding: emb }
            }
            None => {
                let s = Speaker { id: uuid::Uuid::new_v4().to_string(), name: name.to_string(), embedding: embedding.map(|e| e.to_vec()).unwrap_or_default() };
                tx.execute(
                    "INSERT INTO speakers(id, name, embedding, dim, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![s.id, s.name, blob(&s.embedding), s.embedding.len() as i64, chrono::Utc::now()],
                )?;
                s
            }
            })
        };
        let moved_segments: i64 =
            tx.query_row("SELECT COUNT(*) FROM segments WHERE meeting_id = ?1 AND speaker = ?2", params![meeting_id, label], |r| r.get(0))?;
        // Does the target name already label other segments here? Then this is a merge.
        let target_segments: i64 = tx.query_row(
            "SELECT COUNT(*) FROM segments WHERE meeting_id = ?1 AND speaker = ?2 COLLATE NOCASE AND speaker <> ?3",
            params![meeting_id, name, label],
            |r| r.get(0),
        )?;
        let merged_from = (target_segments > 0 && !label.eq_ignore_ascii_case(name)).then(|| label.to_string());
        tx.execute("UPDATE segments SET speaker = ?3 WHERE meeting_id = ?1 AND speaker = ?2", params![meeting_id, label, name])?;
        // A merge can leave the target under a different spelling; normalise it.
        tx.execute(
            "UPDATE segments SET speaker = ?2 WHERE meeting_id = ?1 AND speaker = ?2 COLLATE NOCASE AND speaker <> ?2",
            params![meeting_id, name],
        )?;
        tx.execute(
            "INSERT INTO meeting_speakers(meeting_id, label, speaker_id) VALUES (?1, ?3, ?2)
             ON CONFLICT(meeting_id, label) DO UPDATE SET speaker_id = excluded.speaker_id, suggested_id = NULL, suggested_score = NULL",
            params![meeting_id, speaker.as_ref().map(|s| s.id.clone()), name],
        )?;
        tx.execute("DELETE FROM meeting_speakers WHERE meeting_id = ?1 AND label = ?2 AND label <> ?3", params![meeting_id, label, name])?;
        tx.commit()?;
        Ok(RenameOutcome { speaker, name: name.to_string(), merged_from, moved_segments })
    }

    /// Every speaker label in a meeting with how much it says, in first-heard order.
    pub fn speaker_stats(&self, meeting_id: &str) -> Result<Vec<SpeakerStat>> {
        let conn = self.conn.lock();
        let mut st = conn.prepare(
            "SELECT speaker, COUNT(*), SUM(MAX(end_ms - start_ms, 0)), MIN(start_ms) FROM segments
             WHERE meeting_id = ?1 AND speaker IS NOT NULL AND TRIM(speaker) <> ''
             GROUP BY speaker ORDER BY MIN(start_ms)",
        )?;
        let rows = st.query_map(params![meeting_id], |r| {
            Ok(SpeakerStat { label: r.get(0)?, segments: r.get(1)?, ms: r.get(2)?, first_ms: r.get(3)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_clean_text(&self, meeting_id: &str, updates: &[(i64, String)]) -> Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        {
            let mut st = tx.prepare("UPDATE segments SET clean_text = ?3 WHERE meeting_id = ?1 AND id = ?2")?;
            for (id, text) in updates {
                st.execute(params![meeting_id, id, text])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn clear_clean_text(&self, meeting_id: &str) -> Result<()> {
        self.conn.lock().execute("UPDATE segments SET clean_text = NULL WHERE meeting_id = ?1", params![meeting_id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Meeting, NewSegment};

    fn seg(s: i64, sp: &str, t: &str) -> NewSegment {
        NewSegment { start_ms: s, end_ms: s + 1000, speaker: Some(sp.into()), text: t.into() }
    }

    #[test]
    fn rebuilt_chunks_carry_the_new_name() {
        let st = Store::open_in_memory().unwrap();
        let m = Meeting::new_recording("m", None);
        st.create_meeting(&m).unwrap();
        st.replace_segments(&m.id, &[seg(0, "Speaker 1", "we ship friday"), seg(1000, "Speaker 2", "agreed")]).unwrap();
        st.rebuild_chunks(&m.id).unwrap();
        assert!(st.chunks(&m.id).unwrap()[0].text.contains("Speaker 1: we ship friday"));
        st.rename_speaker(&m.id, "Speaker 1", "Sarah", None).unwrap();
        st.rebuild_chunks(&m.id).unwrap();
        let text = &st.chunks(&m.id).unwrap()[0].text;
        assert!(text.contains("Sarah: we ship friday"), "{text}");
        assert!(!text.contains("Speaker 1"));
    }

    #[test]
    fn rename_applies_to_all_segments_and_creates_speaker() {
        let st = Store::open_in_memory().unwrap();
        let m = Meeting::new_recording("m", None);
        st.create_meeting(&m).unwrap();
        st.replace_segments(&m.id, &[seg(0, "Speaker 1", "a"), seg(1000, "Speaker 2", "b"), seg(2000, "Speaker 1", "c")]).unwrap();
        let out = st.rename_speaker(&m.id, "Speaker 1", "Sarah", Some(&[0.6, 0.8])).unwrap();
        let sp = out.speaker.expect("named speaker");
        assert_eq!(out.merged_from, None);
        assert_eq!(out.moved_segments, 2);
        let segs = st.segments(&m.id).unwrap();
        assert_eq!(segs.iter().filter(|s| s.speaker.as_deref() == Some("Sarah")).count(), 2);
        assert_eq!(segs[1].speaker.as_deref(), Some("Speaker 2"));
        let known = st.list_speakers().unwrap();
        assert_eq!(known.len(), 1);
        assert_eq!(known[0].embedding, vec![0.6, 0.8]);
        let ms = st.meeting_speakers(&m.id).unwrap();
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].label, "Sarah");
        assert_eq!(ms[0].speaker_id.as_deref(), Some(sp.id.as_str()));

        // Renaming another label to the same name merges the two and blends the voiceprint.
        let merge = st.rename_speaker(&m.id, "Speaker 2", "sarah", Some(&[1.0, 0.0])).unwrap();
        assert_eq!(merge.merged_from.as_deref(), Some("Speaker 2"));
        assert_eq!(merge.moved_segments, 1);
        assert_eq!(merge.speaker.as_ref().unwrap().name, "Sarah");
        assert_eq!(st.meeting_speakers(&m.id).unwrap().len(), 1);
        let stats = st.speaker_stats(&m.id).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].label, "Sarah");
        assert_eq!(stats[0].segments, 3);
        let known = st.list_speakers().unwrap();
        assert_eq!(known.len(), 1);
        assert_eq!(known[0].embedding, vec![0.8, 0.4]);
        assert!(st.segments(&m.id).unwrap().iter().all(|s| s.speaker.as_deref() == Some("Sarah")));
    }

    #[test]
    fn merging_into_a_placeholder_does_not_create_a_known_speaker() {
        let st = Store::open_in_memory().unwrap();
        let m = Meeting::new_recording("m", None);
        st.create_meeting(&m).unwrap();
        st.replace_segments(&m.id, &[seg(0, "Speaker 1", "a"), seg(1000, "Speaker 2", "b"), seg(2000, "Speaker 2", "c")]).unwrap();
        let out = st.rename_speaker(&m.id, "Speaker 2", "Speaker 1", Some(&[1.0, 0.0])).unwrap();
        assert!(out.speaker.is_none());
        assert_eq!(out.merged_from.as_deref(), Some("Speaker 2"));
        assert_eq!(out.moved_segments, 2);
        assert!(st.list_speakers().unwrap().is_empty(), "placeholders never become known people");
        let stats = st.speaker_stats(&m.id).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!((stats[0].label.as_str(), stats[0].segments), ("Speaker 1", 3));
        // Naming the merged row afterwards still works and remembers one person.
        let named = st.rename_speaker(&m.id, "Speaker 1", "Ann", Some(&[0.0, 1.0])).unwrap();
        assert_eq!(named.speaker.unwrap().name, "Ann");
        assert_eq!(st.list_speakers().unwrap().len(), 1);
        assert!(is_placeholder("Speaker 12") && !is_placeholder("Speaker Ann") && !is_placeholder("Ann"));
    }

    #[test]
    fn suggestions_and_clean_text() {
        let st = Store::open_in_memory().unwrap();
        let m = Meeting::new_recording("m", None);
        st.create_meeting(&m).unwrap();
        st.replace_segments(&m.id, &[seg(0, "Speaker 1", "um hello there")]).unwrap();
        st.upsert_speaker(&Speaker { id: "s1".into(), name: "Ann".into(), embedding: vec![1.0] }).unwrap();
        st.set_meeting_speakers(
            &m.id,
            &[MeetingSpeaker { meeting_id: m.id.clone(), label: "Speaker 1".into(), speaker_id: None, suggested_id: Some("s1".into()), suggested_name: None, suggested_score: Some(0.8) }],
        )
        .unwrap();
        let ms = st.meeting_speakers(&m.id).unwrap();
        assert_eq!(ms[0].suggested_name.as_deref(), Some("Ann"));
        let id = st.segments(&m.id).unwrap()[0].id;
        st.set_clean_text(&m.id, &[(id, "Hello there.".into())]).unwrap();
        assert_eq!(st.segments(&m.id).unwrap()[0].clean_text.as_deref(), Some("Hello there."));
        assert!(st.search("hello", 5).unwrap().len() == 1);
        st.clear_clean_text(&m.id).unwrap();
        assert!(st.segments(&m.id).unwrap()[0].clean_text.is_none());
        st.delete_speaker("s1").unwrap();
        assert!(st.meeting_speakers(&m.id).unwrap()[0].suggested_id.is_none());
    }
}
