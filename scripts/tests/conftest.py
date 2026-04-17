"""Shared pytest fixtures for the scripts/ test suite."""

from __future__ import annotations

import sqlite3
import sys
from pathlib import Path
from typing import Callable

import pytest

SCRIPTS_DIR = Path(__file__).resolve().parent.parent
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))


# Minimal subset of the riffgrep `samples` schema. Tests only reference
# id, path, category, embedding — the rest are NOT NULL defaults riffgrep
# sets. Keeping the column list exhaustive would couple these tests to
# every Rust schema change; keeping it minimal means we add columns only
# when a test needs them.
_SAMPLES_SCHEMA = """
    CREATE TABLE samples (
        id INTEGER PRIMARY KEY,
        path TEXT NOT NULL UNIQUE,
        name TEXT NOT NULL DEFAULT '',
        parent_folder TEXT NOT NULL DEFAULT '',
        category TEXT NOT NULL DEFAULT '',
        mtime INTEGER NOT NULL DEFAULT 0,
        embedding BLOB
    );

    CREATE TABLE metadata (
        key TEXT PRIMARY KEY,
        value BLOB
    );
"""


@pytest.fixture
def empty_db(tmp_path: Path) -> Path:
    """Create an empty SQLite DB with riffgrep's samples + metadata tables."""
    db_path = tmp_path / "index.db"
    conn = sqlite3.connect(db_path)
    try:
        conn.executescript(_SAMPLES_SCHEMA)
        conn.commit()
    finally:
        conn.close()
    return db_path


InsertRow = Callable[..., int]


@pytest.fixture
def insert_row() -> InsertRow:
    """Return a helper that INSERTs a samples row and returns its id."""

    def _insert(
        conn: sqlite3.Connection,
        *,
        path: str,
        category: str = "",
        embedding: bytes | None = None,
    ) -> int:
        cur = conn.execute(
            "INSERT INTO samples (path, name, parent_folder, category, embedding) "
            "VALUES (?, ?, '', ?, ?)",
            (path, Path(path).name, category, embedding),
        )
        return int(cur.lastrowid)

    return _insert
