//! Brute-force L2 similarity search over embedding vectors.
//!
//! Computes Euclidean distance between a query vector and a set of
//! candidate vectors, returning results ranked by ascending distance
//! (most similar first). The query file is always pinned to position 0
//! with `sim = 1.0`.

use std::path::PathBuf;

/// Embedding dimensionality (LAION-CLAP output size).
#[allow(dead_code)]
pub const EMBEDDING_DIM: usize = 512;

/// A single similarity search result.
#[derive(Debug, Clone)]
pub struct SimilarityResult {
    /// Database row ID.
    #[allow(dead_code)]
    pub id: i64,
    /// File path.
    pub path: PathBuf,
    /// L2 distance from query (0.0 = identical).
    pub dist: f32,
    /// Window-relative similarity: `1.0 - (dist / max_dist)`, in [0.0, 1.0].
    /// Relative to the current result window — not comparable across queries
    /// with different `limit` values. Use `dist` for absolute comparisons.
    /// The query file itself is always 1.0.
    pub sim: f32,
}

/// Squared L2 (Euclidean) distance between two vectors.
///
/// Available for callers that only need ranking (sqrt is monotonic so
/// squared distance preserves order). The main search path uses
/// [`l2_distance`] for human-readable `dist` values.
pub fn l2_distance_sq(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| {
            let d = x - y;
            d * d
        })
        .sum()
}

/// L2 (Euclidean) distance between two vectors.
pub fn l2_distance(a: &[f32], b: &[f32]) -> f32 {
    l2_distance_sq(a, b).sqrt()
}

/// Compute similarity score from distance and max distance.
///
/// Returns `1.0 - (dist / max_dist)`, clamped to [0.0, 1.0].
/// If `max_dist` is 0 (all candidates are identical to query), returns 1.0.
pub fn similarity_score(dist: f32, max_dist: f32) -> f32 {
    if max_dist <= 0.0 {
        return 1.0;
    }
    (1.0 - (dist / max_dist)).clamp(0.0, 1.0)
}

