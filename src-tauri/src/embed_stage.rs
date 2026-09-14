//! Chunk embeddings for semantic retrieval; runs after chunks are rebuilt.

use crate::state::AppState;
use tauri::{AppHandle, Manager};

/// Embed every chunk of `meeting_id` that has no vector yet. Silent no-op
/// without the embedding model. Returns the number embedded.
pub fn embed_meeting(app: &AppHandle, meeting_id: &str) -> Result<usize, String> {
    let state = app.state::<AppState>();
    let Some(engine) = state.embed() else { return Ok(0) };
    let todo = state.store.chunks_without_vectors(meeting_id).map_err(|e| e.to_string())?;
    let mut n = 0;
    for batch in todo.chunks(16) {
        let texts: Vec<&str> = batch.iter().map(|(_, t)| t.as_str()).collect();
        let vecs = engine.embed(&texts).map_err(|e| e.to_string())?;
        let rows: Vec<(i64, Vec<f32>)> = batch.iter().zip(vecs).map(|((id, _), v)| (*id, v)).collect();
        state.store.set_chunk_vectors(&rows).map_err(|e| e.to_string())?;
        n += rows.len();
    }
    Ok(n)
}

/// Hybrid retrieval: BM25 fused with vector similarity when embeddings exist.
pub fn retrieve(state: &AppState, query: &str, scope: Option<&str>, limit: usize) -> Vec<hark_store::ChunkHit> {
    let bm25 = state.store.search_chunks(query, scope, limit).unwrap_or_default();
    let vec_hits = state
        .embed()
        .and_then(|e| e.embed(&[query]).ok())
        .and_then(|v| v.into_iter().next())
        .map(|q| state.store.search_chunks_vec(&q, scope, limit).unwrap_or_default())
        .unwrap_or_default();
    if vec_hits.is_empty() {
        bm25
    } else {
        hark_store::fuse_hits(bm25, vec_hits, limit)
    }
}
