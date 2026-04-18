"""P1.1 (Python side) — the codebook blob layout matches Rust's
`pq::ProductQuantizer::from_bytes` offset formula.

This test does NOT invoke Rust. The Rust side of P1.1 —
`from_bytes → to_bytes` symmetry — lives in `pq::tests::codebook_roundtrip`
in `src/engine/pq.rs`. Together these pin both halves of the format
contract:

- Rust: reading bytes as M * K * DSUB little-endian f32s and round-tripping
  through `to_bytes` is the identity.
- Python (here): the bytes we produce from our serialization path decode
  correctly when indexed with Rust's `(m * K + k) * DSUB .. +DSUB` offset
  formula. The decoder used here is a Python reimplementation of
  `pq.rs:43-53` — kept tiny and inspectable so drift is easy to spot.

The linkage is by inspection: `_reference_decode` below is a direct
transcription of Rust's `from_bytes`. If that assumption is ever in
doubt, add a Rust integration test that reads a fixture file written
by the Python path.

When `faiss-cpu` is installed (dev dep), a second test trains a small
codebook via `embed_train.train()` and asserts the blob length plus
finite decoded centroids. A stricter centroid-value spot check is
listed as a follow-up in `doc/reviews/review-0008.md`.
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
