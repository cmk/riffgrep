# PR #13 — feat/plan02-followups (T1-T4 from plan-2026-04-18-01)

## Local review (2026-04-18)

**Branch:** feat/plan02-followups
**Commits (pre-fix):** 7 (origin/main..feat/plan02-followups)
**Reviewer:** Claude (sonnet, independent)

---

### T1 correctness (memory-bounded `_fetch_training_vectors`)

Logic is sound. Batched IN-clause fetch + `pos_for_id` mapping correctly handles SQLite's arbitrary row-return order. Seed drives `rng.choice` over a stable Python list, preserving reproducibility. `fetched != n_sample` guard handles missing rows.

**One edge case added in fix commit `d1e24c7`:** `samples.id` is the SQLite INTEGER PRIMARY KEY and thus unique by schema, but a silent dedup in `pos_for_id` under hypothetical schema drift (WITHOUT ROWID tables, non-unique pseudo-ids) would produce duplicate-embedding slots. Added a `len(pos_for_id) != n_sample` assertion as a cheap loud guard.

### T2 correctness (FAISS layout spot-check) — MUST FIX

The original test at `e75f4e3` trained two independent FAISS `ProductQuantizer` objects on the same data (one inside `train()` for serialization, one outside for the decode comparison) and compared their outputs. This does not prove layout agreement — two fresh `pq.train()` calls on identical input do NOT produce byte-identical centroids because FAISS's internal k-means RNG isn't seeded to be deterministic across independent objects.

**The prior test passed by accident of that RNG landing near the same cluster-0 on the all-zero code.**

Fixed in `d1e24c7`: construct a single `ProductQuantizer`, train it, serialize *its* centroids via `faiss.vector_to_array(pq.centroids).tobytes()`, then decode the all-zero code from the *same* PQ and compare to `_reference_decode(blob)`. Now the test genuinely exercises the (M, K, DSUB) layout agreement between the Python reshape and FAISS's native representation.

### T3 skip counting

Both skip paths are counted correctly. Per-row `skipped += 1` at the `audio is None` branch; the batch-level `if not audios: continue` afterward doesn't represent uncounted skips because every row already passed through the counter. `write_rows` prints the summary on stderr only when `skipped > 0`, and uses singular/plural phrasing correctly (verified via dedicated test).

### T4 property coverage

The 10 hypothesis properties cover the real invariants for `_fit_window`, `_peak_normalize`, `_trim_silence`, and the composed pipeline. Tolerances (`1%` relative for float32 peak) are appropriate. `assume()` guards are correctly placed. No flakiness risk.

### Memory test realism

`test_fetch_training_vectors_memory_bounded_by_n_train` populates 10K rows (~20 MB of transient blobs during populate, but `tracemalloc.start()` fires after populate so baseline is clean). Post-refactor implementation measured ~1.5 MB peak vs pre-refactor's 21 MB. The 5 MB envelope cleanly distinguishes and scales to larger libraries.

### `override-dependencies` scope

The `numba<0.62` / `llvmlite<0.45` pins in `[tool.uv]` are resolver-global. If a future dep requires `numba>=0.62` the resolver produces a confusing conflict error (not "missing wheel"). Loudened the comment in `d1e24c7` so a future maintainer hits this first when debugging.

### Plan Deferred section

No drift. BEXT PQ-code mirror and text queries remain correctly deferred to the master plan's Plan 3/4.

### Commit hygiene

All 8 commits (post-fix) use valid conventional-commit prefixes under 72 chars. History is linear. No fixups.

---

### Resolution (pre-push)

| # | Item | Fix commit |
|---|------|------------|
| 1 | T2 test compared two independently-trained PQs | `d1e24c7` — single PQ for both serialization and decode |
| 2 | `_fetch_training_vectors` silent dedup under duplicate IDs | `d1e24c7` — `len(pos_for_id) != n_sample` assertion |
| 3 | `override-dependencies` comment too quiet | `d1e24c7` — expanded comment with "resolver conflict, not missing wheel" pointer |

Cleared for push.
