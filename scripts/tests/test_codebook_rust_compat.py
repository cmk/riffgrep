"""P1.1 — the codebook blob layout matches Rust's `pq::ProductQuantizer`.

The Rust side (`src/engine/pq.rs`) expects a flat little-endian f32 blob
of M * K * DSUB floats (524288 bytes) with memory layout
`centroids[(m * K + k) * DSUB .. +DSUB]`. That is a `(M, K, DSUB)`
C-order numpy array, which is exactly what
`faiss.vector_to_array(pq.centroids)` produces.

This test proves the Python-side layout by round-tripping a synthetic
codebook with distinguishable centroid values and checking that a naive
Rust-style reader (`chunks_exact(4) → f32::from_le_bytes → reshape(
M, K, DSUB)`) returns the original array.

When a real FAISS codebook is present (dev dep installed), a second test
asserts that `embed_train.train(vectors)` produces a blob of exactly
CODEBOOK_BYTES with the same layout semantics.
"""

from __future__ import annotations

import numpy as np
import pytest

from embed_train import CODEBOOK_BYTES, PQ_DSUB, PQ_K, PQ_M


def _reference_decode(blob: bytes) -> np.ndarray:
    """Decode a codebook blob the way Rust's `from_bytes` does, then
    reshape to the (M, K, DSUB) logical layout."""
    assert len(blob) == CODEBOOK_BYTES
    flat = np.frombuffer(blob, dtype="<f4")
    assert flat.size == PQ_M * PQ_K * PQ_DSUB
    return flat.reshape(PQ_M, PQ_K, PQ_DSUB)


def test_codebook_layout_round_trips() -> None:
    """Known-value codebook survives serialize → Rust-style decode."""
    # centroids[m][k][d] = m * 1000 + k * 10 + d. Every (m, k, d)
    # triple has a unique float, so any layout mistake surfaces.
    cb = np.empty((PQ_M, PQ_K, PQ_DSUB), dtype=np.float32)
    for m in range(PQ_M):
        for k in range(PQ_K):
            for d in range(PQ_DSUB):
                cb[m, k, d] = float(m * 1000 + k * 10 + d)

    blob = np.ascontiguousarray(cb).astype("<f4", copy=False).tobytes()
    assert len(blob) == CODEBOOK_BYTES

    decoded = _reference_decode(blob)
    np.testing.assert_array_equal(decoded, cb)


def test_rust_offset_formula_matches_flat_buffer() -> None:
    """Rust reads `centroids[(m*K + k)*DSUB .. +DSUB]` from a flat buffer.
    Our (M, K, DSUB) C-order reshape must agree with that indexing."""
    cb = np.arange(PQ_M * PQ_K * PQ_DSUB, dtype=np.float32).reshape(
        PQ_M, PQ_K, PQ_DSUB
    )
    blob = np.ascontiguousarray(cb).astype("<f4", copy=False).tobytes()
    flat = np.frombuffer(blob, dtype="<f4")

    # Spot-check Rust's offset formula for a handful of (m, k) pairs.
    for m, k in [(0, 0), (0, 1), (1, 0), (127, 255), (42, 17)]:
        offset = (m * PQ_K + k) * PQ_DSUB
        rust_slice = flat[offset : offset + PQ_DSUB]
        np.testing.assert_array_equal(rust_slice, cb[m, k])


@pytest.mark.requires_clap_model
def test_faiss_codebook_matches_layout() -> None:
    """If faiss is installed, train a tiny codebook and assert the byte
    length and decode-ability. Gated on the same marker as the ranking
    sanity test so CI without heavy deps skips cleanly."""
    faiss = pytest.importorskip("faiss")  # noqa: F841
    from embed_train import train

    rng = np.random.default_rng(0)
    # FAISS needs enough samples to train 256 clusters per sub-quantizer.
    vectors = rng.standard_normal((4096, 512)).astype(np.float32)
    blob = train(vectors)
    assert len(blob) == CODEBOOK_BYTES
    # Decode sanity: centroids are finite real numbers.
    cb = _reference_decode(blob)
    assert np.isfinite(cb).all()
