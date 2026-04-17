# PR #4 — Harden embeddings and add PQ-accelerated search

## Local review (2026-04-15)

**Branch:** feat/embeddings
**Commits:** 3 (eb4589f..HEAD)
**Reviewer:** Claude (sonnet, independent)

---

### Must fix before push

1. **sqlite.rs:843** — single-slash comment (`/ v1 →`) should be `//`. Verify build is clean.
2. **sqlite.rs** — empty migration test section. Add test for v1→v2 upgrade path. Guard ALTER TABLE against duplicate column (re-migration crash).
3. **mod.rs:758** — PQ path loads all full-precision embeddings (2.4GB at 1.2M) just to encode them to codes. Either persist PQ codes in DB and load only codes, or revise design doc's memory claims and defer code persistence.

### Follow-up (future work)

4. **pq.rs:152** — `sort_unstable_by` + `truncate` is O(N log N). Use `select_nth_unstable_by` for true O(N) partial selection at 1.2M scale.
5. **pq.rs:351** — `proptest_adc_ranking_preserved` tolerance is too loose for large input ranges. Narrow `arb_embedding()` or restate tolerance.
6. **ui/mod.rs** — TUI similarity sort still uses brute-force L2 with 100K cap. Wire through PQ once codes are persisted.

### Confirmed correct

- Codebook serialization roundtrip, ADC distance computation, encoding determinism
- Property tests: non-negativity, valid indices, ranking preservation (structurally sound)
- Schema migration logic (version check, stamp, upgrade path)
- Brute-force fallback when no codebook present
- Design docs are clear and actionable

<!-- gh-id: 3091118907 -->
### Copilot on `src/engine/sqlite.rs:647` (2026-04-16 06:18 UTC)

`insert_embedding` serializes whatever slice length is provided, but downstream code (PQ encode / L2 distance) assumes exactly 512 floats. This can store malformed embeddings and later cause incorrect similarity results or panics. Enforce `vector.len() == similarity::EMBEDDING_DIM` (or 512) before writing, and return an error if the length is wrong.

<!-- gh-id: 4118671569 -->
### copilot-pull-request-reviewer[bot] — COMMENTED (2026-04-16 06:18 UTC)

## Pull request overview

This PR hardens the embeddings-backed similarity search and introduces a Product Quantization (PQ) implementation for accelerated nearest-neighbor search, alongside a SQLite schema migration (v1→v2) to add an `embedding` column and updated design/review documentation.

**Changes:**
- Add PQ codebook loading, PQ encoding, and ADC search (rayon-parallel) with CLI `--similar` preferring PQ when a codebook exists.
- Bump SQLite schema to v2 and add a v1→v2 migration for `samples.embedding`, plus migration tests and embedding helper APIs.
- Documentation and tooling updates: split embedding design docs, add internal review doc, pin Rust toolchain in CI, and apply widespread formatting/clippy-oriented refactors.

### Reviewed changes

Copilot reviewed 30 out of 31 changed files in this pull request and generated 3 comments.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| tests/integration.rs | Formatting-only changes to CLI arg assertions. |
| tests/edge_cases.rs | Formatting-only changes to edge case tests. |
| src/ui/widgets.rs | Refactors/formatting + small UI logic tweaks (label span threshold, status bar conditionals). |
| src/ui/search.rs | Refactors/formatting + conditional simplifications for peaks loading and tests. |
| src/ui/actions.rs | Refactors/formatting; minor string building tweak in `key_display`. |
| src/engine/workflow.rs | Moves/adds `write_metadata_changes` for surgical BEXT writes; refactors Lua/sqlite helpers and tests. |
| src/engine/wav.rs | Refactors/formatting + small test cleanups. |
| src/engine/sqlite.rs | Schema v2 + migration + embedding APIs (`insert_embedding`, `embedding_count`) + prepare_cached consistency + tests. |
| src/engine/source.rs | Hooks RIFF source metadata writing to `workflow::write_metadata_changes`; refactors tests. |
| src/engine/similarity.rs | Documentation hardening + small refactors + proptest tightening. |
| src/engine/riff_info.rs | Refactors/formatting + alignment checks using `is_multiple_of`. |
| src/engine/pq.rs | New PQ module: codebook, encoding, ADC table + rayon search + tests/proptests. |
| src/engine/playback.rs | Refactors/formatting; minor conditional reshaping. |
| src/engine/mod.rs | Adds `pq` module + PQ path in `--similar` + multiple small refactors and condition chaining. |
| src/engine/marks.rs | Refactors/formatting for parent dir creation and locking. |
| src/engine/id3.rs | Adds `merge_id3_into_unified` (moved to main module scope) + refactors/proptests formatting. |
| src/engine/filesystem.rs | Import ordering + formatting; small refactors in tests. |
| src/engine/config.rs | Large formatting refactor + more explicit tests + config parse simplification. |
| src/engine/cli.rs | Help builder formatting + test formatting. |
| src/engine/bext.rs | Formatting + change `to_bytes(self)` signatures for marker structs; small test refactors. |
| rust-toolchain.toml | Pin Rust toolchain to 1.94.0. |
| doc/reviews/review-2026-04-15-02.md | Add saved local review notes for the branch. |
| doc/plans/plan-2026-04-15.md | Add roadmap / plan doc for embedding hardening + PQ + MCP integration. |
| doc/designs/embedding-human.md | Add/restore human-facing embedding design doc. |
| doc/designs/embedding-agentic.md | Add agentic/MCP integration-oriented design doc. |
| benches/tui_render.rs | Allow missing docs in benches. |
| benches/sqlite_ops.rs | Allow missing docs + formatting refactors in benchmarks. |
| benches/metadata_read.rs | Allow missing docs + minor refactors in benchmarks. |
| Cargo.toml | Add clippy lint allowances to reduce noise. |
| .github/workflows/ci.yml | Pin toolchain in CI and enforce `-D warnings`; remove fmt continue-on-error. |
</details>



