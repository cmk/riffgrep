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

When `faiss-cpu` is importable (declared in `pyproject.toml` under
`[project].dependencies`; `importorskip` guards against environments
where the native wheel is unavailable), a second test trains a small
codebook via `embed_train.train()` and asserts the blob length plus
finite decoded centroids. A stricter centroid-value spot check is
listed as a follow-up in `doc/reviews/review-00010.md`.
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


def test_faiss_codebook_matches_layout() -> None:
    """If faiss is installed, train a tiny codebook and assert the byte
    length and decode-ability. Skipped cleanly via `importorskip` when
    faiss is absent; does not require the CLAP checkpoint so it doesn't
    use the `requires_clap_model` marker."""
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


def test_faiss_decode_matches_python_reshape() -> None:
    """T2 — cross-check that FAISS's own decoder agrees with the Python
    `_reference_decode` layout on a hand-crafted code.

    Critical detail: we must serialize and decode from the *same* PQ
    object. An earlier draft trained two independent FAISS PQs (one
    for serialization via `embed_train.train()`, one for the decode
    comparison) and compared their outputs — that test was passing by
    accident of FAISS's internal k-means RNG, not because the layouts
    actually matched. Training a fresh PQ twice with the same input
    does not produce identical centroids, so two independently-trained
    PQs can't be cross-checked meaningfully.

    Decoding the all-zero code `[0, 0, 0, ..., 0]` in FAISS should
    produce a 512-dim vector that is the concatenation of
    `centroids[m][0]` for m in 0..M. If Python's (M, K, DSUB) reshape
    doesn't match FAISS's internal (M, K, DSUB) memory order, the
    per-subquantizer centroids returned by the two decoders won't line
    up and this test catches the layout transposition — whereas
    `test_faiss_codebook_matches_layout` only checks finiteness and
    byte length and would pass under a silent transposition.
    """
    faiss = pytest.importorskip("faiss")

    rng = np.random.default_rng(0)
    vectors = rng.standard_normal((4096, 512)).astype(np.float32)

    # Single PQ for both serialization and decode — this is what makes
    # the cross-check meaningful.
    pq = faiss.ProductQuantizer(PQ_DSUB * PQ_M, PQ_M, 8)
    pq.train(np.ascontiguousarray(vectors, dtype=np.float32))

    # Serialize through the same byte path `embed_train.train()` uses,
    # but applied to *this* pq (not a freshly-trained one).
    centroids_flat = faiss.vector_to_array(pq.centroids).astype(
        "<f4", copy=False
    )
    blob = centroids_flat.tobytes()
    assert len(blob) == CODEBOOK_BYTES

    cb = _reference_decode(blob)

    zero_code = np.zeros((1, PQ_M), dtype=np.uint8)
    faiss_decoded = pq.decode(zero_code).reshape(PQ_DSUB * PQ_M)

    python_decoded = np.concatenate([cb[m, 0] for m in range(PQ_M)])

    # Since serialization + decode both use the same PQ object, the
    # centroids are byte-identical — these must match exactly, not
    # within tolerance. Any drift here indicates a (M, K, DSUB) vs
    # (K, M, DSUB) or inner-dim layout mismatch between the Python
    # reshape and FAISS's native representation.
    np.testing.assert_array_equal(
        faiss_decoded,
        python_decoded,
        err_msg=(
            "FAISS decode of all-zero code disagrees with the Python "
            "`_reference_decode` at centroid-0-per-subquantizer. This "
            "means the (M, K, DSUB) C-order reshape in _reference_decode "
            "is not the actual layout FAISS uses; Rust's "
            "`pq::ProductQuantizer::from_bytes` would misinterpret the "
            "byte stream we serialize here."
        ),
    )
