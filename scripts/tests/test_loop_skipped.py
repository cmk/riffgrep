"""P1.5 — LOOP-category rows never receive an embedding in this sprint.

`embed_encode` filters via `category IS NULL OR category NOT LIKE 'LOOP%'`.
This test verifies the SQL gate: after a full run on a fixture DB that
mixes LOOP and non-LOOP rows, LOOP rows have NULL embeddings.
"""

from __future__ import annotations

import sqlite3
from pathlib import Path
from unittest import mock

import numpy as np

import embed_encode


def test_loop_rows_never_embedded(empty_db: Path, insert_row) -> None:
    fake_audio = np.zeros(48_000 * 2, dtype=np.float32)

    conn = sqlite3.connect(empty_db)
    try:
        insert_row(conn, path="/f/kick.wav", category="DRUMS/KICK")
        insert_row(conn, path="/f/hat.wav", category="DRUMS/HAT")
        insert_row(conn, path="/f/break1.wav", category="LOOP/BREAKS")
        insert_row(conn, path="/f/break2.wav", category="LOOP/DNB")
        insert_row(conn, path="/f/unknown.wav", category="")
        conn.commit()
    finally:
        conn.close()

    rng = np.random.default_rng(0)

    def encoder(batch: np.ndarray) -> np.ndarray:
        return rng.standard_normal((batch.shape[0], 512)).astype(np.float32)

    with mock.patch.object(embed_encode, "preprocess", return_value=fake_audio):
        conn = sqlite3.connect(empty_db)
        try:
            rows = embed_encode._select_rows(conn, limit=None)
            written = embed_encode.encode_rows(
                conn, rows, encoder, batch_size=8, progress=False
            )
        finally:
            conn.close()
    assert written == 3  # kick, hat, unknown (empty category passes through)

    conn = sqlite3.connect(empty_db)
    try:
        (leaked,) = conn.execute(
            "SELECT COUNT(*) FROM samples "
            "WHERE category LIKE 'LOOP%' AND embedding IS NOT NULL"
        ).fetchone()
        (embedded,) = conn.execute(
            "SELECT COUNT(*) FROM samples WHERE embedding IS NOT NULL"
        ).fetchone()
    finally:
        conn.close()
    assert leaked == 0
    assert embedded == 3
