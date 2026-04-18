# PR #10 — feat/embed-pipeline (Plan 1: Python CLAP + PQ codebook)

## Local review (2026-04-17)

**Branch:** feat/embed-pipeline
**Commits:** 4 (origin/main..feat/embed-pipeline)
**Reviewer:** Claude (sonnet, independent)

---

### Commit Hygiene

All four commits use correct conventional prefixes (`test:`, `feat:`, `doc:`, `doc:`) with present-tense imperative subjects under 72 characters. They are reasonably atomic: documentation changes are split from implementation, and tests are a separate commit from the feature code. Since there are no Rust changes, `cargo test` remains unaffected by any commit. The Python-only additions don't break any existing gate.

One minor issue: the `test:` commit (`b072501`) includes only tests, which presupposes the feature code from `b86e78a`. That ordering is backwards — `b86e78a` (`feat:`) comes before `b072501` (`test:`) in the log, which is correct, but the commit message says "Add Plan 1 property tests" after the feature commit. That's fine; linear order is respected.

---

### Code Quality

**`embed_preprocess.py` — preprocess signature deviation from plan (Confidence: 85)**

The plan specifies `target_sr: int = 48000` as a positional-keyword argument. The implementation uses `*` to force keyword-only: `preprocess(path, *, target_sr=..., window_seconds=...)`. This is a better Python convention, but it's a deliberate deviation from the plan signature. Not a bug — callers in `embed_encode.py` call `preprocess(row[1])` with no kwargs, so it works. Flagging only because the plan is explicit about the API surface.

**`embed_encode.py` — tqdm bar accounting on preprocess failures**

```python
pbar.update(len(audios) + (len(batch) - len(audios)))
```

This simplifies to `pbar.update(len(batch))`. The expression is correct but needlessly opaque. The math is right; the readability is poor but not a bug.

**`embed_train.py` `_fetch_training_vectors` — full table load before sampling (Confidence: 90)**

`scripts/embed_train.py` lines around 810-833: the function executes `SELECT id, embedding FROM samples WHERE embedding IS NOT NULL AND ...` with no `LIMIT`, fetches all rows into memory, then samples `n_train` from them in Python. At 1.2M files × 2048 bytes per embedding BLOB = approximately 2.4 GB held in a Python list of tuples. SQLite will also hold intermediate read buffers. This is a real memory pressure issue on a machine that may already be under load during a training run.

The plan's pseudocode used `ORDER BY random_seeded(?) LIMIT ?`, noting SQLite has no seeded random, and the implementation substitutes Python-side sampling as the workaround. The workaround is correct for reproducibility but the memory cost is not flagged anywhere in the implementation. At 100k embedded files (early library state) this is fine; at 1.2M it is not.

**`embed_train.py` `train()` — FAISS centroid layout relies on undocumented behavior (Confidence: 85)**

`scripts/embed_train.py` line ~850: `faiss.vector_to_array(pq.centroids)` is documented to return a flat numpy array from the FAISS internal C++ vector. The layout claim — that it is `(M, K, DSUB)` C-order, i.e., sub-quantizer outer — is structurally correct per FAISS source (it stores centroids as `[M * K, DSUB]` contiguously) but there is no runtime assertion that confirms it before serialization. The `centroids.size` check validates count but not order. If a future FAISS version reorders the layout (unlikely but possible), the bytes produced will pass all size checks and silently corrupt search results.

The `test_codebook_rust_compat.py` `test_faiss_codebook_matches_layout` test calls `train()` and decodes the blob but only checks `np.isfinite(cb).all()` — it does not cross-check any specific centroid value against what FAISS placed at a known index. This means a layout transposition would not be caught by the test. Confidence 85 because FAISS is stable in practice, but the absence of a round-trip assertion is a gap given the "P1.1 Rust-compat" claim.

**`embed_encode.py` — `conn` not opened with WAL mode (Confidence: 80)**

`sqlite3.connect(args.db)` uses the default journal mode. For a long-running batch job writing embeddings for 1.2M files, WAL mode (`PRAGMA journal_mode=WAL`) would improve write throughput and allow concurrent reads. This is a performance concern, not a correctness bug, but on a 4TB library this matters operationally.

**Version byte layout — Python/Rust agreement (verified correct)**

`embed_train.py` `write_codebook` encodes the version as `new_version.to_bytes(8, "little", signed=False)`. The plan pseudocode had `version.to_bytes(8, 'little')` (omitting `signed`). The implementation correctly adds `signed=False`. The 8-byte LE encoding matches. No issue.

**`embed_encode.py` — silent preprocess failures (Confidence: 80)**

`encode_rows()` docstring says "Rows that fail preprocessing are skipped silently." The implementation logs nothing when `preprocess()` returns `None`. For a batch job over 1.2M files, silent skips accumulate invisibly. A debug-level count of skipped files per batch (even just a final summary) would be operationally important. `preprocess()` itself catches all exceptions (`except Exception: return None`) with no logging.

---

### Test Coverage

**P1.1 — Rust round-trip gap (Confidence: 90)**

The plan states P1.1 "invokes `cargo test -p riffgrep pq::tests::codebook_roundtrip` on a file fixture." The actual `test_codebook_rust_compat.py` does NOT invoke Rust at all. The two tests that run without the `requires_clap_model` gate (`test_codebook_layout_round_trips`, `test_rust_offset_formula_matches_flat_buffer`) only exercise Python-side serialization and a Python-reimplemented version of the Rust offset formula. They never call `pq::ProductQuantizer::from_bytes`. The commit message says "Rust-compat" but the cross-language round-trip is not tested.

This is the most significant test gap. The claim in the commit and P1.1 description is not met: a layout bug in FAISS's centroid ordering that Python gets right but Rust reads with a different assumption would not be caught.

**`requires_clap_model` marker — no automatic skip hook (Confidence: 85)**

