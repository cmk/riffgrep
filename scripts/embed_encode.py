"""Populate samples.embedding with LAION-CLAP vectors.

Walks rows where `embedding IS NULL AND (category IS NULL OR
category NOT LIKE 'LOOP%')`, loads and preprocesses audio, runs
LAION-CLAP inference in batches, serializes each 512-dim float32 vector
to a 2048-byte little-endian BLOB, and stores it in the `samples` row.

LOOP-category rows are intentionally skipped in this sprint — they need
onset-based splitting that is deferred to a later plan. See
doc/plans/plan-2026-04-17-01.md.

Usage
-----

    # Preflight: process 50 non-LOOP rows.
    python scripts/embed_encode.py --limit 50

    # Full pass, default DB path.
    python scripts/embed_encode.py

Environment:
    LAION_CLAP_CHECKPOINT — path to the .pt checkpoint. If unset, defaults
    to ~/Library/Application Support/riffgrep/models/
    music_audioset_epoch_15_esc_90.14.pt.

This module is also importable; its `main()` function is used by tests
that mock the CLAP encoder.
"""

from __future__ import annotations

import argparse
import os
import sqlite3
import sys
from pathlib import Path
from typing import Callable, Iterable, Iterator, Sequence

import numpy as np

from embed_preprocess import preprocess

DEFAULT_DB = Path(
    "~/Library/Application Support/riffgrep/index.db"
).expanduser()
DEFAULT_MODEL = Path(
    "~/Library/Application Support/riffgrep/models/"
    "music_audioset_epoch_15_esc_90.14.pt"
).expanduser()

EMBED_DIM = 512
EMBED_BYTES = EMBED_DIM * 4  # 2048

# Row shape returned by the SELECT — id, path.
Row = tuple[int, str]
# Encoder: batch of (N, n_samples) float32 → (N, 512) float32.
Encoder = Callable[[np.ndarray], np.ndarray]


def _positive_int(s: str) -> int:
    v = int(s)
    if v < 1:
        raise argparse.ArgumentTypeError(f"must be >= 1, got {v}")
    return v


def _select_rows(
    conn: sqlite3.Connection, limit: int | None
) -> list[Row]:
    sql = """
        SELECT id, path FROM samples
        WHERE embedding IS NULL
          AND (category IS NULL OR category NOT LIKE 'LOOP%')
    """
    params: tuple = ()
    if limit is not None:
        sql += " LIMIT ?"
        params = (limit,)
    return [(int(r[0]), str(r[1])) for r in conn.execute(sql, params).fetchall()]


def _chunks(seq: Sequence[Row], n: int) -> Iterator[list[Row]]:
    if n < 1:
        raise ValueError(f"_chunks: n must be >= 1, got {n}")
    for i in range(0, len(seq), n):
        yield list(seq[i : i + n])


def _load_laion_clap(model_path: Path) -> Encoder:
    import laion_clap  # lazy import — heavy

    model = laion_clap.CLAP_Module(enable_fusion=False, amodel="HTSAT-base")
    model.load_ckpt(str(model_path))

    def encode(batch: np.ndarray) -> np.ndarray:
        # laion_clap expects (N, n_samples) float32.
        return model.get_audio_embedding_from_data(
            x=batch, use_tensor=False
        ).astype(np.float32)

    return encode


