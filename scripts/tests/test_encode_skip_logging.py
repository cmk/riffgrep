"""P2.4 — `encode_rows` surfaces preprocess-skipped count on stderr.

Over a 1.2M-file run, silent per-row skips accumulate invisibly. The
tail summary is the only operational signal that a batch of files is
unreadable or all-silent. This test captures stderr and asserts the
pattern is present when skips happen, and absent when they don't.
"""

from __future__ import annotations

import sqlite3
import sys
from pathlib import Path
from unittest import mock

import numpy as np
import pytest

import embed_encode


def _encoder() -> embed_encode.Encoder:
    rng = np.random.default_rng(0)

    def encode(batch: np.ndarray) -> np.ndarray:
        return rng.standard_normal((batch.shape[0], 512)).astype(np.float32)

    return encode


def _preprocess_stub(
    *, fail_for: set[str]
) -> "mock._patch[object]":
    """Return a patch context for `embed_encode.preprocess` that returns
    None for paths in `fail_for` and a zero-array otherwise."""
    fake_audio = np.zeros(48_000 * 2, dtype=np.float32)

    def stub(path):
        return None if path in fail_for else fake_audio

    return mock.patch.object(embed_encode, "preprocess", side_effect=stub)


def test_skip_count_surfaced_on_stderr(
    empty_db: Path, insert_row, capsys: pytest.CaptureFixture[str]
) -> None:
    """When preprocess fails on some rows, `encode_rows` prints the
    skip total on stderr at the end."""
    fail_paths = {"/f/broken_1.wav", "/f/broken_2.wav"}
    conn = sqlite3.connect(empty_db)
    try:
        insert_row(conn, path="/f/clean_1.wav")
        insert_row(conn, path="/f/broken_1.wav")
        insert_row(conn, path="/f/clean_2.wav")
        insert_row(conn, path="/f/broken_2.wav")
        conn.commit()
        rows = embed_encode._select_rows(conn, limit=None)

        with _preprocess_stub(fail_for=fail_paths):
            written = embed_encode.encode_rows(
                conn, rows, _encoder(), batch_size=4, progress=False
            )
    finally:
        conn.close()

    captured = capsys.readouterr()
    assert written == 2
    assert "skipped 2 rows (preprocess returned None" in captured.err


def test_no_skip_line_when_nothing_skipped(
    empty_db: Path, insert_row, capsys: pytest.CaptureFixture[str]
) -> None:
    """Clean runs (no preprocess failures) must not print the skip line
    — otherwise the operational signal is noise."""
    conn = sqlite3.connect(empty_db)
    try:
        for i in range(3):
            insert_row(conn, path=f"/f/clean_{i}.wav")
        conn.commit()
        rows = embed_encode._select_rows(conn, limit=None)

        with _preprocess_stub(fail_for=set()):
            written = embed_encode.encode_rows(
                conn, rows, _encoder(), batch_size=4, progress=False
            )
    finally:
        conn.close()

    captured = capsys.readouterr()
    assert written == 3
    assert "skipped" not in captured.err


def test_skip_line_singular_phrasing(
    empty_db: Path, insert_row, capsys: pytest.CaptureFixture[str]
) -> None:
    """A single skip produces "skipped 1 row", not "1 rows"."""
    conn = sqlite3.connect(empty_db)
    try:
        insert_row(conn, path="/f/clean.wav")
        insert_row(conn, path="/f/broken.wav")
        conn.commit()
        rows = embed_encode._select_rows(conn, limit=None)

        with _preprocess_stub(fail_for={"/f/broken.wav"}):
            embed_encode.encode_rows(
                conn, rows, _encoder(), batch_size=4, progress=False
            )
    finally:
        conn.close()

    captured = capsys.readouterr()
    assert "skipped 1 row (preprocess" in captured.err
    assert "skipped 1 rows" not in captured.err
