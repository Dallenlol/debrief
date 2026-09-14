//! Pure logic: attach diarization turns to captions and find the local user.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub start_ms: i64,
    pub end_ms: i64,
    pub cluster: i32,
}

fn overlap(a: (i64, i64), b: (i64, i64)) -> i64 {
    (a.1.min(b.1) - a.0.max(b.0)).max(0)
}

/// For each caption `(start_ms, end_ms)`, the cluster with the most overlapping
/// time, or `None` when no turn overlaps it at all.
pub fn assign_clusters(captions: &[(i64, i64)], turns: &[Turn]) -> Vec<Option<i32>> {
    captions
        .iter()
        .map(|&c| {
            let mut best: Option<(i32, i64)> = None;
            for t in turns {
                let o = overlap(c, (t.start_ms, t.end_ms));
                if o > 0 && best.map(|(_, bo)| o > bo).unwrap_or(true) {
                    best = Some((t.cluster, o));
                }
            }
            best.map(|(c, _)| c)
        })
        .collect()
}

/// Which cluster is the person at this computer? For each cluster, measure how
/// much of its speaking time the mic was louder than system audio by `margin_db`.
/// The cluster with the highest ratio wins if that ratio is >= 0.6.
///
/// `mic_db` / `sys_db` are `(timestamp_ms, level_dbfs)` series at any hop.
pub fn find_me_cluster(turns: &[Turn], mic_db: &[(i64, f32)], sys_db: &[(i64, f32)], margin_db: f32) -> Option<i32> {
    if turns.is_empty() || mic_db.is_empty() {
        return None;
    }
    let level_at = |series: &[(i64, f32)], t: i64| -> f32 {
        // Nearest sample at or before t (series is sorted by time).
        match series.binary_search_by_key(&t, |x| x.0) {
            Ok(i) => series[i].1,
            Err(0) => series[0].1,
            Err(i) => series[i - 1].1,
        }
    };
    let mut stats: std::collections::BTreeMap<i32, (i64, i64)> = Default::default(); // cluster -> (mic_dominant_ms, total_ms)
    for t in turns {
        let mut hop = 250;
        if t.end_ms - t.start_ms < hop {
            hop = (t.end_ms - t.start_ms).max(1);
        }
        let mut pos = t.start_ms;
        while pos < t.end_ms {
            let m = level_at(mic_db, pos);
            let s = if sys_db.is_empty() { -100.0 } else { level_at(sys_db, pos) };
            let e = stats.entry(t.cluster).or_default();
            e.1 += hop;
            if m > -55.0 && m > s + margin_db {
                e.0 += hop;
            }
            pos += hop;
        }
    }
    stats
        .iter()
        .map(|(c, (dom, tot))| (*c, if *tot > 0 { *dom as f64 / *tot as f64 } else { 0.0 }))
        .filter(|(_, r)| *r >= 0.6)
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .map(|(c, _)| c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: i64, e: i64, c: i32) -> Turn {
        Turn { start_ms: s, end_ms: e, cluster: c }
    }

    #[test]
    fn assigns_max_overlap_and_none_for_gaps() {
        let turns = [t(0, 1000, 0), t(1000, 3000, 1), t(5000, 6000, 0)];
        let caps = [(0, 900), (800, 2000), (3500, 4000), (2900, 3100)];
        assert_eq!(assign_clusters(&caps, &turns), vec![Some(0), Some(1), None, Some(1)]);
    }

    #[test]
    fn me_cluster_is_the_mic_dominant_one() {
        // Cluster 0 speaks 0-2s while mic is loud; cluster 1 speaks 2-4s while sys is loud.
        let turns = [t(0, 2000, 0), t(2000, 4000, 1)];
        let mic: Vec<(i64, f32)> = (0..16).map(|i| (i * 250, if i < 8 { -20.0 } else { -60.0 })).collect();
        let sys: Vec<(i64, f32)> = (0..16).map(|i| (i * 250, if i < 8 { -60.0 } else { -20.0 })).collect();
        assert_eq!(find_me_cluster(&turns, &mic, &sys, 6.0), Some(0));
        // Without system audio, whoever speaks over a live mic is "me".
        assert_eq!(find_me_cluster(&turns, &mic, &[], 6.0), Some(0));
        // A silent mic means nobody is "me".
        let quiet: Vec<(i64, f32)> = (0..16).map(|i| (i * 250, -80.0)).collect();
        assert_eq!(find_me_cluster(&turns, &quiet, &sys, 6.0), None);
    }
}