def encode_rows(
    conn: sqlite3.Connection,
    rows: Iterable[Row],
    encoder: Encoder,
    *,
    batch_size: int = 32,
    dry_run: bool = False,
    progress: bool = True,
) -> int:
    """Encode `rows` and UPDATE `samples.embedding`.

    Returns the number of embeddings actually committed to the DB. The
    count is always 0 when `dry_run=True`, and can be less than the
    number of rows encoded when a concurrent writer has already populated
    an embedding between the SELECT and the UPDATE — the write guards on
    `embedding IS NULL` so it is idempotent under that race.

    Rows that fail preprocessing are counted in `skipped` (see below)
    and surfaced on stderr at the end of the run. Over a 1.2M-file
    sweep, silent per-row skips accumulate invisibly — the tail summary
    is the only operational signal that a batch of files is unreadable
    or outside the preprocessing contract.

    `rows` is materialized into a list on entry (if it isn't one
    already) so the tqdm progress bar can show a total. This is
    intentional: at 1.2M scale the candidate list is `(int, str)`
    tuples, not BLOBs — a few hundred MB peak, well within the memory
    budget T1 is protecting. The memory concern the compartmentalized
    resolver and `_fetch_training_vectors` rewrite address is the
    *training-vector* blobs (~2.5 GB if materialized), not these
    metadata tuples.
    """
    if not isinstance(rows, list):
        rows = list(rows)
    if progress:
        try:
            from tqdm import tqdm  # type: ignore[import-untyped]

            pbar = tqdm(total=len(rows), unit="file")
        except ImportError:  # pragma: no cover — tqdm is a declared dep
            pbar = None
    else:
        pbar = None

    written = 0
    skipped = 0
    for batch in _chunks(rows, batch_size):
        audios: list[tuple[Row, np.ndarray]] = []
        for row in batch:
            audio = preprocess(row[1])
            if audio is None:
                skipped += 1
                continue
            audios.append((row, audio))
        if not audios:
            if pbar:
                pbar.update(len(batch))
            continue

        stacked = np.stack([a for _, a in audios])
        embeddings = encoder(stacked)
        if embeddings.shape != (len(audios), EMBED_DIM):
            raise RuntimeError(
                f"encoder returned {embeddings.shape}, expected "
                f"({len(audios)}, {EMBED_DIM})"
            )

        if not dry_run:
            for i, (row, _) in enumerate(audios):
                blob = embeddings[i].astype("<f4", copy=False).tobytes()
                if len(blob) != EMBED_BYTES:
                    raise RuntimeError(
                        f"serialized embedding for row {row[0]} is "
                        f"{len(blob)} bytes, expected {EMBED_BYTES}"
                    )
                cur = conn.execute(
                    "UPDATE samples SET embedding = ? "
                    "WHERE id = ? AND embedding IS NULL",
                    (blob, row[0]),
                )
                if cur.rowcount == 1:
                    written += 1
            conn.commit()
        if pbar:
            pbar.update(len(batch))

    if pbar:
        pbar.close()
    if skipped > 0:
        print(
            f"skipped {skipped} row{'s' if skipped != 1 else ''} "
            f"(preprocess returned None — unreadable or all-silent)",
            file=sys.stderr,
        )
    return written


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--db", type=Path, default=DEFAULT_DB)
    parser.add_argument("--batch-size", type=_positive_int, default=32)
    parser.add_argument("--limit", type=_positive_int, default=None)
    parser.add_argument("--model", type=Path, default=None)
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Run encoder but do not write to the DB.",
    )
    args = parser.parse_args(argv)

    args.db = args.db.expanduser()
    if not args.db.exists():
        print(f"error: db not found: {args.db}", file=sys.stderr)
        return 2

    model_path = (
        args.model
        or Path(os.environ.get("LAION_CLAP_CHECKPOINT", str(DEFAULT_MODEL)))
    ).expanduser()
    if not model_path.exists():
        print(
            f"error: CLAP checkpoint not found: {model_path}\n"
            "Set LAION_CLAP_CHECKPOINT or pass --model <path>.",
            file=sys.stderr,
        )
        return 2

    conn = sqlite3.connect(args.db)
    try:
        rows = _select_rows(conn, args.limit)
        if not rows:
            print("nothing to do — all non-LOOP rows already embedded")
            return 0
        print(f"encoding {len(rows)} files (batch_size={args.batch_size})")
        encoder = _load_laion_clap(model_path)
        written = encode_rows(
            conn, rows, encoder, batch_size=args.batch_size, dry_run=args.dry_run
        )
        if args.dry_run:
            print(f"dry run — skipped DB writes for {len(rows)} candidate rows")
        else:
            print(f"wrote {written} embeddings (of {len(rows)} candidates)")
    finally:
        conn.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
