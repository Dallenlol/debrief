//! Chunk embeddings for semantic retrieval, plus BM25 + vector rank fusion.

use crate::ai::ChunkHit;
use crate::{Result, Store};
use rusqlite::params;

fn blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn unblob(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect()
}

/// Cosine similarity; vectors are expected to be L2-normalised already, so
/// this is just a dot product with a guard for mismatched sizes.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Reciprocal rank fusion (k = 60) of two ranked lists keyed by (meeting, start).
pub fn fuse_hits(a: Vec<ChunkHit>, b: Vec<ChunkHit>, limit: usize) -> Vec<ChunkHit> {
    const K: f64 = 60.0;
    let mut scored: Vec<(f64, ChunkHit)> = Vec::new();
    for list in [a, b] {
        for (i, hit) in list.into_iter().enumerate() {
            let s = 1.0 / (K + i as f64 + 1.0);
            match scored.iter_mut().find(|(_, h)| h.meeting_id == hit.meeting_id && h.start_ms == hit.start_ms) {
                Some(e) => e.0 += s,
                None => scored.push((s, hit)),
            }
        }
    }
    scored.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap_or(std::cmp::Ordering::Equal));
    scored
        .into_iter()
        .take(limit)
        .map(|(s, mut h)| {
            h.rank = -s;
            h
        })
        .collect()
}

impl Store {
    pub fn set_chunk_vectors(&self, rows: &[(i64, Vec<f32>)]) -> Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        {
            let mut st = tx.prepare("INSERT OR REPLACE INTO chunk_vectors(chunk_id, dim, vec) VALUES (?1, ?2, ?3)")?;
            for (id, v) in rows {
                st.execute(params![id, v.len() as i64, blob(v)])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Chunks of a meeting that have no embedding yet: (chunk_id, text).
    pub fn chunks_without_vectors(&self, meeting_id: &str) -> Result<Vec<(i64, String)>> {
        let conn = self.conn.lock();
        let mut st = conn.prepare(
            "SELECT c.id, c.text FROM chunks c LEFT JOIN chunk_vectors v ON v.chunk_id = c.id
             WHERE c.meeting_id = ?1 AND v.chunk_id IS NULL ORDER BY c.start_ms",
        )?;
        let rows = st.query_map(params![meeting_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn has_chunk_vectors(&self, meeting_id: &str) -> Result<bool> {
        let n: i64 = self.conn.lock().query_row(
            "SELECT COUNT(*) FROM chunk_vectors v JOIN chunks c ON c.id = v.chunk_id WHERE c.meeting_id = ?1",
            params![meeting_id],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    /// Nearest chunks by cosine similarity. Brute force over the scope.
    // fable: linear scan is fine to ~200k chunks (a few thousand hours of meetings); add an ANN index past that.
    pub fn search_chunks_vec(&self, query: &[f32], scope: Option<&str>, limit: usize) -> Result<Vec<ChunkHit>> {
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock();
        let mut st = conn.prepare(
            "SELECT c.meeting_id, m.title, m.started_at, c.start_ms, c.end_ms, c.text, v.vec
             FROM chunk_vectors v JOIN chunks c ON c.id = v.chunk_id JOIN meetings m ON m.id = c.meeting_id
             WHERE (?1 IS NULL OR c.meeting_id = ?1) AND v.dim = ?2",
        )?;
        let rows = st.query_map(params![scope, query.len() as i64], |r| {
            let v = unblob(&r.get::<_, Vec<u8>>(6)?);
            Ok(ChunkHit {
                meeting_id: r.get(0)?,
                meeting_title: r.get(1)?,
                started_at: r.get(2)?,
                start_ms: r.get(3)?,
                end_ms: r.get(4)?,
                text: r.get(5)?,
                rank: -(cosine(query, &v) as f64),
            })
        })?;
        let mut hits = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        hits.sort_by(|a, b| a.rank.partial_cmp(&b.rank).unwrap_or(std::cmp::Ordering::Equal));
        hits.truncate(limit);
        Ok(hits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Meeting, NewSegment};

    fn hit(m: &str, start: i64) -> ChunkHit {
        ChunkHit { meeting_id: m.into(), meeting_title: "t".into(), started_at: chrono::Utc::now(), start_ms: start, end_ms: start + 1, text: String::new(), rank: 0.0 }
    }

    #[test]
    fn rrf_interleaves_and_boosts_shared_hits() {
        let a = vec![hit("m", 0), hit("m", 100), hit("m", 200)];
        let b = vec![hit("m", 300), hit("m", 100)];
        let f = fuse_hits(a, b, 10);
        assert_eq!(f[0].start_ms, 100); // in both lists
        assert_eq!(f.len(), 4);
        assert_eq!(fuse_hits(vec![hit("m", 0)], vec![], 1).len(), 1);
    }

    #[test]
    fn vector_search_ranks_nearest_first() {
        let s = Store::open_in_memory().unwrap();
        let m = Meeting::new_recording("m", None);
        s.create_meeting(&m).unwrap();
        s.replace_segments(
            &m.id,
            &[
                NewSegment { start_ms: 0, end_ms: 1000, speaker: None, text: "pricing".into() },
                NewSegment { start_ms: 60_000, end_ms: 61_000, speaker: None, text: "hiring".into() },
            ],
        )
        .unwrap();
        s.rebuild_chunks(&m.id).unwrap();
        let todo = s.chunks_without_vectors(&m.id).unwrap();
        assert_eq!(todo.len(), 2);
        assert!(!s.has_chunk_vectors(&m.id).unwrap());
        s.set_chunk_vectors(&[(todo[0].0, vec![1.0, 0.0]), (todo[1].0, vec![0.0, 1.0])]).unwrap();
        assert!(s.has_chunk_vectors(&m.id).unwrap());
        assert!(s.chunks_without_vectors(&m.id).unwrap().is_empty());
        let hits = s.search_chunks_vec(&[0.1, 0.9], Some(&m.id), 5).unwrap();
        assert_eq!(hits[0].text, "hiring");
        assert_eq!(hits.len(), 2);
        assert!(s.search_chunks_vec(&[1.0, 0.0, 0.0], None, 5).unwrap().is_empty()); // dim mismatch
        // Rebuilding chunks drops stale vectors (cascade).
        s.rebuild_chunks(&m.id).unwrap();
        assert!(!s.has_chunk_vectors(&m.id).unwrap());
    }
}
