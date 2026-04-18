"""P1.3 — hand-labeled ranking sanity check.

Gated on `@pytest.mark.requires_clap_model` — skipped by default so the
test suite doesn't require a ~600MB checkpoint download. When the
checkpoint + fixture env vars are set, runs `embed_encode` with the
real CLAP encoder on a labeled fixture dir, then computes pairwise L2
rankings in Python over the stored full-precision embeddings. This
sprint does NOT invoke PQ training or the `rfg --similar` CLI — that
coverage is Plan 3's integration gate. Assertion: ≥80% of queries have
≥8/10 top neighbors matching the query's category label.

Fixture expectations (set these before running):

    LAION_CLAP_CHECKPOINT=/abs/path/to/music_audioset_epoch_15_esc_90.14.pt
    RIFFGREP_RANKING_FIXTURE_DIR=/abs/path/to/fixture_corpus

The fixture dir must contain at least 5 category subdirs
(e.g., DRUMS_KICK/, DRUMS_HAT/, PAD/, BASS/, SFX/), each with at
least 10 WAV files named `*.wav`. The category label is the parent dir
name.
"""

from __future__ import annotations

import os
import sqlite3
from pathlib import Path
from typing import Iterable

import numpy as np
import pytest

import embed_encode

pytestmark = pytest.mark.requires_clap_model


def _collect_fixtures(fixture_dir: Path) -> list[tuple[str, str]]:
    rows: list[tuple[str, str]] = []
    for category_dir in sorted(p for p in fixture_dir.iterdir() if p.is_dir()):
        for wav in sorted(category_dir.glob("*.wav")):
            rows.append((str(wav), category_dir.name))
    return rows


def _top_n_categories(
    query_emb: np.ndarray,
    candidates: Iterable[tuple[str, np.ndarray]],
    n: int,
) -> list[str]:
    scored = [
        (cat, float(np.linalg.norm(query_emb - emb)))
        for cat, emb in candidates
    ]
    scored.sort(key=lambda x: x[1])
    return [cat for cat, _ in scored[:n]]


@pytest.fixture
def ranking_env() -> tuple[Path, Path]:
    ckpt = os.environ.get("LAION_CLAP_CHECKPOINT")
    fix = os.environ.get("RIFFGREP_RANKING_FIXTURE_DIR")
    if not ckpt or not fix:
        pytest.skip(
            "LAION_CLAP_CHECKPOINT and RIFFGREP_RANKING_FIXTURE_DIR "
            "must both be set for this test"
        )
    ckpt_path = Path(ckpt)
    fix_path = Path(fix)
    if not ckpt_path.exists() or not fix_path.exists():
        pytest.skip("checkpoint or fixture dir does not exist")
    return ckpt_path, fix_path


def test_ranking_sanity(
    empty_db: Path, insert_row, ranking_env: tuple[Path, Path]
) -> None:
    ckpt_path, fix_dir = ranking_env
    rows = _collect_fixtures(fix_dir)

    # Enforce the docstring's fixture contract: ≥5 categories, each with
    # ≥10 WAVs. A corpus of 50 files concentrated in one category would
    # trivially pass the hit-rate threshold without exercising
    # cross-category discrimination.
    per_category: dict[str, int] = {}
    for _, cat in rows:
        per_category[cat] = per_category.get(cat, 0) + 1
    small = {c: n for c, n in per_category.items() if n < 10}
    if len(per_category) < 5 or small:
        pytest.skip(
            f"fixture needs ≥5 categories with ≥10 WAVs each; got "
            f"{len(per_category)} categories, under-populated: {small}"
        )

    category_of: dict[str, str] = {path: cat for path, cat in rows}

    conn = sqlite3.connect(empty_db)
    try:
        for path, cat in rows:
            insert_row(conn, path=path, category=cat)
        conn.commit()
        encoder = embed_encode._load_laion_clap(ckpt_path)
        embed_encode.encode_rows(
            conn,
            embed_encode._select_rows(conn, limit=None),
            encoder,
            batch_size=16,
            progress=False,
        )
        stored = conn.execute(
            "SELECT path, category, embedding FROM samples WHERE embedding IS NOT NULL"
        ).fetchall()
    finally:
        conn.close()

    embeddings = {
        path: np.frombuffer(blob, dtype="<f4") for path, _, blob in stored
    }

    # Treat every stored file as a query and score the rest. This gives
    # a per-query hit-rate; the threshold check below requires ≥80% of
    # queries to satisfy the ≥8/10 match invariant.
    if not stored:
        pytest.skip("no embeddings were stored — fixture unreadable?")

    violations: list[str] = []
    for query_path, query_cat, _ in stored:
        others = [
            (category_of[p], e) for p, e in embeddings.items() if p != query_path
        ]
        top_cats = _top_n_categories(embeddings[query_path], others, n=10)
        hits = sum(1 for c in top_cats if c == query_cat)
        if hits < 8:
            violations.append(f"{query_path}: {hits}/10 ({query_cat})")

    ok_ratio = 1.0 - len(violations) / len(stored)
    assert ok_ratio >= 0.8, (
        f"ranking_ok_ratio={ok_ratio:.2f} below 0.80; "
        f"violations={violations[:10]}"
    )
