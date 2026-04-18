"""P1.2 — a second `embed_encode.py` run on a fully-populated DB issues
zero UPDATE statements.

Mocks the CLAP encoder and the preprocessor so the test doesn't need a
real audio file or model checkpoint.
"""

from __future__ import annotations

import sqlite3
from pathlib import Path
from typing import Any
from unittest import mock

import numpy as np

import embed_encode


def _make_encoder(dim: int = 512) -> Any:
    rng = np.random.default_rng(0)

    def encoder(batch: np.ndarray) -> np.ndarray:
        return rng.standard_normal((batch.shape[0], dim)).astype(np.float32)

    return encoder


def _run(db_path: Path) -> int:
    conn = sqlite3.connect(db_path)
    try:
        rows = embed_encode._select_rows(conn, limit=None)
        encoder = _make_encoder()
        return embed_encode.encode_rows(
            conn, rows, encoder, batch_size=8, progress=False
        )
    finally:
        conn.close()


def test_second_run_is_noop(empty_db: Path, insert_row) -> None:
    # Stub the preprocessor to always return a fixed-length zero array
    # (bypasses soundfile entirely).
    fake_audio = np.zeros(48_000 * 2, dtype=np.float32)
    with mock.patch.object(embed_encode, "preprocess", return_value=fake_audio):
        conn = sqlite3.connect(empty_db)
        try:
            for i in range(10):
                insert_row(conn, path=f"/fixtures/kick_{i:02d}.wav")
            conn.commit()
        finally:
            conn.close()

        first = _run(empty_db)
        assert first == 10

        second = _run(empty_db)
        assert second == 0

    conn = sqlite3.connect(empty_db)
    try:
        (count,) = conn.execute(
            "SELECT COUNT(*) FROM samples WHERE embedding IS NOT NULL"
        ).fetchone()
    finally:
        conn.close()
    assert count == 10
