//! Split a long recording into slices that diarize in parallel processes, then
//! stitch the per-slice results back together. Cluster ids are made unique per
//! slice; `consolidate` afterwards merges the same voice across slices.

use crate::merge::Turn;
use std::collections::BTreeMap;

/// Slices shorter than this are not worth a separate process.
pub const MIN_SHARD_MS: i64 = 5 * 60_000;
/// Overlap between neighbouring slices so a turn cut at the boundary is seen whole by one of them.
pub const OVERLAP_MS: i64 = 4_000;

/// `(start_ms, end_ms)` per slice for a recording of `duration_ms`.
pub fn plan(duration_ms: i64, workers: usize) -> Vec<(i64, i64)> {
    let workers = workers.max(1) as i64;
    let n = (duration_ms / MIN_SHARD_MS).clamp(1, workers);
    let len = duration_ms / n;
    (0..n)
        .map(|i| {
            let start = (i * len - if i > 0 { OVERLAP_MS } else { 0 }).max(0);
            let end = if i == n - 1 { duration_ms } else { (i + 1) * len };
            (start, end)
        })
        .collect()
}

/// How many slices this machine should run at once: enough to use the cores,
/// leaving each process at least two threads.
pub fn workers_for(logical_cpus: usize) -> usize {
    (logical_cpus / 4).clamp(1, 8)
}

/// Threads each slice's models get.
pub fn threads_per_worker(logical_cpus: usize, workers: usize) -> usize {
    ((logical_cpus / 2) / workers.max(1)).clamp(2, 8)
}

/// One slice's result: its `(start_ms, end_ms)` window, turns relative to the
/// window start, and per-cluster embeddings.
pub type SliceResult = ((i64, i64), Vec<Turn>, BTreeMap<i32, Vec<f32>>);

/// Merge slice results. Each slice's turns are shifted by its start, trimmed to
/// its own half of any overlap, and its cluster ids offset by `1000 * index`.
pub fn stitch(slices: Vec<SliceResult>) -> (Vec<Turn>, BTreeMap<i32, Vec<f32>>) {
    let mut turns = Vec::new();
    let mut embeddings = BTreeMap::new();
    for (i, ((start, end), slice_turns, slice_emb)) in slices.into_iter().enumerate() {
        let offset = (i as i32) * 1000;
        // Ownership is disjoint at the nominal split points: a slice that started
        // early (overlap) only keeps what lies after the previous slice's end.
        let lo = if i > 0 { start + OVERLAP_MS } else { start };
        let hi = end;
        for t in slice_turns {
            let (s, e) = (t.start_ms + start, t.end_ms + start);
            if e <= lo || s >= hi {
                continue;
            }
            turns.push(Turn { start_ms: s.max(lo), end_ms: e.min(hi), cluster: t.cluster + offset });
        }
        for (c, e) in slice_emb {
            embeddings.insert(c + offset, e);
        }
    }
    turns.sort_by_key(|t| t.start_ms);
    (turns, embeddings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_recordings_get_one_slice_and_long_ones_split_with_overlap() {
        assert_eq!(plan(3 * 60_000, 4), vec![(0, 180_000)]);
        let p = plan(60 * 60_000, 4);
        assert_eq!(p.len(), 4);
        assert_eq!(p[0], (0, 900_000));
        assert_eq!(p[1], (900_000 - OVERLAP_MS, 1_800_000));
        assert_eq!(p[3].1, 3_600_000);
        assert_eq!(plan(12 * 60_000, 4).len(), 2);
    }

    #[test]
    fn workers_follow_the_core_count() {
        assert_eq!(workers_for(4), 1);
        assert_eq!(workers_for(8), 2);
        assert_eq!(workers_for(32), 8);
        assert_eq!(workers_for(16), 4);
        assert_eq!(threads_per_worker(32, 8), 2);
        assert_eq!(threads_per_worker(8, 2), 2);
    }

    #[test]
    fn stitch_shifts_trims_and_renumbers() {
        let t = |s, e, c| Turn { start_ms: s, end_ms: e, cluster: c };
        let a = ((0, 10_000), vec![t(0, 3_000, 0), t(8_000, 10_000, 1)], BTreeMap::from([(0, vec![1.0]), (1, vec![0.0])]));
        // Slice b starts 4 s early (overlap); its first turn is the tail of a's last turn.
        let b = ((6_000, 20_000), vec![t(2_000, 4_000, 0), t(5_000, 9_000, 1)], BTreeMap::from([(0, vec![0.0]), (1, vec![0.5])]));
        let (turns, emb) = stitch(vec![a, b]);
        // a owns [0, 10 s), b owns [10 s, 20 s): the 8-10 s turn comes from a only.
        assert_eq!(turns.len(), 3);
        assert!(turns.iter().all(|t| t.end_ms > t.start_ms));
        assert_eq!(turns.iter().filter(|t| t.start_ms == 8_000).count(), 1);
        assert!(turns.iter().any(|t| t.cluster == 1001 && t.start_ms == 11_000 && t.end_ms == 15_000));
        assert_eq!(emb.len(), 4);
        assert!(emb.contains_key(&1000));
    }
}
