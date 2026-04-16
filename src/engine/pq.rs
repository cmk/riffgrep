//! Product Quantization (PQ) for accelerated similarity search.
//!
//! Compresses 512-dim f32 embeddings into 128-byte codes using 128
//! sub-quantizers with 256 centroids each (4 dims per sub-space).
//! Search uses Asymmetric Distance Computation (ADC): the query stays
//! at full precision while candidates are compared via table lookups.
//!
//! The codebook is trained offline (Python/FAISS) and stored in the
//! SQLite `metadata` table. PQ codes are built in memory from the
//! stored full-precision vectors — they are ephemeral, not persisted.

use rayon::prelude::*;

/// Number of sub-quantizers (sub-spaces).
pub const M: usize = 128;

/// Number of centroids per sub-quantizer.
pub const K: usize = 256;

/// Dimensionality of each sub-space (512 / 128 = 4).
pub const DSUB: usize = 4;

/// Full embedding dimensionality.
pub const DIM: usize = M * DSUB;

/// A trained Product Quantizer codebook.
///
/// Shape: `centroids[m][k]` is a `[f32; DSUB]` centroid vector for
/// sub-quantizer `m`, centroid index `k`.
pub struct ProductQuantizer {
    /// `[M][K][DSUB]` centroid vectors, stored flat for cache locality.
    /// Layout: `centroids[(m * K + k) * DSUB .. +DSUB]`.
    centroids: Vec<f32>,
}

/// Serialized codebook size: M × K × DSUB × 4 bytes = 512 KB.
pub const CODEBOOK_BYTES: usize = M * K * DSUB * 4;

impl ProductQuantizer {
    /// Load a codebook from a serialized little-endian f32 blob.
    ///
    /// Expected size: 512 KB (128 × 256 × 4 × f32).
    pub fn from_bytes(data: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(
            data.len() == CODEBOOK_BYTES,
            "codebook size mismatch: expected {CODEBOOK_BYTES} bytes, got {}",
            data.len()
        );
        let centroids: Vec<f32> = data
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        Ok(Self { centroids })
    }

    /// Serialize the codebook to a little-endian f32 blob.
    #[allow(dead_code)]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.centroids
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect()
    }

    /// Get centroid vector `k` for sub-quantizer `m`.
    #[inline]
    fn centroid(&self, m: usize, k: usize) -> &[f32] {
        let offset = (m * K + k) * DSUB;
        &self.centroids[offset..offset + DSUB]
    }

    /// Encode a single embedding vector to a 128-byte PQ code.
    ///
    /// For each sub-space, finds the nearest centroid by L2 distance.
    pub fn encode(&self, vector: &[f32]) -> [u8; M] {
        assert_eq!(
            vector.len(),
            DIM,
            "PQ encode requires {DIM}-dim vector, got {}",
            vector.len()
        );
        let mut code = [0u8; M];
        for m in 0..M {
            let sub = &vector[m * DSUB..(m + 1) * DSUB];
            code[m] = self.nearest_centroid(m, sub);
        }
        code
    }

    /// Encode a batch of vectors in parallel using rayon.
    #[allow(dead_code)]
    pub fn encode_batch(&self, vectors: &[(i64, Vec<f32>)]) -> Vec<(i64, [u8; M])> {
        vectors
            .par_iter()
            .map(|(id, vec)| (*id, self.encode(vec)))
            .collect()
    }

    /// Find the nearest centroid index for a sub-vector in sub-space `m`.
    #[inline]
    fn nearest_centroid(&self, m: usize, sub: &[f32]) -> u8 {
        let mut best_idx = 0u8;
        let mut best_dist = f32::MAX;
        for k in 0..K {
            let c = self.centroid(m, k);
            let dist = l2_sq_4d(sub, c);
            if dist < best_dist {
                best_dist = dist;
                best_idx = k as u8;
            }
        }
        best_idx
    }

    /// Build the ADC distance lookup table for a query vector.
    ///
    /// Returns a flat `[f32; M * K]` table where `table[m * K + k]` is
    /// the squared L2 distance from the query's sub-vector `m` to
    /// centroid `k` of sub-quantizer `m`.
    pub fn adc_table(&self, query: &[f32]) -> Vec<f32> {
        assert_eq!(
            query.len(),
            DIM,
            "ADC table requires {DIM}-dim query, got {}",
            query.len()
        );
        let mut table = vec![0.0f32; M * K];
        for m in 0..M {
            let sub = &query[m * DSUB..(m + 1) * DSUB];
            for k in 0..K {
                table[m * K + k] = l2_sq_4d(sub, self.centroid(m, k));
            }
        }
        table
    }

    /// Compute approximate squared distance from a query (via ADC table)
    /// to a PQ code. This is the inner loop of the search — 128 table
    /// lookups and additions.
    #[inline]
    pub fn adc_distance(table: &[f32], code: &[u8; M]) -> f32 {
        let mut dist = 0.0f32;
        for m in 0..M {
            dist += table[m * K + code[m] as usize];
        }
        dist
    }

    /// Search for the top `limit` nearest codes to a query vector.
    ///
    /// Returns `(row_id, squared_distance)` pairs sorted by ascending
    /// distance. Uses rayon for parallel scanning of the code buffer.
    pub fn search(&self, query: &[f32], codes: &[(i64, [u8; M])], limit: usize) -> Vec<(i64, f32)> {
        let table = self.adc_table(query);

        let mut scored: Vec<(i64, f32)> = codes
            .par_iter()
            .map(|(id, code)| (*id, Self::adc_distance(&table, code)))
            .collect();

        // Full sort then truncate. TODO: use select_nth_unstable_by for
        // O(N) partial selection at 1.2M scale.
        scored.sort_unstable_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        scored
    }
}