<details>
<summary>Comments suppressed due to low confidence (1)</summary>

**src/engine/sqlite.rs:700**
* `load_all_embeddings` accepts any BLOB length >= 4 and converts it to floats. With the new PQ path, vectors with length != 512 will panic in `pq.encode` (out-of-bounds slicing) and may also silently skew brute-force distances. Validate the blob length is exactly `EMBEDDING_DIM * 4` (2048 bytes) (or the decoded vec len is 512) and either skip invalid rows with a warning or return an error.
```
    pub fn load_all_embeddings(&self) -> anyhow::Result<Vec<(i64, std::path::PathBuf, Vec<f32>)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, path, embedding FROM samples WHERE embedding IS NOT NULL",
        )?;
        let rows = stmt.query_map([], |row| {
            let id: i64 = row.get(0)?;
            let path: String = row.get(1)?;
            let blob: Vec<u8> = row.get(2)?;
            Ok((id, path, blob))
        })?;

        let mut results = Vec::new();
        for row in rows {
            let (id, path, blob) = row?;
            if blob.len() >= 4 {
                let vec: Vec<f32> = blob
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect();
                results.push((id, std::path::PathBuf::from(path), vec));
            }
```
</details>



---

💡 <a href="/cmk/riffgrep/new/main?filename=.github/instructions/*.instructions.md" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Add Copilot custom instructions</a> for smarter, more guided reviews. <a href="https://docs.github.com/en/copilot/customizing-copilot/adding-repository-custom-instructions-for-github-copilot" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Learn how to get started</a>.

<!-- gh-id: 3091118928 -->
### Copilot on `src/engine/workflow.rs:468` (2026-04-16 06:18 UTC)

`write_metadata_changes` writes several fields into the packed-Description offsets (e.g., 44..112), but it never initializes the packed schema markers (file_id / version_major+minor / bext_version). `parse_bext_buffer` only reads these fields when the packed schema is detected, so writes to those offsets on an unpacked file won’t be reflected on subsequent reads and can also inject NULs into the plain-text Description. Consider either: (1) only writing standard BEXT fields when the file is unpacked, or (2) initializing the packed schema (similar to `init_packed_and_write_markers`) before writing packed fields, and gating packed writes on packed detection.

<!-- gh-id: 3091118958 -->
### Copilot on `src/engine/workflow.rs:426` (2026-04-16 06:18 UTC)

`write_ascii` copies raw UTF-8 bytes and truncates at an arbitrary byte boundary. For non-ASCII input this can split a multi-byte codepoint and write invalid UTF-8 into the BEXT chunk. Since these fields are specified as fixed-width ASCII in `BextFields`, validate `val.is_ascii()` (or sanitize/replace) and truncate on a character boundary (or explicitly document that non-ASCII will be lossy).

<!-- gh-id: 3091406075 -->
#### ↳ cmk (2026-04-16 07:20 UTC)

Fixed in 1e25d5d — insert_embedding now validates vector.len() == 512 before serializing. load_embedding and load_all_embeddings also tightened to exact 2048-byte blob check with warnings for malformed rows.

<!-- gh-id: 3091406196 -->
#### ↳ cmk (2026-04-16 07:20 UTC)

Fixed in 1e25d5d — packed Description writes (offsets 44..112) are now gated on `before.file_id != 0`, preventing corruption of the plain-text Description block on unpacked files.

<!-- gh-id: 3091406341 -->
#### ↳ cmk (2026-04-16 07:20 UTC)

Fixed in 1e25d5d — write_ascii now sanitizes non-ASCII characters to '?' before copying, preventing split multi-byte codepoints at field boundaries.

<!-- gh-id: 3091432335 -->
### Copilot on `src/engine/api.rs:137` (2026-04-16 07:25 UTC)

