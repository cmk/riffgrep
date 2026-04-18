"""Train a Product Quantization codebook over stored CLAP embeddings.

Samples up to N non-LOOP rows from the `samples` table, trains a FAISS
ProductQuantizer(d=512, M=128, nbits=8), and writes the 524288-byte
codebook blob plus a bumped version counter into the `metadata` table.

The byte layout matches Rust's `riffgrep::engine::pq::ProductQuantizer`:
flat little-endian f32 with shape (M, K, DSUB) = (128, 256, 4) in
C-order. That matches FAISS's internal `pq.centroids` memory layout, so
`faiss.vector_to_array(pq.centroids).astype('<f4').tobytes()` gives the
right blob.

Usage
-----

    python scripts/embed_train.py --n-train 10000

The codebook lives in `metadata.pq_codebook`; the monotonic version
counter in `metadata.pq_codebook_version` (8-byte little-endian int).
Each retrain increments the version by 1 and atomically replaces the
codebook.

This module is also importable; tests call `train()` directly to avoid
depending on faiss in the hot test path (tests that need a codebook
construct one by hand).
"""

from __future__ import annotations

import argparse
import sqlite3
import sys
from pathlib import Path

import numpy as np

DEFAULT_DB = Path(
    "~/Library/Application Support/riffgrep/index.db"
).expanduser()

EMBED_DIM = 512
PQ_M = 128
PQ_K = 256
PQ_DSUB = 4
CODEBOOK_BYTES = PQ_M * PQ_K * PQ_DSUB * 4  # 524288
assert PQ_M * PQ_DSUB == EMBED_DIM


def _fetch_training_vectors(
    conn: sqlite3.Connection, n_train: int, seed: int
) -> np.ndarray:
    """Return an (N, 512) float32 matrix from non-LOOP embedded rows."""
    # SQLite has no seeded ORDER BY; fetch id+embedding and sample in Python
    # so the choice is reproducible and testable.
    rows = conn.execute(
        """
        SELECT id, embedding FROM samples
        WHERE embedding IS NOT NULL
          AND (category IS NULL OR category NOT LIKE 'LOOP%')
        """
    ).fetchall()
    if not rows:
        raise RuntimeError("no non-LOOP embedded rows available for training")

    rng = np.random.default_rng(seed)
    picks = rng.permutation(len(rows))[: min(n_train, len(rows))]
    vecs = np.empty((len(picks), EMBED_DIM), dtype=np.float32)
    for i, j in enumerate(picks):
        buf = rows[j][1]
        if len(buf) != EMBED_DIM * 4:
            raise RuntimeError(
                f"row {rows[j][0]}: embedding is {len(buf)} bytes, "
                f"expected {EMBED_DIM * 4}"
            )
        vecs[i] = np.frombuffer(buf, dtype="<f4")
    return vecs


def train(vectors: np.ndarray) -> bytes:
    """Train a PQ codebook over `vectors` (N, 512) float32 and return the
    byte blob in Rust-compatible little-endian f32 order, shape
    (M, K, DSUB) = (128, 256, 4) C-contiguous.
    """
    if vectors.ndim != 2 or vectors.shape[1] != EMBED_DIM:
        raise ValueError(
            f"train expects (N, {EMBED_DIM}) float32; got {vectors.shape}"
        )
    import faiss  # lazy import — heavy

    pq = faiss.ProductQuantizer(EMBED_DIM, PQ_M, 8)
    pq.train(np.ascontiguousarray(vectors, dtype=np.float32))

    centroids = faiss.vector_to_array(pq.centroids)
    if centroids.size != PQ_M * PQ_K * PQ_DSUB:
        raise RuntimeError(
            f"faiss returned {centroids.size} centroid floats, "
            f"expected {PQ_M * PQ_K * PQ_DSUB}"
        )
    blob = centroids.astype("<f4", copy=False).tobytes()
    if len(blob) != CODEBOOK_BYTES:
        raise RuntimeError(
            f"codebook serialized to {len(blob)} bytes, expected {CODEBOOK_BYTES}"
        )
    return blob


def _current_version(conn: sqlite3.Connection) -> int:
    row = conn.execute(
        "SELECT value FROM metadata WHERE key = 'pq_codebook_version'"
    ).fetchone()
    if row is None:
        return 0
    buf = row[0]
    if not isinstance(buf, (bytes, bytearray)) or len(buf) != 8:
        raise RuntimeError(
            f"metadata.pq_codebook_version is malformed "
            f"(expected 8-byte LE uint, got {type(buf).__name__} "
            f"len={len(buf) if hasattr(buf, '__len__') else 'n/a'})"
        )
    return int.from_bytes(buf, "little", signed=False)


def write_codebook(conn: sqlite3.Connection, blob: bytes) -> int:
    """Install a new codebook atomically; return the new version."""
    if len(blob) != CODEBOOK_BYTES:
        raise ValueError(
            f"codebook blob is {len(blob)} bytes, expected {CODEBOOK_BYTES}"
        )
    with conn:
        new_version = _current_version(conn) + 1
        conn.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES (?, ?)",
            ("pq_codebook", blob),
        )
        conn.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES (?, ?)",
            (
                "pq_codebook_version",
                new_version.to_bytes(8, "little", signed=False),
            ),
        )
    return new_version


def _positive_int(s: str) -> int:
    v = int(s)
    if v < 1:
        raise argparse.ArgumentTypeError(f"must be >= 1, got {v}")
    return v


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--db", type=Path, default=DEFAULT_DB)
    parser.add_argument("--n-train", type=_positive_int, default=10_000)
    parser.add_argument("--seed", type=int, default=0)
    args = parser.parse_args(argv)

    args.db = args.db.expanduser()
    if not args.db.exists():
        print(f"error: db not found: {args.db}", file=sys.stderr)
        return 2

    conn = sqlite3.connect(args.db)
    try:
        vecs = _fetch_training_vectors(conn, args.n_train, args.seed)
        print(f"training PQ on {len(vecs)} vectors (seed={args.seed})")
        blob = train(vecs)
        version = write_codebook(conn, blob)
        print(f"wrote codebook version {version} ({len(blob)} bytes)")
    finally:
        conn.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