/// Squared L2 distance for 4-dimensional sub-vectors (unrolled).
#[inline]
fn l2_sq_4d(a: &[f32], b: &[f32]) -> f32 {
    let d0 = a[0] - b[0];
    let d1 = a[1] - b[1];
    let d2 = a[2] - b[2];
    let d3 = a[3] - b[3];
    d0 * d0 + d1 * d1 + d2 * d2 + d3 * d3
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a simple codebook where centroid k in sub-space m has
    /// all values set to `k as f32`. This makes distance computation
    /// predictable.
    fn make_test_codebook() -> ProductQuantizer {
        let mut centroids = vec![0.0f32; M * K * DSUB];
        for m in 0..M {
            for k in 0..K {
                let offset = (m * K + k) * DSUB;
                for d in 0..DSUB {
                    centroids[offset + d] = k as f32;
                }
            }
        }
        ProductQuantizer { centroids }
    }

    #[test]
    fn codebook_roundtrip() {
        let pq = make_test_codebook();
        let bytes = pq.to_bytes();
        assert_eq!(bytes.len(), CODEBOOK_BYTES);
        let pq2 = ProductQuantizer::from_bytes(&bytes).unwrap();
        assert_eq!(pq.centroids, pq2.centroids);
    }

    #[test]
    fn codebook_wrong_size_errors() {
        let too_small = vec![0u8; CODEBOOK_BYTES - 1];
        assert!(ProductQuantizer::from_bytes(&too_small).is_err());
    }

    #[test]
    fn encode_zero_vector_maps_to_centroid_0() {
        let pq = make_test_codebook();
        let zero = vec![0.0f32; DIM];
        let code = pq.encode(&zero);
        // Centroid 0 has all values 0.0, so the zero vector should map to it.
        assert!(code.iter().all(|&c| c == 0), "expected all zeros: {code:?}");
    }

    #[test]
    fn encode_high_vector_maps_to_high_centroid() {
        let pq = make_test_codebook();
        // Vector with all values 200.0 should map to centroid 200 in each sub-space.
        let high = vec![200.0f32; DIM];
        let code = pq.encode(&high);
        assert!(code.iter().all(|&c| c == 200), "expected all 200: {code:?}");
    }

    #[test]
    fn adc_distance_self_is_zero() {
        let pq = make_test_codebook();
        let vec = vec![42.0f32; DIM];
        let code = pq.encode(&vec);
        let table = pq.adc_table(&vec);
        let dist = ProductQuantizer::adc_distance(&table, &code);
        // Not exactly zero due to quantization, but should be very small.
        assert!(dist < 1.0, "self-distance should be near zero, got {dist}");
    }

    #[test]
    fn adc_preserves_ranking() {
        let pq = make_test_codebook();

        let query = vec![10.0f32; DIM];
        let close = vec![11.0f32; DIM]; // distance ~1 per dim
        let far = vec![100.0f32; DIM]; // distance ~90 per dim

        let code_close = pq.encode(&close);
        let code_far = pq.encode(&far);
        let table = pq.adc_table(&query);

        let dist_close = ProductQuantizer::adc_distance(&table, &code_close);
        let dist_far = ProductQuantizer::adc_distance(&table, &code_far);

        assert!(
            dist_close < dist_far,
            "close={dist_close} should be < far={dist_far}"
        );
    }

    #[test]
    fn search_returns_sorted() {
        let pq = make_test_codebook();
        let query = vec![10.0f32; DIM];

        let codes: Vec<(i64, [u8; M])> = (0..100)
            .map(|i| {
                let mut v = vec![(10 + i) as f32; DIM];
                // Make some closer, some farther.
                v[0] = 10.0 + i as f32 * 0.5;
                (i as i64, pq.encode(&v))
            })
            .collect();

        let results = pq.search(&query, &codes, 10);
        assert_eq!(results.len(), 10);

        // Should be sorted by ascending distance.
        for i in 0..results.len() - 1 {
            assert!(
                results[i].1 <= results[i + 1].1,
                "not sorted at {i}: {} > {}",
                results[i].1,
                results[i + 1].1
            );
        }
    }

    #[test]
    fn encode_batch_matches_sequential() {
        let pq = make_test_codebook();
        let vectors: Vec<(i64, Vec<f32>)> = (0..10)
            .map(|i| (i as i64, vec![i as f32 * 10.0; DIM]))
            .collect();

        let batch_codes = pq.encode_batch(&vectors);
        for (id, vec) in &vectors {
            let sequential_code = pq.encode(vec);
            let batch_code = batch_codes.iter().find(|(bid, _)| bid == id).unwrap();
            assert_eq!(sequential_code, batch_code.1, "mismatch for id={id}");
        }
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;
        use std::sync::LazyLock;

        // Build the codebook once — it's 512KB and expensive to construct.
        static PQ: LazyLock<ProductQuantizer> = LazyLock::new(make_test_codebook);

        // Use a small dimensionality for proptests to keep encode() fast.
        // Full DIM=512 with 128×256 centroids is too slow for proptest.
        const TEST_DIM: usize = 32;

        fn arb_small_vec() -> impl Strategy<Value = Vec<f32>> {
            proptest::collection::vec(-10.0f32..10.0, TEST_DIM)
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(64))]

            /// ADC distance is always non-negative.
            #[test]
            fn proptest_adc_nonnegative(
                query in arb_small_vec(),
                target in arb_small_vec(),
            ) {
                // Pad to full DIM for encode compatibility.
                let mut q = vec![0.0f32; DIM];
                let mut t = vec![0.0f32; DIM];
                q[..TEST_DIM].copy_from_slice(&query);
                t[..TEST_DIM].copy_from_slice(&target);

                let code = PQ.encode(&t);
                let table = PQ.adc_table(&q);
                let dist = ProductQuantizer::adc_distance(&table, &code);
                prop_assert!(dist >= 0.0, "distance={dist}");
            }

            /// ADC ranking should roughly preserve brute-force L2 ranking.
            #[test]
            fn proptest_adc_ranking_preserved(
                query in arb_small_vec(),
                a in arb_small_vec(),
                b in arb_small_vec(),
            ) {
                let mut q = vec![0.0f32; DIM];
                let mut va = vec![0.0f32; DIM];
                let mut vb = vec![0.0f32; DIM];
                q[..TEST_DIM].copy_from_slice(&query);
                va[..TEST_DIM].copy_from_slice(&a);
                vb[..TEST_DIM].copy_from_slice(&b);

                let l2_a = crate::engine::similarity::l2_distance_sq(&q, &va);
                let l2_b = crate::engine::similarity::l2_distance_sq(&q, &vb);

                let code_a = PQ.encode(&va);
                let code_b = PQ.encode(&vb);
                let table = PQ.adc_table(&q);
                let adc_a = ProductQuantizer::adc_distance(&table, &code_a);
                let adc_b = ProductQuantizer::adc_distance(&table, &code_b);

                let margin = (l2_a + l2_b) * 0.3;
                if (l2_a - l2_b).abs() > margin && l2_a < l2_b {
                    prop_assert!(adc_a <= adc_b + margin,
                        "ranking flipped: l2({l2_a}<{l2_b}) but adc({adc_a}>{adc_b})");
                }
            }
        }
    }
}