Same issue as the CLI PQ path: `max_dist` is derived from `scored.last()` but `results.truncate(limit)` may drop the farthest neighbor due to the prepended subject, skewing similarity scaling. Compute `max_dist` from the last neighbor actually returned (post-skip, post-limit) or adjust the PQ search limit/truncation so the farthest used for scaling is included in the output window.

<!-- gh-id: 3091432390 -->
### Copilot on `src/engine/sqlite.rs:686` (2026-04-16 07:25 UTC)

`embedding_count()` counts any non-NULL embedding BLOB, but `load_all_embeddings()`/`load_embedding()` explicitly skip malformed blobs whose length != DIM*4. This means the 100K guard can reject a DB even when the number of *valid* embeddings is under the cap. Consider counting only well-formed embeddings in SQL (e.g., `WHERE embedding IS NOT NULL AND length(embedding)=?`) to keep the guard consistent with what will actually be loaded.


<!-- gh-id: 3091432417 -->
### Copilot on `src/engine/api.rs:59` (2026-04-16 07:25 UTC)

`search()` runs `db.search()` / `finder.walk()` synchronously but uses a bounded channel and only drains `rx` after the producer finishes. Since both producers call `tx.send()` directly, this can deadlock once the channel buffer (2048) fills (e.g., >2048 matches). Consider spawning the producer on a thread and draining concurrently (dropping the receiver when `limit` is reached to stop early), or switch to an unbounded channel plus early truncation/stop logic.

<!-- gh-id: 4118997779 -->
### copilot-pull-request-reviewer[bot] — COMMENTED (2026-04-16 07:25 UTC)

## Pull request overview

This PR hardens the embedding-based similarity search pipeline and introduces Product Quantization (PQ) support for faster approximate similarity search, along with schema migration and new documentation/API surfaces to support MCP integration.

**Changes:**
- Added schema v2 migration and embedding IO hardening (dimension checks, malformed-row skipping, row-count guard).
- Introduced PQ codebook loading/encoding + ADC scan and wired `--similar` to use PQ when available with brute-force fallback.
- Added a new headless `engine::api` module plus expanded design/review documentation.

### Reviewed changes

Copilot reviewed 10 out of 11 changed files in this pull request and generated 5 comments.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| src/ui/mod.rs | Adds a 100K guard before brute-force similarity search in the TUI. |
| src/engine/workflow.rs | Sanitizes non-ASCII writes and gates packed-Description writes to avoid corruption. |
| src/engine/sqlite.rs | Bumps schema version, adds migration + tests, hardens embedding read/write, adds `embedding_count()`. |
| src/engine/similarity.rs | Clarifies similarity score semantics and updates squared-distance doc. |
| src/engine/pq.rs | New PQ implementation: codebook load/serialize, encode, ADC table + parallel scan, tests/proptests. |
| src/engine/mod.rs | Exposes `api`/`pq` modules and updates `--similar` to prefer PQ with brute-force cap fallback. |
| src/engine/api.rs | New programmatic API for search/similarity/metadata/indexing with unit tests. |
| doc/reviews/review-2026-04-15-02.md | Captures local review findings and status. |
| doc/plans/plan-2026-04-15.md | Adds roadmap for embedding hardening, API extraction, PQ, and stdio integration. |
| doc/designs/embedding-human.md | Human-facing design doc split out and updated to match current approach. |
| doc/designs/embedding-agentic.md | Agentic/MCP integration plan doc added/rewritten. |
</details>






---

💡 <a href="/cmk/riffgrep/new/main?filename=.github/instructions/*.instructions.md" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Add Copilot custom instructions</a> for smarter, more guided reviews. <a href="https://docs.github.com/en/copilot/customizing-copilot/adding-repository-custom-instructions-for-github-copilot" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Learn how to get started</a>.

<!-- gh-id: 3091432437 -->
### Copilot on `src/engine/api.rs:139` (2026-04-16 07:25 UTC)

The brute-force fallback loads all embeddings without any row-count guard. For large libraries (or when no PQ codebook is present), this can allocate multiple GB and likely OOM/hang. Mirror the CLI hardening by checking `db.embedding_count()` against a cap (e.g., 100K) before calling `load_all_embeddings()`, and return a clear error suggesting training/providing a PQ codebook.


<!-- gh-id: 3091432477 -->
### Copilot on `src/engine/mod.rs:809` (2026-04-16 07:25 UTC)

In the PQ path, `max_dist` is computed from `scored.last()`, but then `results.truncate(limit)` drops the farthest neighbor (because the subject is prepended). This makes the returned window’s max distance smaller than `max_dist`, so similarity scores won’t scale to 0.0 for the last returned item. Compute `max_dist` from the last *returned* neighbor (after skipping the query and applying the `limit-1` cutoff), or adjust the PQ search limit/truncation to match `search_similar` semantics.