`pyproject.toml` registers the `requires_clap_model` marker, but there is no `pytest_configure` or `pytest_collection_modifyitems` hook in `conftest.py` that automatically skips tests carrying it. The `test_ranking_sanity.py` tests will skip cleanly because they all depend on the `ranking_env` fixture, which calls `pytest.skip()` when the env vars are absent. But `test_codebook_rust_compat.py:test_faiss_codebook_matches_layout` uses `@pytest.mark.requires_clap_model` as its only marker, then relies on `pytest.importorskip("faiss")` to skip if FAISS is absent. In an environment where FAISS is installed but the CLAP checkpoint is not, this test will run (it doesn't need a checkpoint), so the marker is misleading but not broken.

**`hypothesis`/`proptest` for Python transforms — absent (Confidence: 85)**

Per CLAUDE.md: "Property-based testing is mandatory for any module that parses, encodes, or transforms data." `embed_preprocess.py` and the serialization path in `embed_train.py::train()` are data-transform modules. All five tests are example-based with fixed inputs. No `hypothesis` strategies are present. The plan does not invoke `hypothesis` either, so this was an intentional choice — but the CLAUDE.md convention is not met for the Python side.

**Edge cases missing from `embed_preprocess.py`:**
- Zero-length file (empty audio)
- Mono vs stereo (tested by `data.ndim == 2` but no test covers stereo input)
- File shorter than 1 sample after trim (covered implicitly by `len(data) == 0` guard but no explicit test)
- Non-WAV format fallback (soundfile exception path is silently `None`; no test)

**P1.2 idempotence test correctness**

`test_second_run_is_noop` correctly verifies zero writes on second call. The `_run` helper uses `_select_rows(conn, limit=None)` which returns zero rows when all are populated. The `encode_rows` call with an empty list returns 0. Pass.

**P1.5 loop filtering**

The `test_loop_rows_never_embedded` test uses `category='LOOP/BREAKS'` and `category='LOOP/DNB'`. These start with `LOOP` so they match the `NOT LIKE 'LOOP%'` SQL gate. The test correctly asserts `written == 3`. Pass.

---

### Plan Conformance

| Task | Landed | Notes |
|------|--------|-------|
| 1. pyproject.toml | Yes | Missing `pytest-xdist>=3.0` from plan's dev deps |
| 2. embed_preprocess.py | Yes | Signature uses `*` keyword-only vs plan's positional — acceptable |
| 3. embed_encode.py | Yes | Matches plan algorithm |
| 4. embed_train.py | Yes | Uses Python-side sampling instead of SQL `random_seeded` (documented in code) |
| 5. P1.1 test | Partial | Python-only layout test; no actual Rust invocation |
| 5. P1.2 test | Yes | |
| 5. P1.3 test | Yes | Gated correctly |
| 5. P1.4 test | Yes | |
| 5. P1.5 test | Yes | |

The plan explicitly defers BEXT `[128:256]` mirror to Plan 2. However, `doc/designs/embedding-human.md` and `doc/PICKER_SCHEMA.md` have been updated in this diff to describe the BEXT mirror as already designed and adopted (the schema doc now says `pq_code` instead of "Reserved"). This is documentation work only — no code writes to BEXT — but it represents a design pivot that was supposed to be Plan 2 work. The `Deferred` section of the plan still says "BEXT `[128:256]` mirror — Plan 2" while the design docs now present it as a done decision. This contradiction is acceptable (design docs are ahead of implementation) but reviewers should note the plan's deferred section is now stale.

**`pytest-xdist` missing:** Plan task 1 specifies `dev = ["pytest>=8.0", "pytest-xdist>=3.0"]`. Pyproject has only `pytest>=8.0`. The tests don't use `pytest-xdist` features, so this is a minor conformance miss.

**Out-of-plan additions:** The `doc/designs/embedding-human.md` BEXT schema section (64 lines added) was not in the plan's tasks. It's documentation only and consistent with the sprint's direction, but strictly it was a deferred item being pre-specified.

---

### Risks

**Memory risk in `_fetch_training_vectors` at scale (Confidence: 90)**

`scripts/embed_train.py` lines ~810-833. At 1.2M embedded rows × 2048 bytes = 2.46 GB of raw BLOB data loaded into a Python list before sampling. At 100k rows (early state) this is fine. This should be addressed before the library reaches full scale. A streaming approach using `SELECT id FROM samples ... ORDER BY RANDOM() LIMIT ?` with a fixed seed approximation, or SQLite's `RANDOM()` with a Python re-seed, would cap memory to `n_train * 2048` bytes.

**CLAP model + checkpoint mismatch — no runtime validation (Confidence: 80)**

`embed_encode.py` `_load_laion_clap` loads with `enable_fusion=False, amodel="HTSAT-base"`. The plan and design docs reference `music_audioset_epoch_15_esc_90.14.pt` as the target checkpoint. This checkpoint was trained with those parameters, so the pairing is correct. However, if a user points `LAION_CLAP_CHECKPOINT` at a fusion checkpoint or a different architecture, `load_ckpt` will either fail with a cryptic PyTorch error or silently load with shape mismatches that produce garbage embeddings without a clear error. No validation of the checkpoint filename or architecture metadata is performed.

**Security — no path traversal guard on audio files (low severity)**

`preprocess(row[1])` passes the DB-stored path directly to `soundfile.read()`. If an attacker can inject a path into the `samples` table (e.g., via a malicious DB file), they can cause the script to read arbitrary files. The risk is low in the operational context (the DB is the user's own index) but worth noting if the tooling is ever exposed to untrusted input.

**Pickle/model loading:** `laion_clap` uses PyTorch `torch.load()` internally, which deserializes pickles. Loading a `.pt` checkpoint from an untrusted source is a known arbitrary-code-execution vector. The default path is `~/Library/Application Support/riffgrep/models/` (user-owned), so the risk is low in practice.

---

### Recommendations

**Must fix before push:**

1. **P1.1 Rust round-trip is not tested.** The plan says P1.1 "invokes `cargo test -p riffgrep pq::tests::codebook_roundtrip`." The test does not. Either: (a) add a subprocess call to `cargo test` with a known-value fixture blob, or (b) amend the plan's P1.1 description to accurately describe what the test actually proves (Python-side layout only), so the commit message "Rust-compat" is not misleading. Option (b) is acceptable if invoking Rust from Python tests is not practical, but the discrepancy between the plan and the implementation must be resolved.
   - File: `scripts/tests/test_codebook_rust_compat.py`
   - File: `doc/plans/plan-2026-04-17-01.md`

2. **`requires_clap_model` marker has no skip hook.** Add a `pytest_collection_modifyitems` hook to `conftest.py` that auto-skips tests with this marker when `LAION_CLAP_CHECKPOINT` is absent. Without it, `test_faiss_codebook_matches_layout` coincidentally skips via `importorskip` but the marker's semantics are inconsistent.
   - File: `scripts/tests/conftest.py`

3. **`pytest-xdist` missing from pyproject.toml dev deps.** Plan task 1 specifies it. Add `"pytest-xdist>=3.0"` to `[project.optional-dependencies].dev`.
   - File: `pyproject.toml`

**Follow-up (future work):**

4. **`_fetch_training_vectors` memory ceiling.** At full 1.2M library scale, loading all embedding BLOBs before sampling consumes ~2.5 GB. Stream row IDs first, sample in Python, then fetch only the sampled BLOBs in batches.
   - File: `scripts/embed_train.py`

5. **FAISS centroid layout — add a round-trip spot check.** Add a check that `_reference_decode(blob)[0, 0, :]` equals the result of calling the FAISS PQ's `decode` on a hand-crafted code `[0, 0, 0, ..., 0]` (all-zeros centroid indices). This would catch a layout transposition.
   - File: `scripts/tests/test_codebook_rust_compat.py`

6. **Silent preprocess failures.** Add a logged count of skipped files at the end of `encode_rows()`. A single `print(f"skipped {n_skipped} files (preprocess failed)")` in `encode_rows` is sufficient.
   - File: `scripts/embed_encode.py`

7. **Plan deferred section is stale.** The BEXT mirror design is now documented as adopted in `embedding-human.md` and `PICKER_SCHEMA.md`, but the plan's `Deferred` section still says "BEXT `[128:256]` mirror — Plan 2." Update the plan's deferred section or add a note that the design decision was made in Plan 1 and implementation is Plan 2.
   - File: `doc/plans/plan-2026-04-17-01.md`

8. **`hypothesis` property tests for `embed_preprocess.py` transforms.** CLAUDE.md mandates property-based testing for modules that transform data. Add tests for: invariant that output length == `n_samples` regardless of input length; invariant that peak amplitude after normalize is within tolerance of `_db_to_amp(PEAK_DB)`; invariant that all-silence input returns `None`.
   - File: new `scripts/tests/test_preprocess_properties.py`

### Resolution (pre-push)

All three must-fix items above were addressed before the initial push to PR #10:

| # | Item | Fix commit |
|---|------|------------|
| 1 | P1.1 Rust round-trip claim | `7f91e9a` — plan description + test docstring now accurately describe what each side proves |
| 2 | `requires_clap_model` skip hook | `443a751` — `pytest_collection_modifyitems` added to `conftest.py` |
| 3 | `pytest-xdist` missing from dev deps | `c48d488` — added `"pytest-xdist>=3.0"` to `[project.optional-dependencies].dev` |

Follow-ups 4–8 remain open and are tracked for a later sprint.

<!-- gh-id: 3104265037 -->
### Copilot on [`scripts/embed_encode.py:152`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104265037) (2026-04-18 02:01 UTC)

Progress accounting is incorrect: skipped rows already call `pbar.update(1)` in the inner loop, but this update adds `len(batch)` again (via `len(audios) + (len(batch) - len(audios))`). That double-counts any preprocess failures and can push the progress bar past 100%. Update the bar exactly once per input row (either only here with `len(batch)` or only in the inner loop).


<!-- gh-id: 3104265048 -->
### Copilot on [`scripts/embed_preprocess.py:8`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104265048) (2026-04-18 02:01 UTC)

The module docstring says soundfile "falls back to librosa's audioread for non-PCM", but the implementation only uses `soundfile.read()` and returns `None` on failure (no fallback path). Either implement the documented fallback (e.g., try librosa.load on exception) or adjust the docstring to match actual behavior.


<!-- gh-id: 3104265063 -->
### Copilot on [`pyproject.toml:13`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104265063) (2026-04-18 02:01 UTC)

`laion-clap` and `faiss-cpu` are listed as unconditional runtime dependencies, but the PR description/test setup suggests the default developer workflow should run the test suite without heavy/fragile deps. Consider moving heavyweight/optional packages (at least `faiss-cpu`, possibly `laion-clap`) into optional extras (e.g., `clap`/`faiss`) so contributors on unsupported platforms can still install and run the lightweight tests.

<!-- gh-id: 3104265071 -->
### Copilot on [`scripts/tests/test_ranking_sanity.py:30`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104265071) (2026-04-18 02:01 UTC)

`embed_train` is imported but never used in this test. If this was intended to ensure the module is importable, a clearer assertion/comment would help; otherwise please remove the unused import to keep the test focused.


<!-- gh-id: 3104265080 -->
### Copilot on [`doc/designs/embedding-human.md:14`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104265080) (2026-04-18 02:01 UTC)

This doc now states that PQ codes are mirrored into BEXT `[128:256]` and used for `--no-db` similarity search, but this PR's implementation explicitly defers the BEXT mirror to Plan 2 and there is no code writing/reading `pq_code` yet. Please clarify that the BEXT mirror is a planned/future capability (or split this doc update into the Plan 2 PR) to avoid documenting behavior that doesn't exist.

<!-- gh-id: 3104265085 -->
### Copilot on [`doc/PICKER_SCHEMA.md:79`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104265085) (2026-04-18 02:01 UTC)

This schema change documents `pq_code` living in BEXT Description `[128:256]`, but Plan 1 code in this PR does not write/read BEXT at all (and the PR description says the BEXT mirror is Plan 2). Please mark this field as planned/future until the implementation lands, or defer the schema doc change to the Plan 2 PR to keep docs aligned with shipped behavior.

<!-- gh-id: 3104265088 -->
### Copilot on [`scripts/embed_train.py:72`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104265088) (2026-04-18 02:01 UTC)

`_fetch_training_vectors()` calls `.fetchall()` on `SELECT id, embedding ...` without a LIMIT, which loads *all* embeddings into Python memory before sampling. On a large library this is likely to be multi-GB of BLOB data and can OOM or stall due to massive I/O. Prefer sampling in SQL (deterministic pseudo-random order using `id` + `seed`, then `LIMIT n_train`) so only `n_train` embeddings are read, or use a streaming/reservoir sampler that doesn't materialize the full result set.


<!-- gh-id: 3104265099 -->
### Copilot on [`scripts/embed_encode.py:177`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104265099) (2026-04-18 02:01 UTC)

`model_path` is not normalized/expanded when provided via `--model` (argparse leaves `~` unexpanded). That makes `--model ~/...` fail even when the checkpoint exists. Consider applying `.expanduser()` to `args.model` / `model_path` (and similarly for the env-var path if you want to support `~` there too).


<!-- gh-id: 3104265116 -->
### Copilot on [`scripts/tests/test_ranking_sanity.py:7`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104265116) (2026-04-18 02:01 UTC)

The docstring claims this test "runs the real CLAP pipeline ..." and implies a full similar-search ranking, but the body only does brute-force L2 over the stored embeddings (no PQ training and no call into `rfg --similar`). Please update the docstring so it reflects what the test actually validates, to avoid misleading future debugging/calibration work.


<!-- gh-id: 3104265123 -->
### Copilot on [`scripts/tests/test_codebook_rust_compat.py:81`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104265123) (2026-04-18 02:01 UTC)

This FAISS-only test is marked `requires_clap_model`, but it doesn't require a CLAP checkpoint (it only `importorskip("faiss")` and trains on random vectors). With the current `pytest_collection_modifyitems` hook, it will be skipped unless `LAION_CLAP_CHECKPOINT` is set, reducing coverage unnecessarily. Consider using a separate marker (e.g., `requires_faiss`) or dropping the marker and relying solely on `importorskip`.


<!-- gh-id: 4133231540 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-18 02:01 UTC](https://github.com/cmk/riffgrep/pull/10#pullrequestreview-4133231540))

## Pull request overview

Implements “Plan 1” of the embedding roadmap by adding a Python pipeline to (1) preprocess audio for LAION-CLAP, (2) encode and store 512-dim embeddings into `samples.embedding`, and (3) train/serialize a FAISS PQ codebook into the SQLite `metadata` table, plus a new pytest suite and accompanying docs.

**Changes:**
- Add Python scripts for audio preprocessing, CLAP embedding encoding into SQLite, and FAISS PQ codebook training/metadata installation.
- Add pytest fixtures + property/integration tests (with CLAP/FAISS-gated tests) for idempotency, LOOP skipping, embedding blob invariants, and Rust/PQ layout compatibility.
- Add/adjust documentation for the embedding plan and BEXT schema notes; add Python project config + ignore rules.

### Reviewed changes

Copilot reviewed 14 out of 16 changed files in this pull request and generated 13 comments.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| scripts/embed_preprocess.py | Audio loading/resampling/trimming/windowing/normalization used by embedding encode. |
| scripts/embed_encode.py | Batch CLAP inference + embedding serialization and DB updates for eligible rows. |
| scripts/embed_train.py | FAISS PQ training + Rust-compatible codebook serialization and metadata writes. |
| scripts/tests/conftest.py | SQLite test DB fixtures + marker-based skipping logic. |
| scripts/tests/test_codebook_rust_compat.py | Verifies Python-produced codebook byte layout matches Rust offset formula (+ optional FAISS training test). |
| scripts/tests/test_encode_idempotent.py | Ensures encoding is idempotent via `embedding IS NULL` gating. |
| scripts/tests/test_embedding_norm.py | Validates embedding blob size/endianness and near-unit-norm invariant. |
| scripts/tests/test_loop_skipped.py | Ensures LOOP-category rows are excluded from encoding. |
| scripts/tests/test_ranking_sanity.py | Optional real-model “ranking sanity” test over a labeled fixture corpus. |
| scripts/tests/__init__.py | Test package marker file. |
| pyproject.toml | Declares Python deps + pytest configuration/markers. |
| doc/plans/plan-2026-04-17-01.md | Plan/spec for the Plan 1 pipeline and its tests. |
| doc/designs/embedding-human.md | Updates embedding design doc (including BEXT/PQ mirror narrative). |
| doc/PICKER_SCHEMA.md | Documents `[128:256]` as `pq_code` in the packed schema. |
| .gitignore | Ignores common Python build/test artifacts. |
</details>






---

💡 <a href="https://github.com/cmk/riffgrep/new/main?filename=.github/instructions/*.instructions.md" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Add Copilot custom instructions</a> for smarter, more guided reviews. <a href="https://docs.github.com/en/copilot/customizing-copilot/adding-repository-custom-instructions-for-github-copilot" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Learn how to get started</a>.

<!-- gh-id: 3104265133 -->
### Copilot on [`doc/plans/plan-2026-04-17-01.md:170`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104265133) (2026-04-18 02:01 UTC)

This plan doc says `test_ranking_sanity.py` "runs encode + train + query", but the current test implementation only encodes embeddings and does a brute-force L2 ranking in Python (no PQ training, no call into the CLI). Please adjust the description so it matches what the test actually does, or update the test to perform the described steps.

<!-- gh-id: 3104265140 -->
### Copilot on [`scripts/embed_train.py:152`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104265140) (2026-04-18 02:01 UTC)

`--db` is parsed as a `Path` but `~` is not expanded for user-provided values, so `--db ~/...` will fail the `exists()` check even when the file exists. Consider normalizing with `.expanduser()` (and optionally `.resolve()`) after parsing.


<!-- gh-id: 3104265148 -->
### Copilot on [`scripts/embed_encode.py:174`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104265148) (2026-04-18 02:01 UTC)

`--db` is parsed as a `Path` but `~` is not expanded for user-provided values, so `--db ~/...` will fail the `exists()` check even when the file exists. Consider normalizing with `.expanduser()` (and optionally `.resolve()`) after parsing.

<!-- gh-id: 3104278056 -->
#### ↳ cmk ([2026-04-18 02:10 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104278056))

Fixed in fcf96c1 — removed the inner-loop `pbar.update(1)` and advance by `len(batch)` exactly once per batch, including the all-failed short-circuit path.

<!-- gh-id: 3104278103 -->
#### ↳ cmk ([2026-04-18 02:10 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104278103))

Fixed in d5c196a — docstring now describes actual behavior (soundfile load, `None` on failure, librosa used only for resampling). No decoder fallback in this sprint.

<!-- gh-id: 3104278195 -->
#### ↳ cmk ([2026-04-18 02:10 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104278195))

Deferred — valid but broader than this PR. Splitting `laion-clap`/`faiss-cpu` into optional extras touches the default `uv sync` path and wants its own round. Tracked as follow-up.

<!-- gh-id: 3104278291 -->
#### ↳ cmk ([2026-04-18 02:10 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104278291))

Fixed in fcf96c1 — removed the unused import.

<!-- gh-id: 3104278450 -->
#### ↳ cmk ([2026-04-18 02:10 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104278450))

Intentional. The BEXT mirror is Plan 2's implementation target; the design doc is the contract that unblocks it. Not flipping to "planned/future" because this repo's flow is design-first-then-implement, and `pq_code` at `[128:256]` is the locked decision for Plan 2.

<!-- gh-id: 3104278548 -->
#### ↳ cmk ([2026-04-18 02:10 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104278548))

Intentional — same rationale as the `embedding-human.md` thread. Schema doc describes the locked layout Plan 2 will implement; keeping the schema ahead of the code is the repo convention.

<!-- gh-id: 3104278618 -->
#### ↳ cmk ([2026-04-18 02:10 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104278618))

Accepted as follow-up — same finding appeared in the local review (`doc/reviews/review-0010.md`, follow-up 4). Fine at the current ~100K scale; a streaming ID-sample rewrite goes in before the library hits full 1.2M.

<!-- gh-id: 3104278720 -->
#### ↳ cmk ([2026-04-18 02:10 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104278720))

Fixed in fcf96c1 — `model_path` now `.expanduser()`s the CLI and env-var paths together.

<!-- gh-id: 3104278777 -->
#### ↳ cmk ([2026-04-18 02:10 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104278777))

Fixed in d5c196a — docstring now states brute-force L2 in Python over stored full-precision embeddings; no PQ training, no CLI invocation (those belong to Plan 3).

<!-- gh-id: 3104278817 -->
#### ↳ cmk ([2026-04-18 02:10 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104278817))

Fixed in fcf96c1 — dropped the `requires_clap_model` marker. `importorskip('faiss')` is the only gate this test needs.

<!-- gh-id: 3104278854 -->
#### ↳ cmk ([2026-04-18 02:10 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104278854))

Fixed in d5c196a — plan P1.3 description now matches what the test actually does, and calls out that PQ training + CLI coverage are Plan 3's integration gate.

<!-- gh-id: 3104278909 -->
#### ↳ cmk ([2026-04-18 02:10 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104278909))

Fixed in fcf96c1 — `args.db = args.db.expanduser()` immediately after argparse.

<!-- gh-id: 3104278957 -->
#### ↳ cmk ([2026-04-18 02:10 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104278957))

Fixed in fcf96c1 — `args.db = args.db.expanduser()` immediately after argparse.

<!-- gh-id: 3104285928 -->
### Copilot on [`scripts/tests/test_ranking_sanity.py:108`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104285928) (2026-04-18 02:13 UTC)

The comment says "For each category, pick one file as the query" but the loop actually treats every stored file as a query. Update the comment to match the behavior (or change the loop if the intent was one query per category).


<!-- gh-id: 3104285947 -->
### Copilot on [`scripts/tests/test_ranking_sanity.py:122`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104285947) (2026-04-18 02:13 UTC)

`ok_ratio = 1.0 - len(violations) / len(stored)` will raise `ZeroDivisionError` if no embeddings were stored (e.g., if all fixture audio fails preprocessing / is unreadable). Add a guard to `skip` (or fail with a clearer message) when `stored` is empty (and possibly when it's below a minimum size after encoding).

<!-- gh-id: 4133261906 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-18 02:13 UTC](https://github.com/cmk/riffgrep/pull/10#pullrequestreview-4133261906))

## Pull request overview

Adds the “Plan 1” Python embedding pipeline to populate `samples.embedding` with LAION-CLAP vectors and train/store a FAISS PQ codebook in SQLite `metadata`, plus a pytest suite and supporting docs/config so Rust similarity search can use the populated DB automatically.

**Changes:**
- Add Python scripts for audio preprocessing, CLAP embedding encoding into SQLite, and FAISS PQ codebook training/serialization.
- Add pytest fixtures and tests covering encoding idempotence, LOOP skipping, embedding blob invariants, ranking sanity (gated), and PQ codebook byte-layout compatibility.
- Add Python project config (`pyproject.toml`), Python `.gitignore` entries, and documentation updates for the embedding/PQ roadmap and schema.

### Reviewed changes

Copilot reviewed 14 out of 16 changed files in this pull request and generated 4 comments.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| scripts/embed_preprocess.py | Implements shared audio load/resample/trim/window/normalize preprocessing for CLAP inference. |
| scripts/embed_encode.py | Encodes eligible rows with CLAP, serializes 512×f32 LE blobs, and writes to `samples.embedding`. |
| scripts/embed_train.py | Samples embeddings, trains FAISS PQ, serializes codebook blob, and writes it + version to `metadata`. |
| scripts/tests/conftest.py | Adds minimal SQLite schema fixtures and skip hook for CLAP-gated tests. |
| scripts/tests/test_codebook_rust_compat.py | Validates Python-produced codebook blob layout matches Rust indexing expectations; optional FAISS training check. |
| scripts/tests/test_encode_idempotent.py | Tests that a second encode run produces zero additional updates. |
| scripts/tests/test_embedding_norm.py | Tests embedding blob length/endianness and near-unit-norm invariant. |
| scripts/tests/test_loop_skipped.py | Tests LOOP-category rows are excluded from embedding writes. |
| scripts/tests/test_ranking_sanity.py | Gated real-model ranking sanity test over labeled fixture corpus using brute-force L2 in Python. |
| scripts/tests/__init__.py | Marks the tests directory as a package. |
| pyproject.toml | Declares Python dependencies, dev deps, and pytest config/markers for scripts/tests. |
| doc/plans/plan-2026-04-17-01.md | Documents Plan 1 goals, algorithms, and test invariants for embedding + PQ training. |
| doc/designs/embedding-human.md | Updates embedding design, including PQ/BEXT mirroring design rationale and schema. |
| doc/PICKER_SCHEMA.md | Documents BEXT `[128:256]` as `pq_code` (Plan 2 target layout). |
| doc/reviews/review-0010.md | Captures local review notes and follow-ups for this Plan 1 work. |
| .gitignore | Adds Python cache/venv/pytest ignore patterns. |
</details>






---

💡 <a href="https://github.com/cmk/riffgrep/new/main?filename=.github/instructions/*.instructions.md" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Add Copilot custom instructions</a> for smarter, more guided reviews. <a href="https://docs.github.com/en/copilot/customizing-copilot/adding-repository-custom-instructions-for-github-copilot" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Learn how to get started</a>.

<!-- gh-id: 3104285959 -->
### Copilot on [`scripts/embed_train.py:115`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104285959) (2026-04-18 02:13 UTC)

`_current_version()` silently returns 0 when `pq_codebook_version` exists but is malformed (wrong type/length). That can cause the version counter to reset and break the “monotonic version” contract without any signal. Prefer treating malformed values as an error (raise with details) and only default to 0 when the key is missing.


<!-- gh-id: 3104285977 -->
### Copilot on [`scripts/tests/test_encode_idempotent.py:36`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104285977) (2026-04-18 02:13 UTC)

`_run(db_path, n)` takes an `n` parameter that is never used, which makes the helper misleading and can hide future mistakes (e.g., thinking the helper limits the run). Remove the unused parameter (and adjust call sites) or use it to assert the expected number of rows were written.

<!-- gh-id: 3104293574 -->
#### ↳ cmk ([2026-04-18 02:16 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104293574))

Fixed in b754dad — comment now matches the all-files loop. Iterating every stored file as a query is the intended, more-robust design (gives a per-query hit-rate rather than a single-file sample).

<!-- gh-id: 3104293610 -->
#### ↳ cmk ([2026-04-18 02:16 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104293610))

Fixed in b754dad — early `pytest.skip` when `stored` is empty, before the ratio calc.

<!-- gh-id: 3104293653 -->
#### ↳ cmk ([2026-04-18 02:16 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104293653))

Fixed in b754dad — `_current_version` now raises `RuntimeError` with a diagnostic on malformed rows. 0 is returned only when the key is absent.

<!-- gh-id: 3104293698 -->
#### ↳ cmk ([2026-04-18 02:16 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104293698))

Fixed in b754dad — removed the unused `n` param and adjusted call sites.

<!-- gh-id: 3104299451 -->
### Copilot on [`scripts/embed_encode.py:114`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104299451) (2026-04-18 02:19 UTC)

`encode_rows()` materializes the entire `rows` iterable (`rows = list(rows)`) and `_select_rows()` already does a `.fetchall()` into a Python list. On a large DB this can be a significant, unnecessary memory spike and delays starting work. Prefer streaming: iterate directly over a cursor (or `fetchmany`) and chunk batches without converting the full selection to a list, so memory stays O(batch_size).

<!-- gh-id: 3104299456 -->
### Copilot on [`scripts/embed_preprocess.py:41`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104299456) (2026-04-18 02:19 UTC)

`_trim_silence()` returns the original audio when *all* samples are below the silence threshold (`if not mask.any(): return audio`). That means an “all-silent (below -60 dBFS)” file won’t be treated as silence: it will later be peak-normalized, potentially amplifying low-level noise, which contradicts the module docstring’s “trim silence (-60 dBFS)” and `preprocess()`’s “entirely silence → None” contract. Consider returning an empty array (or having `preprocess()` treat `not mask.any()` as silence) so these files are skipped consistently.


<!-- gh-id: 3104299458 -->
### Copilot on [`scripts/tests/test_ranking_sanity.py:82`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104299458) (2026-04-18 02:19 UTC)

The test docstring specifies fixture structure constraints (≥5 category subdirs, each with ≥10 WAVs), but the test only checks `len(rows) >= 50`. With fewer categories (or a highly imbalanced corpus), the assertion can become trivial and less meaningful. Consider enforcing these per-category minimums (or `skip` with a clear message) so the test actually validates cross-category ranking behavior as described.


<!-- gh-id: 4133281315 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-18 02:19 UTC](https://github.com/cmk/riffgrep/pull/10#pullrequestreview-4133281315))

## Pull request overview

Implements Plan 1 of the embedding roadmap by adding a Python pipeline to (1) preprocess audio, (2) encode LAION-CLAP embeddings into `samples.embedding`, and (3) train/serialize a FAISS PQ codebook into SQLite `metadata`, plus a pytest suite and supporting docs/config so existing Rust similarity paths can operate on populated DBs.

**Changes:**
- Add `embed_preprocess.py`, `embed_encode.py`, and `embed_train.py` to generate/store CLAP embeddings and a Rust-compatible PQ codebook in SQLite.
- Add `scripts/tests/` fixtures + tests covering idempotence, LOOP skipping, embedding blob invariants, PQ layout compatibility, and an optional real-model ranking sanity check.
- Add Python project configuration (`pyproject.toml`), update embedding/schema documentation, and extend `.gitignore` for Python artifacts.

### Reviewed changes

Copilot reviewed 14 out of 16 changed files in this pull request and generated 4 comments.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| scripts/embed_preprocess.py | Audio load/resample/trim/window/normalize preprocessing for CLAP inference. |
| scripts/embed_encode.py | Batch CLAP inference, embedding serialization (LE f32), and DB updates for eligible rows. |
| scripts/embed_train.py | Sample embeddings, train FAISS PQ, serialize codebook blob, and write codebook + version to `metadata`. |
| scripts/tests/conftest.py | Minimal SQLite schema + fixtures and skip hook for CLAP-gated tests. |
| scripts/tests/test_codebook_rust_compat.py | Validates codebook byte layout vs Rust indexing expectations; optional FAISS training check. |
| scripts/tests/test_encode_idempotent.py | Ensures a second encode run performs no additional writes. |
| scripts/tests/test_embedding_norm.py | Verifies embedding blob size/endianness and near-unit-norm invariant. |
| scripts/tests/test_loop_skipped.py | Ensures LOOP-category rows are excluded from embedding writes. |
| scripts/tests/test_ranking_sanity.py | Optional real-model ranking sanity check using brute-force L2 over stored embeddings. |
| scripts/tests/__init__.py | Marks the tests directory as a package. |
| pyproject.toml | Declares Python deps/dev-deps and pytest configuration/marker registration. |
| doc/reviews/review-0010.md | Local review notes, follow-ups, and resolved comment threads for this plan. |
| doc/plans/plan-2026-04-17-01.md | Plan/spec describing algorithms and test invariants for Plan 1. |
| doc/designs/embedding-human.md | Embedding design documentation including PQ+BEXT mirroring contract for Plan 2. |
| doc/PICKER_SCHEMA.md | Documents BEXT `[128:256]` as `pq_code` layout for future implementation. |
| .gitignore | Adds Python cache/venv/pytest ignores. |
</details>






---

💡 <a href="https://github.com/cmk/riffgrep/new/main?filename=.github/instructions/*.instructions.md" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Add Copilot custom instructions</a> for smarter, more guided reviews. <a href="https://docs.github.com/en/copilot/customizing-copilot/adding-repository-custom-instructions-for-github-copilot" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Learn how to get started</a>.

<!-- gh-id: 3104299460 -->
### Copilot on [`pyproject.toml:25`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104299460) (2026-04-18 02:19 UTC)

The `requires_clap_model` marker description says tests are “skipped unless LAION_CLAP_CHECKPOINT is set and the fixture corpus exists”, but the skip hook in `scripts/tests/conftest.py` only checks `LAION_CLAP_CHECKPOINT`, and not all marked tests necessarily require a fixture corpus. Updating this marker text to match the actual skip semantics (checkpoint only; individual tests may gate on additional env vars) will prevent confusion.


<!-- gh-id: 3104313217 -->
#### ↳ cmk ([2026-04-18 02:25 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104313217))

Declined for this sprint — same scale-hardening bucket as `_fetch_training_vectors` (tracked as follow-up in `review-0010.md`). `(id, path)` tuples are \~200MB at 1.2M scale vs multi-GB for BLOBs, so it's the second priority after the train-side rewrite.

<!-- gh-id: 3104313270 -->
#### ↳ cmk ([2026-04-18 02:25 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104313270))

Fixed in c0e0a46 — `_trim_silence` now returns an empty array when no sample exceeds the threshold, so the caller's `len(data) == 0` guard converts to `None`. Previous behavior would peak-normalize sub-threshold noise up to -1 dBFS.

<!-- gh-id: 3104313329 -->
#### ↳ cmk ([2026-04-18 02:25 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104313329))

Fixed in c0e0a46 — added per-category enforcement (≥5 categories, ≥10 WAVs each) before the test runs.

<!-- gh-id: 3104313391 -->
#### ↳ cmk ([2026-04-18 02:25 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104313391))

Fixed in c0e0a46 — marker description now matches the conftest hook's actual semantics (checkpoint only; individual tests may add env-var gates at runtime).

<!-- gh-id: 3104322910 -->
### Copilot on [`scripts/tests/test_codebook_rust_compat.py:24`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104322910) (2026-04-18 02:30 UTC)

The docstring references `doc/reviews/review-0008.md`, but that file doesn't exist in the repo (reviews jump from review-0007 to review-0010). Update the reference to the correct review doc (likely `doc/reviews/review-0010.md`) or remove the link so contributors can find the follow-up item.


<!-- gh-id: 4133310353 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-18 02:30 UTC](https://github.com/cmk/riffgrep/pull/10#pullrequestreview-4133310353))

## Pull request overview

Implements Plan 1 of the embedding roadmap by adding a Python pipeline that (a) preprocesses audio for LAION-CLAP, (b) encodes/stores CLAP embeddings into SQLite `samples.embedding`, and (c) trains/serializes a FAISS PQ codebook into SQLite `metadata`, with a pytest suite and supporting docs/config so existing Rust similarity paths can operate once the DB is populated.

**Changes:**
- Add Python scripts for CLAP preprocessing/encoding and FAISS PQ codebook training + metadata installation.
- Add pytest fixtures and tests for codebook byte layout, encode idempotence, LOOP skipping, embedding blob invariants, and a gated real-model ranking sanity check.
- Add Python project configuration (`pyproject.toml`), documentation updates for Plan 1 + embedding design/schema, and Python-related `.gitignore` entries.

### Reviewed changes

Copilot reviewed 14 out of 16 changed files in this pull request and generated 3 comments.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| scripts/embed_preprocess.py | Audio load/resample/trim/window/normalize preprocessing for CLAP inference. |
| scripts/embed_encode.py | Batch CLAP inference, embedding serialization (LE f32), and DB updates for eligible rows. |
| scripts/embed_train.py | Sample embeddings, train FAISS PQ, serialize codebook blob, and write codebook + version to `metadata`. |
| scripts/tests/conftest.py | Minimal SQLite schema/fixtures + marker-based skip hook for CLAP-gated tests. |
| scripts/tests/test_codebook_rust_compat.py | Validates codebook blob layout matches Rust indexing assumptions (plus optional FAISS training sanity). |
| scripts/tests/test_encode_idempotent.py | Tests encode idempotence via `embedding IS NULL` gating. |
| scripts/tests/test_embedding_norm.py | Verifies embedding blob size/endianness and near-unit-norm invariant. |
| scripts/tests/test_loop_skipped.py | Ensures LOOP-category rows are excluded from embedding writes. |
| scripts/tests/test_ranking_sanity.py | Gated real-model ranking sanity test using brute-force L2 over stored embeddings. |
| scripts/tests/__init__.py | Marks `scripts/tests` as a package. |
| pyproject.toml | Declares Python deps/dev-deps and pytest config/marker registration. |
| doc/reviews/review-0010.md | Local review notes and follow-ups for this Plan 1 work. |
| doc/plans/plan-2026-04-17-01.md | Plan/spec describing algorithms and test invariants for Plan 1. |
| doc/designs/embedding-human.md | Embedding design documentation, including PQ + BEXT mirroring contract for Plan 2. |
| doc/PICKER_SCHEMA.md | Documents BEXT `[128:256]` as `pq_code` layout for future implementation. |
| .gitignore | Adds ignores for common Python cache/venv/pytest artifacts. |
</details>






---

💡 <a href="https://github.com/cmk/riffgrep/new/main?filename=.github/instructions/*.instructions.md" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Add Copilot custom instructions</a> for smarter, more guided reviews. <a href="https://docs.github.com/en/copilot/customizing-copilot/adding-repository-custom-instructions-for-github-copilot" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Learn how to get started</a>.

<!-- gh-id: 3104322921 -->
### Copilot on [`doc/reviews/review-0010.md:107`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104322921) (2026-04-18 02:30 UTC)

This review doc says `pytest-xdist` is missing from `pyproject.toml` (and lists it as a must-fix), but `pyproject.toml` in this PR already includes `pytest-xdist>=3.0` under `dev`. Either update these sections to reflect the current state (e.g., mark the item as resolved with a commit id) or remove the now-stale finding to avoid misleading readers.


<!-- gh-id: 3104322926 -->
### Copilot on [`doc/reviews/review-0010.md:142`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104322926) (2026-04-18 02:30 UTC)

The "Must fix before push" list still includes items that appear to be resolved in the current PR (e.g., adding the `requires_clap_model` skip hook and adding `pytest-xdist`). Consider annotating each must-fix with its resolution (commit hash) or moving resolved items into a separate "Resolved" subsection so this doc reads consistently end-to-end.


<!-- gh-id: 3104325224 -->
#### ↳ cmk ([2026-04-18 02:32 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104325224))

Fixed in 19878fc — stale `review-0008.md` reference updated to `review-0010.md` (renamed when PR #10 was opened).

<!-- gh-id: 3104325270 -->
#### ↳ cmk ([2026-04-18 02:32 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104325270))

Fixed in 19878fc — added a "Resolution (pre-push)" block to the Local Review section linking each must-fix to its fix SHA. The original finding is preserved above; the resolution table below makes the status unambiguous.

<!-- gh-id: 3104325307 -->
#### ↳ cmk ([2026-04-18 02:32 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104325307))

Fixed in 19878fc — same resolution table covers all three must-fix items (7f91e9a, 443a751, c48d488). Follow-ups 4-8 explicitly flagged as remaining open.

<!-- gh-id: 3104336181 -->
### Copilot on [`scripts/embed_train.py:148`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104336181) (2026-04-18 02:41 UTC)

`--n-train` is not validated. If it’s 0 or negative, `_fetch_training_vectors()` will return an empty or unexpectedly large sample (negative slicing on the permutation), and `faiss.ProductQuantizer.train()` will fail or do the wrong thing. Please enforce `n_train >= 1` (and ideally error out early with a clear message).


<!-- gh-id: 4133323823 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-18 02:41 UTC](https://github.com/cmk/riffgrep/pull/10#pullrequestreview-4133323823))

## Pull request overview

Implements Plan 1 of the embedding roadmap by adding a Python pipeline to (a) preprocess audio, (b) encode/store LAION-CLAP embeddings into `samples.embedding`, and (c) train/serialize a Rust-compatible FAISS PQ codebook into SQLite `metadata`, plus a pytest suite and supporting docs/config so existing Rust similarity paths can operate once the DB is populated.

**Changes:**
- Add `scripts/embed_preprocess.py` + `scripts/embed_encode.py` to generate and store 512×f32 LE CLAP embeddings in SQLite (skipping LOOP rows).
- Add `scripts/embed_train.py` to train a FAISS `ProductQuantizer` and atomically write the codebook + version to `metadata`.
- Add `scripts/tests/` with fixtures and tests for codebook layout compatibility, encoding idempotence, LOOP skipping, embedding invariants, and a gated real-model ranking sanity check.

### Reviewed changes

Copilot reviewed 14 out of 16 changed files in this pull request and generated 4 comments.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| scripts/embed_encode.py | Batch encode + DB update path for CLAP embeddings. |
| scripts/embed_preprocess.py | Shared audio preprocess (48kHz mono, trim, window, peak-normalize). |
| scripts/embed_train.py | PQ training + Rust-compatible codebook serialization + metadata install/version bump. |
| scripts/tests/conftest.py | Test DB fixtures + skip hook for CLAP-checkpoint-gated tests. |
| scripts/tests/test_codebook_rust_compat.py | Validates codebook blob layout vs Rust offset expectations (+ optional FAISS training sanity). |
| scripts/tests/test_encode_idempotent.py | Ensures second encode pass performs no additional writes. |
| scripts/tests/test_embedding_norm.py | Embedding blob size/endianness and near-unit-norm invariant check. |
| scripts/tests/test_loop_skipped.py | Ensures LOOP-category rows are excluded from embedding writes. |
| scripts/tests/test_ranking_sanity.py | Optional real-model ranking sanity check using brute-force L2 over stored embeddings. |
| scripts/tests/__init__.py | Marks `scripts/tests` as a package. |
| pyproject.toml | Declares Python deps/dev-deps and pytest config/markers. |
| doc/plans/plan-2026-04-17-01.md | Plan/spec for the Plan 1 pipeline and its invariants/tests. |
| doc/designs/embedding-human.md | Embedding design doc updates (including PQ + BEXT mirroring contract). |
| doc/PICKER_SCHEMA.md | Documents BEXT `[128:256]` as `pq_code` (Plan 2 target layout). |
| doc/reviews/review-0010.md | Local review log + resolutions/follow-ups. |
| .gitignore | Adds Python cache/venv/pytest ignores. |
</details>






---

💡 <a href="https://github.com/cmk/riffgrep/new/main?filename=.github/instructions/*.instructions.md" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Add Copilot custom instructions</a> for smarter, more guided reviews. <a href="https://docs.github.com/en/copilot/customizing-copilot/adding-repository-custom-instructions-for-github-copilot" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Learn how to get started</a>.

<!-- gh-id: 3104336188 -->
### Copilot on [`scripts/tests/test_codebook_rust_compat.py:24`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104336188) (2026-04-18 02:41 UTC)

Docstring says faiss is installed as a “dev dep”, but `faiss-cpu` is currently listed in `pyproject.toml` under main `[project].dependencies`. Either adjust the wording (e.g., “optional dependency for this test”) or move `faiss-cpu` to an extra in a follow-up to keep docs and packaging aligned.


<!-- gh-id: 3104336197 -->
### Copilot on [`scripts/embed_encode.py:52`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104336197) (2026-04-18 02:41 UTC)

`EMBED_BYTES` is defined but never used in this module. Consider removing it (or using it in a blob-length assertion) to avoid dead constants drifting from the actual serialized size.


<!-- gh-id: 3104336203 -->
### Copilot on [`scripts/embed_encode.py:75`](https://github.com/cmk/riffgrep/pull/10#discussion_r3104336203) (2026-04-18 02:41 UTC)

`_chunks()` assumes `n > 0`; with `--batch-size 0` (or a negative value) this will raise at runtime (`range()` step cannot be 0) or behave unexpectedly. Add validation (e.g., argparse `choices`/custom type, or an explicit check in `main()`/`encode_rows()`) to ensure `batch_size >= 1`.


<!-- gh-id: 3104338640 -->
#### ↳ cmk ([2026-04-18 02:43 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104338640))

Fixed in fd376f0 — added a `_positive_int` argparse type; `--n-train 0` or negative now fails at parse time with a clear message.

<!-- gh-id: 3104338661 -->
#### ↳ cmk ([2026-04-18 02:43 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104338661))

Fixed in fd376f0 — docstring now correctly describes `faiss-cpu` as a runtime dependency under `[project].dependencies` and notes that `importorskip` handles environments where the native wheel is unavailable.

<!-- gh-id: 3104338682 -->
#### ↳ cmk ([2026-04-18 02:43 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104338682))

Fixed in fd376f0 — `EMBED_BYTES` is now used in a serialization invariant check inside `encode_rows` (raises if an encoder's output ever serializes to the wrong length).

<!-- gh-id: 3104338710 -->
#### ↳ cmk ([2026-04-18 02:43 UTC](https://github.com/cmk/riffgrep/pull/10#discussion_r3104338710))

Fixed in fd376f0 — `--batch-size` uses the same `_positive_int` type, and `_chunks` guards `n >= 1` for library callers.
