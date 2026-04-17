"""P1.4 — every stored embedding has ||v||_2 in [0.9, 1.1].

CLAP outputs approximately-unit-norm vectors. This test asserts the
invariant on the stored BLOBs and catches preprocessing regressions
(e.g., accidental un-normalization, wrong byte order, dtype drift).

Uses a mocked encoder that returns L2-normalized vectors; the test
exercises the SERIALIZATION side of the pipeline, not CLAP itself.
"""

from __future__ import annotations

import sqlite3
from pathlib import Path
from unittest import mock

import numpy as np

import embed_encode


def test_stored_embeddings_are_unit_norm(empty_db: Path, insert_row) -> None:
    fake_audio = np.zeros(48_000 * 2, dtype=np.float32)
    rng = np.random.default_rng(42)

    def encoder(batch: np.ndarray) -> np.ndarray:
        v = rng.standard_normal((batch.shape[0], 512)).astype(np.float32)
        norms = np.linalg.norm(v, axis=1, keepdims=True)
        return (v / norms).astype(np.float32)

    with mock.patch.object(embed_encode, "preprocess", return_value=fake_audio):
        conn = sqlite3.connect(empty_db)
        try:
            for i in range(32):
                insert_row(conn, path=f"/f/sample_{i:02d}.wav")
            conn.commit()
            rows = embed_encode._select_rows(conn, limit=None)
            embed_encode.encode_rows(
                conn, rows, encoder, batch_size=8, progress=False
            )
        finally:
            conn.close()

    conn = sqlite3.connect(empty_db)
    try:
        blobs = [
            row[0]
            for row in conn.execute(
                "SELECT embedding FROM samples WHERE embedding IS NOT NULL"
            ).fetchall()
        ]
    finally:
        conn.close()

    assert len(blobs) == 32
    for blob in blobs:
        assert len(blob) == 2048, f"blob is {len(blob)} bytes, expected 2048"
        vec = np.frombuffer(blob, dtype="<f4")
        assert vec.size == 512
        norm = float(np.linalg.norm(vec))
        assert 0.9 <= norm <= 1.1, f"norm={norm}"
