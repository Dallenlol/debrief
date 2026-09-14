//! Matching voice embeddings against speakers seen before.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnownSpeaker {
    pub id: String,
    pub name: String,
    pub embedding: Vec<f32>,
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

/// Mean vector, L2-normalised. Empty input gives an empty vector.
pub fn centroid(embs: &[Vec<f32>]) -> Vec<f32> {
    let Some(first) = embs.first() else { return Vec::new() };
    let dim = first.len();
    let mut acc = vec![0.0f32; dim];
    let mut n = 0usize;
    for e in embs.iter().filter(|e| e.len() == dim) {
        for (a, x) in acc.iter_mut().zip(e) {
            *a += x;
        }
        n += 1;
    }
    if n == 0 {
        return Vec::new();
    }
    let norm = acc.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
    acc.iter().map(|x| x / (n as f32) / (norm / n as f32)).collect()
}

/// Best known speaker whose cosine similarity to `emb` is at least `threshold`.
pub fn best_match<'a>(emb: &[f32], known: &'a [KnownSpeaker], threshold: f32) -> Option<(&'a KnownSpeaker, f32)> {
    known
        .iter()
        .map(|k| (k, cosine(emb, &k.embedding)))
        .filter(|(_, s)| *s >= threshold)
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_basics() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), 0.0);
    }

    #[test]
    fn centroid_is_unit_length_mean() {
        let c = centroid(&[vec![2.0, 0.0], vec![0.0, 2.0]]);
        assert!((cosine(&c, &[1.0, 1.0]) - 1.0).abs() < 1e-5);
        let n: f32 = c.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((n - 1.0).abs() < 1e-5);
        assert!(centroid(&[]).is_empty());
    }

    #[test]
    fn best_match_respects_threshold() {
        let known = vec![
            KnownSpeaker { id: "a".into(), name: "Ann".into(), embedding: vec![1.0, 0.0] },
            KnownSpeaker { id: "b".into(), name: "Bob".into(), embedding: vec![0.7, 0.7] },
        ];
        let (k, s) = best_match(&[0.9, 0.1], &known, 0.72).unwrap();
        assert_eq!(k.name, "Ann");
        assert!(s > 0.9);
        assert!(best_match(&[0.0, 1.0], &known, 0.95).is_none());
    }
}
