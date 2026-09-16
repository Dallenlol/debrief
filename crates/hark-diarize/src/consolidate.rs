//! Tame over-segmented diarization: long, noisy calls can come back as
//! hundreds of clusters. Clusters with very little speech are folded into the
//! most similar voice, then the closest pairs are merged until the count is
//! plausible for a meeting.

use crate::identify::cosine;
use crate::merge::Turn;
use std::collections::BTreeMap;

/// More distinct voices than this in one meeting is almost always noise.
pub const MAX_SPEAKERS: usize = 8;
/// A cluster with less speech than this (ms) is merged into its nearest voice.
pub const MIN_CLUSTER_MS: i64 = 8_000;
/// Pairs at least this similar are merged even below the cap.
pub const MERGE_SIMILARITY: f32 = 0.75;

/// Remap `turns` in place and merge `embeddings` (duration-weighted) so that
/// at most `max_speakers` clusters remain. Clusters without an embedding are
/// merged into the largest cluster when they are tiny, else kept.
pub fn consolidate(turns: &mut [Turn], embeddings: &mut BTreeMap<i32, Vec<f32>>, max_speakers: usize) {
    let mut dur: BTreeMap<i32, i64> = BTreeMap::new();
    for t in turns.iter() {
        *dur.entry(t.cluster).or_insert(0) += (t.end_ms - t.start_ms).max(0);
    }
    if dur.len() <= 1 {
        return;
    }
    // cluster id -> the id it was merged into (chains resolved at the end).
    let mut remap: BTreeMap<i32, i32> = dur.keys().map(|&c| (c, c)).collect();

    let nearest = |c: i32, dur: &BTreeMap<i32, i64>, embeddings: &BTreeMap<i32, Vec<f32>>| -> Option<i32> {
        let e = embeddings.get(&c)?;
        dur.keys()
            .filter(|&&o| o != c)
            .filter_map(|&o| embeddings.get(&o).map(|oe| (o, cosine(e, oe))))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(o, _)| o)
    };
    let largest = |dur: &BTreeMap<i32, i64>| dur.iter().max_by_key(|(_, d)| **d).map(|(c, _)| *c);

    // 1. Fold tiny clusters into their nearest voice (or the largest cluster).
    let mut tiny: Vec<i32> = dur.iter().filter(|(_, d)| **d < MIN_CLUSTER_MS).map(|(c, _)| *c).collect();
    tiny.sort_by_key(|c| dur[c]);
    for c in tiny {
        if dur.len() <= 1 {
            break;
        }
        let target = nearest(c, &dur, embeddings).or_else(|| largest(&dur).filter(|l| *l != c));
        if let Some(t) = target {
            merge_into(c, t, &mut dur, embeddings, &mut remap);
        }
    }
    // 2. Merge the most similar pairs while there are too many, or while a pair is near-identical.
    loop {
        if dur.len() <= 1 {
            break;
        }
        let ids: Vec<i32> = dur.keys().copied().collect();
        let mut best: Option<(i32, i32, f32)> = None;
        for (i, &a) in ids.iter().enumerate() {
            let Some(ea) = embeddings.get(&a) else { continue };
            for &b in &ids[i + 1..] {
                let Some(eb) = embeddings.get(&b) else { continue };
                let s = cosine(ea, eb);
                if best.map(|(_, _, bs)| s > bs).unwrap_or(true) {
                    best = Some((a, b, s));
                }
            }
        }
        let Some((a, b, s)) = best else { break };
        if dur.len() > max_speakers || s >= MERGE_SIMILARITY {
            let (from, into) = if dur[&a] >= dur[&b] { (b, a) } else { (a, b) };
            merge_into(from, into, &mut dur, embeddings, &mut remap);
        } else {
            break;
        }
    }
    // Resolve chains and rewrite the turns.
    let resolve = |mut c: i32| {
        for _ in 0..remap.len() {
            let n = remap[&c];
            if n == c {
                break;
            }
            c = n;
        }
        c
    };
    for t in turns.iter_mut() {
        t.cluster = resolve(t.cluster);
    }
}

fn merge_into(from: i32, into: i32, dur: &mut BTreeMap<i32, i64>, embeddings: &mut BTreeMap<i32, Vec<f32>>, remap: &mut BTreeMap<i32, i32>) {
    let (df, di) = (dur[&from].max(1) as f32, dur[&into].max(1) as f32);
    if let (Some(ef), Some(ei)) = (embeddings.get(&from).cloned(), embeddings.get(&into).cloned()) {
        if ef.len() == ei.len() {
            let mut m: Vec<f32> = ef.iter().zip(&ei).map(|(a, b)| (a * df + b * di) / (df + di)).collect();
            let n = m.iter().map(|x| x * x).sum::<f32>().sqrt();
            if n > 0.0 {
                m.iter_mut().for_each(|x| *x /= n);
            }
            embeddings.insert(into, m);
        }
    }
    embeddings.remove(&from);
    let d = dur.remove(&from).unwrap_or(0);
    *dur.entry(into).or_insert(0) += d;
    remap.insert(from, into);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(s: i64, e: i64, c: i32) -> Turn {
        Turn { start_ms: s, end_ms: e, cluster: c }
    }

    #[test]
    fn tiny_clusters_fold_into_their_nearest_voice() {
        // Two real voices (0 and 1) plus a 2 s blip whose embedding matches voice 1.
        let mut turns = vec![turn(0, 30_000, 0), turn(30_000, 60_000, 1), turn(60_000, 62_000, 2)];
        let mut emb = BTreeMap::from([(0, vec![1.0, 0.0]), (1, vec![0.0, 1.0]), (2, vec![0.1, 0.99])]);
        consolidate(&mut turns, &mut emb, 8);
        assert_eq!(turns[2].cluster, 1);
        assert_eq!(emb.len(), 2);
    }

    #[test]
    fn caps_the_number_of_speakers_by_merging_closest_pairs() {
        let mut turns: Vec<Turn> = (0..12i64).map(|i| turn(i * 20_000, i * 20_000 + 20_000, i as i32)).collect();
        let mut emb: BTreeMap<i32, Vec<f32>> = (0..12i32)
            .map(|i| {
                let a = (i % 3) as f32; // three underlying voices with jitter
                let v = [(a * 1.2).cos() + 0.01 * i as f32, (a * 1.2).sin()];
                let n = (v[0] * v[0] + v[1] * v[1]).sqrt();
                (i, vec![v[0] / n, v[1] / n])
            })
            .collect();
        consolidate(&mut turns, &mut emb, 4);
        let distinct: std::collections::BTreeSet<i32> = turns.iter().map(|t| t.cluster).collect();
        assert!(distinct.len() <= 4, "{distinct:?}");
        assert_eq!(emb.len(), distinct.len());
    }

    #[test]
    fn distinct_voices_are_left_alone() {
        let mut turns = vec![turn(0, 30_000, 0), turn(30_000, 60_000, 1)];
        let mut emb = BTreeMap::from([(0, vec![1.0, 0.0]), (1, vec![0.0, 1.0])]);
        consolidate(&mut turns, &mut emb, 8);
        assert_eq!(turns.iter().map(|t| t.cluster).collect::<Vec<_>>(), vec![0, 1]);
    }
}
