"""P2.2 — `_fetch_training_vectors` peak memory is bounded by `n_train`,
not by total embedded rows.

Guards the T1 refactor from `doc/plans/plan-2026-04-18-01.md`. The old
implementation did `SELECT id, embedding FROM samples ...` with no LIMIT
and fetched every BLOB into a Python list before sampling — at 1.2M
rows × 2048 bytes that's ~2.5 GB. The new implementation streams IDs
only, samples them with numpy, and batch-fetches just the selected
BLOBs.

Uses `tracemalloc` (stdlib) to measure peak Python allocations during
the function call.
"""

from __future__ import annotations

import sqlite3
import tracemalloc
from pathlib import Path

import numpy as np

import embed_train


def _populate(conn: sqlite3.Connection, n_rows: int) -> None:
    """Insert `n_rows` rows with random 512-dim f32 embedding blobs."""
    rng = np.random.default_rng(0)
    # Batched executemany keeps the populate step fast and its own
    # memory footprint low so it doesn't pollute tracemalloc's baseline.
    BATCH = 500
    for start in range(0, n_rows, BATCH):
        rows = []
        for i in range(start, min(start + BATCH, n_rows)):
            blob = rng.standard_normal(embed_train.EMBED_DIM).astype("<f4").tobytes()
            rows.append((f"/fixtures/f_{i:06d}.wav", f"f_{i:06d}.wav", blob))
        conn.executemany(
            "INSERT INTO samples (path, name, parent_folder, embedding) "
            "VALUES (?, ?, '', ?)",
            rows,
        )
    conn.commit()


def test_fetch_training_vectors_memory_bounded_by_n_train(
    empty_db: Path,
) -> None:
    """With a library of 10K rows and n_train=100, peak memory during
    `_fetch_training_vectors` must stay well below what a full-library
    materialization would consume.

    Envelope:
    - 100 × 2048 bytes (sampled blobs)          = 200 KB
    - 10K IDs × ~28 B (PyLong overhead)         = 280 KB
    - batch fetch buffer (500 × 2KB)            = 1 MB
    - numpy + Python overhead                   ≈ 1 MB
    Comfortable ceiling: 5 MB.

    Bad (old) implementation would peak at ~20 MB just for the blobs
    from the full fetchall, so 5 MB cleanly distinguishes the two.
    """
    n_rows = 10_000
    n_train = 100

    conn = sqlite3.connect(empty_db)
    try:
        _populate(conn, n_rows)

        tracemalloc.start()
        vecs = embed_train._fetch_training_vectors(conn, n_train, seed=0)
        _, peak = tracemalloc.get_traced_memory()
        tracemalloc.stop()

        assert vecs.shape == (n_train, embed_train.EMBED_DIM)
        assert vecs.dtype == np.float32

        ceiling = 5 * 1024 * 1024
        assert peak < ceiling, (
            f"peak memory {peak / 1024 / 1024:.1f} MB exceeded {ceiling / 1024 / 1024:.1f} MB "
            f"envelope on library of {n_rows} rows with n_train={n_train}. "
            "Likely regression: the function is fetching all BLOBs before sampling."
        )
    finally:
        conn.close()


def test_fetch_training_vectors_errors_when_empty(empty_db: Path) -> None:
    """Preserves the pre-refactor guarantee: empty libraries raise
    a descriptive `RuntimeError` rather than silently returning an
    empty array."""
    conn = sqlite3.connect(empty_db)
    try:
        import pytest

        with pytest.raises(RuntimeError, match="no non-LOOP embedded rows"):
            embed_train._fetch_training_vectors(conn, n_train=10, seed=0)
    finally:
        conn.close()


def test_fetch_training_vectors_respects_loop_filter(empty_db: Path) -> None:
    """LOOP-category rows must never appear in the training set even if
    they have embeddings — the filter lives in the SELECT, not just in
    embed_encode's write path."""
    conn = sqlite3.connect(empty_db)
    try:
        rng = np.random.default_rng(0)

        # 3 non-LOOP rows with embeddings.
        for i in range(3):
            blob = rng.standard_normal(embed_train.EMBED_DIM).astype("<f4").tobytes()
            conn.execute(
                "INSERT INTO samples (path, name, parent_folder, category, embedding) "
                "VALUES (?, ?, '', 'DRUMS/KICK', ?)",
                (f"/f/k{i}.wav", f"k{i}.wav", blob),
            )
        # 3 LOOP rows with embeddings (shouldn't get selected even though
        # embed_encode should never populate these; defense in depth).
        for i in range(3):
            blob = rng.standard_normal(embed_train.EMBED_DIM).astype("<f4").tobytes()
            conn.execute(
                "INSERT INTO samples (path, name, parent_folder, category, embedding) "
                "VALUES (?, ?, '', 'LOOP/BREAKS', ?)",
                (f"/f/l{i}.wav", f"l{i}.wav", blob),
            )
        conn.commit()

        vecs = embed_train._fetch_training_vectors(conn, n_train=100, seed=0)
        # Only the 3 non-LOOP rows should be eligible.
        assert vecs.shape == (3, embed_train.EMBED_DIM)
    finally:
        conn.close()
