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

<!-- gh-id: 3104526697 -->
### Copilot on [`scripts/tests/test_encode_skip_logging.py:12`](https://github.com/cmk/riffgrep/pull/13#discussion_r3104526697) (2026-04-18 04:47 UTC)

`sys` is imported but never used in this test module; please remove the unused import to keep the test file tidy.

<!-- gh-id: 3104526720 -->
### Copilot on [`scripts/tests/test_preprocess_properties.py:20`](https://github.com/cmk/riffgrep/pull/13#discussion_r3104526720) (2026-04-18 04:47 UTC)

Unused imports: `pytest` and `settings` are imported but not referenced anywhere in this file. Please remove them to avoid confusion about intended usage/configuration.

<!-- gh-id: 3104526737 -->
### Copilot on [`pyproject.toml:9`](https://github.com/cmk/riffgrep/pull/13#discussion_r3104526737) (2026-04-18 04:47 UTC)

The comment above `[project].dependencies` says the core deps are enough to “run the default test suite”, but with only `numpy` installed the suite can’t run because `pytest` (and now `hypothesis`) are in the `dev` extra. Consider rewording to distinguish “import scripts” vs “run tests with `--extra dev`” to avoid confusion.

<!-- gh-id: 3104526757 -->
### Copilot on [`doc/plans/plan-2026-04-18-01.md:32`](https://github.com/cmk/riffgrep/pull/13#discussion_r3104526757) (2026-04-18 04:47 UTC)

Part A step 1 says a plain `uv sync` pulls “numpy + test tooling”, but in `pyproject.toml` the test tooling lives in the `dev` extra. As written this is misleading—either update the text to mention `uv sync --extra dev` for running tests, or move the needed test deps into the default sync story you want.

<!-- gh-id: 3104526771 -->
### Copilot on [`doc/plans/plan-2026-04-18-01.md:114`](https://github.com/cmk/riffgrep/pull/13#discussion_r3104526771) (2026-04-18 04:47 UTC)

T1’s last paragraph says `encode_rows` will “take an iterator, not a pre-materialized list” and that tests were updated for an iterator contract. In this PR, `encode_rows` still materializes non-lists via `list(rows)` (and `_select_rows` returns a list), so this plan text doesn’t match the implemented behavior. Either update the plan to describe the current behavior, or follow through by making `encode_rows` truly streaming (and adjusting tqdm/progress accordingly).

<!-- gh-id: 3104526775 -->
### Copilot on [`scripts/tests/test_train_memory.py:27`](https://github.com/cmk/riffgrep/pull/13#discussion_r3104526775) (2026-04-18 04:47 UTC)

`EMBED_BYTES` is defined but never used in this test module. Please remove it (or use it in the assertions) to keep the test focused.

<!-- gh-id: 3104526784 -->
### Copilot on [`doc/plans/plan-2026-04-18-01.md:21`](https://github.com/cmk/riffgrep/pull/13#discussion_r3104526784) (2026-04-18 04:47 UTC)

`~/.claude/plans/...` is a local filesystem path that won’t exist for other readers and isn’t accessible from the repo. Please replace this with a repo-relative doc link (e.g., under `doc/plans/`) or an issue/PR link so the “master plan” reference is actionable.

<!-- gh-id: 3104526799 -->
### Copilot on [`doc/plans/plan-2026-04-18-01.md:175`](https://github.com/cmk/riffgrep/pull/13#discussion_r3104526799) (2026-04-18 04:47 UTC)

P2.2 says to add a 100K-row test and assert RSS delta, but the actual added regression test (`scripts/tests/test_train_memory.py`) uses a 10K-row fixture and `tracemalloc` peak Python allocations. Please align this row with the implemented test so the plan’s verification table stays accurate.

<!-- gh-id: 4133562261 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-18 04:47 UTC](https://github.com/cmk/riffgrep/pull/13#pullrequestreview-4133562261))

## Pull request overview

Implements Plan 2 follow-ups for the Python embedding pipeline (memory-bounded training vector sampling, FAISS layout validation, skip-count logging, and property-based preprocess testing), plus Python/UV dependency compartmentalization and CI updates to keep default installs lightweight.

**Changes:**
- Refactor `embed_train._fetch_training_vectors` to sample IDs first and batch-fetch only selected embedding BLOBs; add tracemalloc regression coverage.
- Add/extend tests: FAISS decode/layout spot-check, preprocess Hypothesis properties, and encode-row skip-count stderr logging.
- Rework Python dependencies into opt-in extras (`encode`/`train`/`ml`) and update CI to install Hypothesis for the new property tests.

### Reviewed changes

Copilot reviewed 11 out of 12 changed files in this pull request and generated 9 comments.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| scripts/embed_train.py | Makes training-vector fetch memory-bounded by sampling IDs then batch-fetching embeddings. |
| scripts/embed_encode.py | Adds skip counting and stderr summary; adjusts `rows` materialization behavior. |
| scripts/tests/test_train_memory.py | New tracemalloc regression test for bounded peak allocations in training-vector fetch. |
| scripts/tests/test_codebook_rust_compat.py | Adds FAISS-native decode vs Python reshape/layout cross-check. |
| scripts/tests/test_encode_skip_logging.py | New tests asserting skip-count line presence/absence and singular phrasing. |
| scripts/tests/test_preprocess_properties.py | New Hypothesis property tests for preprocess helper invariants and composition. |
| pyproject.toml | Splits heavy ML deps into extras; adds Hypothesis to dev extra; adds UV overrides. |
| .github/workflows/ci.yml | Installs Hypothesis in Python CI job to run new property tests. |
| doc/plans/plan-2026-04-18-01.md | Adds the Plan 02 document describing tasks/verification. |
| doc/reviews/review-0000.md | Adds local review artifact for this PR’s changes. |
| .python-version | Pins local Python version to 3.12. |
| .gitignore | Ignores `uv.lock`. |
</details>






<!-- gh-id: 3104526810 -->
### Copilot on [`scripts/embed_encode.py:130`](https://github.com/cmk/riffgrep/pull/13#discussion_r3104526810) (2026-04-18 04:47 UTC)

The docstring suggests callers can pass an iterator and that it will be consumed “as a sequence,” but the implementation immediately materializes any non-list with `list(rows)`, which can be large (e.g., 1.2M candidates). Consider clarifying in the docstring that `encode_rows` will fully materialize the iterable (or refactor progress reporting to avoid requiring `len(rows)` and support true streaming).