/// Search for the most similar vectors to `query` among `candidates`.
///
/// Returns up to `limit` results sorted by ascending distance (most
/// similar first). The query itself (identified by `query_id`) is
/// pinned to position 0 with `sim = 1.0`.
///
/// Each candidate is `(row_id, embedding_vector)`.
pub fn search_similar(
    query_id: i64,
    query: &[f32],
    candidates: &[(i64, PathBuf, Vec<f32>)],
    limit: usize,
) -> Vec<SimilarityResult> {
    // Compute distances.
    let mut scored: Vec<(i64, PathBuf, f32)> = candidates
        .iter()
        .filter(|(id, _, _)| *id != query_id)
        .map(|(id, path, vec)| (*id, path.clone(), l2_distance(query, vec)))
        .collect();

    // Sort by ascending distance.
    scored.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    // Truncate to limit (minus 1 for the subject).
    scored.truncate(limit.saturating_sub(1));

    // Compute max distance for sim scaling.
    let max_dist = scored.last().map(|r| r.2).unwrap_or(0.0);

    // Build results: subject first, then ranked candidates.
    let mut results = Vec::with_capacity(scored.len() + 1);

    // Find subject's path.
    let subject_path = candidates
        .iter()
        .find(|(id, _, _)| *id == query_id)
        .map(|(_, p, _)| p.clone())
        .unwrap_or_default();

    results.push(SimilarityResult {
        id: query_id,
        path: subject_path,
        dist: 0.0,
        sim: 1.0,
    });

    for (id, path, dist) in scored {
        let sim = similarity_score(dist, max_dist);
        results.push(SimilarityResult {
            id,
            path,
            dist,
            sim,
        });
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn vec_of(vals: &[f32]) -> Vec<f32> {
        let mut v = vec![0.0f32; EMBEDDING_DIM];
        for (i, &val) in vals.iter().enumerate() {
            if i < EMBEDDING_DIM {
                v[i] = val;
            }
        }
        v
    }

    fn path(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    // --- l2_distance ---

    #[test]
    fn test_l2_distance_zero() {
        let a = vec![1.0, 2.0, 3.0];
        assert_eq!(l2_distance(&a, &a), 0.0);
    }

    #[test]
    fn test_l2_distance_known() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let d = l2_distance(&a, &b);
        assert!((d - std::f32::consts::SQRT_2).abs() < 1e-6);
    }

    #[test]
    fn test_l2_distance_symmetric() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![4.0, 5.0, 6.0];
        assert_eq!(l2_distance(&a, &b), l2_distance(&b, &a));
    }

    // --- similarity_score ---

    #[test]
    fn test_sim_identity() {
        assert_eq!(similarity_score(0.0, 10.0), 1.0);
    }

    #[test]
    fn test_sim_max_distance() {
        assert_eq!(similarity_score(10.0, 10.0), 0.0);
    }

    #[test]
    fn test_sim_midpoint() {
        let s = similarity_score(5.0, 10.0);
        assert!((s - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_sim_zero_max_dist() {
        // All candidates identical to query.
        assert_eq!(similarity_score(0.0, 0.0), 1.0);
    }

    #[test]
    fn test_sim_clamped() {
        // dist > max_dist shouldn't happen in practice but should clamp to 0.
        assert_eq!(similarity_score(15.0, 10.0), 0.0);
    }

    // --- search_similar ---

    #[test]
    fn test_search_subject_at_top() {
        let query = vec_of(&[1.0, 0.0]);
        let candidates = vec![
            (1, path("a.wav"), vec_of(&[1.0, 0.0])),
            (2, path("b.wav"), vec_of(&[0.9, 0.1])),
            (3, path("c.wav"), vec_of(&[0.0, 1.0])),
        ];
        let results = search_similar(1, &query, &candidates, 10);
        assert_eq!(results[0].id, 1);
        assert_eq!(results[0].sim, 1.0);
        assert_eq!(results[0].dist, 0.0);
    }

    #[test]
    fn test_search_ordering() {
        let query = vec_of(&[1.0, 0.0]);
        let close = vec_of(&[0.9, 0.1]);
        let far = vec_of(&[0.0, 1.0]);
        let candidates = vec![
            (1, path("query.wav"), query.clone()),
            (2, path("close.wav"), close),
            (3, path("far.wav"), far),
        ];
        let results = search_similar(1, &query, &candidates, 10);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].id, 1); // subject
        assert_eq!(results[1].id, 2); // close
        assert_eq!(results[2].id, 3); // far
        assert!(results[1].sim > results[2].sim);
    }

    #[test]
    fn test_search_limit() {
        let query = vec_of(&[1.0]);
        let candidates: Vec<_> = (0..20)
            .map(|i| {
                let mut v = vec_of(&[]);
                v[0] = i as f32 * 0.1;
                (i as i64, path(&format!("{i}.wav")), v)
            })
            .collect();
        let results = search_similar(0, &query, &candidates, 5);
        assert_eq!(results.len(), 5); // subject + 4 candidates
    }

    #[test]
    fn test_search_empty_candidates() {
        let query = vec_of(&[1.0]);
        let candidates = vec![(1, path("q.wav"), query.clone())];
        let results = search_similar(1, &query, &candidates, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].sim, 1.0);
    }

    #[test]
    fn test_search_sim_range() {
        let query = vec_of(&[1.0]);
        let candidates: Vec<_> = (0..10)
            .map(|i| {
                let mut v = vec_of(&[]);
                v[0] = i as f32;
                (i as i64, path(&format!("{i}.wav")), v)
            })
            .collect();
        // query_id=0, whose vector has v[0]=0.0, query has v[0]=1.0
        // But we want to query with the actual vector, not the stored one.
        // Use id=99 (not in candidates) to avoid confusion.
        let results = search_similar(99, &query, &candidates, 100);
        // Subject at top.
        assert_eq!(results[0].id, 99);
        assert_eq!(results[0].sim, 1.0);
        // All others in [0.0, 1.0].
        for r in &results[1..] {
            assert!(r.sim >= 0.0 && r.sim <= 1.0, "sim={} out of range", r.sim);
        }
        // Last result has sim=0.0 (furthest).
        let last = results.last().unwrap();
        assert!((last.sim - 0.0).abs() < 1e-6, "last sim={}", last.sim);
    }

    // --- proptests ---

    /// Integration test: inject synthetic embeddings into an in-memory DB,
    /// load them back out, and verify search_similar returns correct ordering.
    #[test]
    fn test_inject_and_search() {
        use crate::engine::UnifiedMetadata;
        use crate::engine::sqlite::Database;

        let db = Database::open_in_memory().unwrap();

        // Insert 10 files.
        let records: Vec<_> = (0..10)
            .map(|i| {
                let meta = UnifiedMetadata {
                    path: PathBuf::from(format!("/test/{i}.wav")),
                    ..Default::default()
                };
                (meta, 100i64, None)
            })
            .collect();
        db.insert_batch(&records).unwrap();

        // Inject embeddings: file 0 is the query vector [1,0,0,...].
        // Files 1-9 have decreasing similarity (increasing distance from file 0).
        for i in 0..10 {
            let mut v = vec![0.0f32; EMBEDDING_DIM];
            // File 0: [1, 0, 0, ...]
            // File i: [(1 - i*0.1), i*0.1, 0, 0, ...]
            v[0] = 1.0 - i as f32 * 0.1;
            v[1] = i as f32 * 0.1;
            db.insert_embedding(&format!("/test/{i}.wav"), &v).unwrap();
        }

        // Load from DB and search.
        let candidates = db.load_all_embeddings().unwrap();
        assert_eq!(candidates.len(), 10);

        // Query = file 0's vector.
        let query = vec![1.0f32; 1]
            .into_iter()
            .chain(std::iter::repeat(0.0f32))
            .take(EMBEDDING_DIM)
            .collect::<Vec<_>>();

        let query_id = candidates
            .iter()
            .find(|(_, p, _)| p.to_str() == Some("/test/0.wav"))
            .unwrap()
            .0;

        let results = search_similar(query_id, &query, &candidates, 10);

        // Subject at position 0 with sim=1.0.
        assert_eq!(results[0].id, query_id);
        assert_eq!(results[0].sim, 1.0);

        // Remaining results should be in order 1, 2, 3, ..., 9.
        for i in 1..results.len() - 1 {
            assert!(
                results[i].dist <= results[i + 1].dist,
                "results not sorted: dist[{i}]={} > dist[{}]={}",
                results[i].dist,
                i + 1,
                results[i + 1].dist
            );
        }

        // File 1 should be closest (highest sim after subject).
        assert!(results[1].sim > results[2].sim);
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        fn arb_vec(dim: usize) -> impl Strategy<Value = Vec<f32>> {
            proptest::collection::vec(-10.0f32..10.0, dim)
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(512))]

            /// L2 distance is always non-negative.
            #[test]
            fn proptest_l2_nonnegative(
                a in arb_vec(64),
                b in arb_vec(64),
            ) {
                let d = l2_distance(&a, &b);
                prop_assert!(d >= 0.0, "distance={d}");
            }

            /// L2 distance is symmetric.
            #[test]
            fn proptest_l2_symmetric(
                a in arb_vec(64),
                b in arb_vec(64),
            ) {
                let ab = l2_distance(&a, &b);
                let ba = l2_distance(&b, &a);
                prop_assert!((ab - ba).abs() < 1e-5,
                    "dist(a,b)={ab} != dist(b,a)={ba}");
            }

            /// L2 distance of a vector to itself is 0.
            #[test]
            fn proptest_l2_self_zero(a in arb_vec(64)) {
                let d = l2_distance(&a, &a);
                prop_assert!(d.abs() < 1e-6, "dist(a,a)={d}");
            }

            /// Triangle inequality: dist(a,c) <= dist(a,b) + dist(b,c).
            #[test]
            fn proptest_l2_triangle(
                a in arb_vec(32),
                b in arb_vec(32),
                c in arb_vec(32),
            ) {
                let ab = l2_distance(&a, &b);
                let bc = l2_distance(&b, &c);
                let ac = l2_distance(&a, &c);
                prop_assert!(ac <= ab + bc + 1e-4,
                    "triangle: {ac} > {ab} + {bc}");
            }

            /// Similarity score is always in [0.0, 1.0].
            #[test]
            fn proptest_sim_in_range(
                dist in 0.0f32..100.0,
                max_dist in 0.01f32..100.0,
            ) {
                let s = similarity_score(dist, max_dist);
                prop_assert!((0.0..=1.0).contains(&s), "sim={s}");
            }

            /// Closer distance → higher sim (monotonic).
            #[test]
            fn proptest_sim_monotonic(
                d1 in 0.0f32..50.0,
                d2 in 0.0f32..50.0,
                max_dist in 50.0f32..100.0,
            ) {
                let s1 = similarity_score(d1, max_dist);
                let s2 = similarity_score(d2, max_dist);
                if d1 < d2 {
                    prop_assert!(s1 >= s2, "d1={d1}<d2={d2} but s1={s1}<s2={s2}");
                }
            }
        }
    }
}
