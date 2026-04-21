# PR #1 — Misc improvements + CLAP embedding prototype

<!-- gh-id: 3090235015 -->
### Copilot on [`src/engine/workflow.rs`](https://github.com/cmk/riffgrep/pull/1#discussion_r3090235015) (2026-04-16 01:11 UTC)

In `db:query_one`, `row.get(i).unwrap_or(None)` silently converts any type mismatch / conversion failure into `nil`, which can hide real SQL/data issues and make scripts behave incorrectly without an error. It would be safer to either (1) return an mlua runtime error on `row.get` failure, or (2) decode values into appropriate Lua types (string/integer/number/blob) instead of forcing `Option<String>` for every column.

<!-- gh-id: 3090235038 -->
### Copilot on [`src/engine/workflow.rs`](https://github.com/cmk/riffgrep/pull/1#discussion_r3090235038) (2026-04-16 01:11 UTC)

There are two consecutive, nearly identical comments above the parameter binding logic ("Bind the single parameter..."). Consider removing the duplicate to keep the block concise.
```suggestion

```

<!-- gh-id: 3090235051 -->
### Copilot on [`src/engine/sqlite.rs`](https://github.com/cmk/riffgrep/pull/1#discussion_r3090235051) (2026-04-16 01:11 UTC)

`migrate()` is now a no-op that only updates `PRAGMA user_version`, and `SCHEMA_VERSION` was reset to 1. For an existing on-disk DB created by earlier versions, `create_schema()` won’t add the new `embedding`/`metadata` columns/tables (because `CREATE TABLE IF NOT EXISTS` doesn’t alter schemas), so the app will mark the DB as "current" while still missing required schema elements, leading to runtime SQL errors. Either restore idempotent migrations (ALTER TABLE / CREATE TABLE for missing pieces) or detect old DBs and fail fast with an explicit "reindex required" error instead of silently updating `user_version`.

<!-- gh-id: 3090235058 -->
### Copilot on [`src/engine/sqlite.rs:637`](https://github.com/cmk/riffgrep/pull/1#discussion_r3090235058) (2026-04-16 01:11 UTC)

`load_embedding()` uses `query_row(...)` and propagates `rusqlite::Error::QueryReturnedNoRows` when the path isn’t present in `samples`, but the doc comment says it returns `None` when no embedding is available. Consider explicitly mapping `QueryReturnedNoRows` to `Ok(None)` (and optionally distinguishing "file not indexed" vs "indexed but embedding NULL") so callers like `--similar` can produce a clear user-facing message instead of a SQL error.

<!-- gh-id: 3090235062 -->
### Copilot on [`src/engine/mod.rs`](https://github.com/cmk/riffgrep/pull/1#discussion_r3090235062) (2026-04-16 01:11 UTC)

`--similar` resolves the query string via `query_path.to_string_lossy()`, but indexed paths are stored exactly as discovered during indexing (often absolute, based on the root passed to the walker). If the user passes a different representation (relative vs absolute, different case on case-insensitive FS, etc.), `load_embedding()` may miss and `query_id` may become -1, which makes the pinned subject result print with an empty/default path. Consider normalizing the query path the same way indexing does (e.g., canonicalize/absolutize consistently) and treat "path not in DB" as a dedicated error before calling `search_similar`.

<!-- gh-id: 3090235074 -->
### Copilot on [`src/ui/mod.rs`](https://github.com/cmk/riffgrep/pull/1#discussion_r3090235074) (2026-04-16 01:11 UTC)

`active_reps_all_zero` now uses `.unwrap_or(true)`, so when `active_bank_ref()` is `None` (e.g., preview/markers not loaded yet), `play_program()` immediately takes the sentinel path and plays the whole file, skipping `ensure_markers()` and segment-program playback. Previously the comment explicitly relied on `None` falling through to marker initialization. If `None` can mean "markers not loaded" (not "no markers exist"), this changes behavior incorrectly; consider distinguishing these cases (e.g., check for format-without-markers separately, or treat `None` as `false` and let `ensure_markers()` initialize defaults).

<!-- gh-id: 4117756858 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-16 01:11 UTC](https://github.com/cmk/riffgrep/pull/1#pullrequestreview-4117756858))

## Pull request overview

This PR expands riffgrep’s prototype embedding workflow and multi-format audio support, adding similarity search (CLI + TUI) plus a format-dispatch refactor via an `AudioSource` registry, alongside several playback/workflow improvements and supporting docs/scripts/tests.

**Changes:**
- Add embedding storage + brute-force similarity search (`--similar`, TUI “sim” column + sort action).
- Introduce `AudioSource`/`AudioRegistry` to dispatch metadata/peaks/PCM/audio-info across WAV and decoder-backed formats (AIFF/MP3/FLAC/OGG).
- Add a Lua workflow engine with optional SQLite access, and expand playback features (reverse toggle) + test hardening.

### Reviewed changes

Copilot reviewed 32 out of 32 changed files in this pull request and generated 7 comments.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| tests/ui/main.rs | Placeholder UI integration test crate entry. |
| tests/integration.rs | Makes tests deterministic by defaulting to `--no-db` helper; adds raw helper for DB-specific tests. |
| tests/engine/workflow.rs | Placeholder for workflow engine integration tests. |
| tests/engine/main.rs | Placeholder for engine integration tests. |
| tests/edge_cases.rs | Forces filesystem mode in edge-case tests via `--no-db`. |
| test_files/riff+defaults-info_reaper-sm.wav | Adds a small WAV fixture via Git LFS pointer. |
| src/ui/widgets.rs | Adds `sim` column rendering and updates test rows for new `TableRow.sim`. |
| src/ui/search.rs | Routes audio-info loading through `AudioRegistry`; updates row construction to include `sim`. |
| src/ui/mod.rs | Adds similarity sort action, DB path plumbing, reverse playback toggle, zoom center logic tweaks, and marker/program sentinel behavior change. |
| src/ui/actions.rs | Adds `SortBySimilarity` action and updates action tables/tests. |
| src/engine/workflow.rs | New Lua workflow engine with `sample` userdata + a minimal SQLite Lua module; supports diffing and writing metadata back for writable formats. |
| src/engine/wav.rs | Adds decoder-based peaks/PCM helpers and registry dispatch; introduces configurable peak computation scaffolding. |
| src/engine/sqlite.rs | Adds embedding + metadata tables and embedding CRUD helpers; alters schema versioning/migration behavior. |
| src/engine/source.rs | New `AudioSource` abstraction + registry; adds WAV fast-path and decoder fallback implementations. |
| src/engine/similarity.rs | New brute-force L2 similarity search module with unit + property tests. |
| src/engine/playback.rs | Adds atomic reverse flag support in `SegmentSource`; improves lock poisoning messages. |
| src/engine/mod.rs | Dispatches to `--similar`; routes metadata/peaks reading and workflow writes through `AudioRegistry`; uses registry-derived file globs for indexing. |
| src/engine/marks.rs | Improves lock poisoning diagnostics; annotates trait method usage. |
| src/engine/id3.rs | Adds ID3v2 reading via lofty + merge helper to fill `UnifiedMetadata` fields. |
| src/engine/filesystem.rs | Builds walker file-type globs from `AudioRegistry`; updates doc comment to “audio files”. |
| src/engine/config.rs | Adds `sim` to available column definitions; annotates reserved config plumbing. |
| src/engine/cli.rs | Adds `--similar` option; updates help examples and parsing tests. |
| src/engine/bext.rs | Comment/doc formatting tweaks and a few `#[allow(dead_code)]` annotations. |
| scripts/inject_synthetic_embeddings.py | Utility to inject synthetic normalized embeddings into the DB for prototyping. |
| scripts/etl_soundminer_ref.lua | Reference ETL Lua script using the workflow + sqlite module to port metadata from SoundMiner REF.sqlite. |
| scripts/enrich_from_ref.sql | SQL script to enrich riffgrep’s DB from SoundMiner REF.sqlite via attach/join/update. |
| doc/plans/plan-2026-03-27.md | Embedding phase plan and testing strategy. |
| doc/plans/plan-2026-03-26.md | AudioSource refactor plan and test/property checklist. |
| doc/designs/EMBEDDING.md | Full embedding design doc (storage, PQ, CLI/TUI UX, scaling plan). |
| doc/TODO.md | Reverse playback unification TODO / risks / missing tests. |
| Cargo.toml | Enables additional symphonia decode features (aiff/mp3/flac/ogg). |
| .gitattributes | Adds explicit LFS rule entry for the new WAV fixture (in addition to `*.wav`). |
</details>






---

💡 <a href="https://github.com/cmk/riffgrep/new/main?filename=.github/instructions/*.instructions.md" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Add Copilot custom instructions</a> for smarter, more guided reviews. <a href="https://docs.github.com/en/copilot/customizing-copilot/adding-repository-custom-instructions-for-github-copilot" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Learn how to get started</a>.

<!-- gh-id: 3090235077 -->
### Copilot on [`src/ui/mod.rs:1535`](https://github.com/cmk/riffgrep/pull/1#discussion_r3090235077) (2026-04-16 01:11 UTC)

`sort_by_similarity()` claims to pin the selected file to position 0, but the implementation only assigns `sim` scores and sorts by `sim` descending. This doesn’t guarantee the selected row is first (ties at `sim=1.0`, or the selected path not found causing the subject result to have a default/empty path and thus not match any row). Consider explicitly moving the selected row to index 0 after scoring/sorting (or sorting with a primary key that forces `row.meta.path == selected_path` first).

<!-- gh-id: 3090612369 -->
#### ↳ cmk ([2026-04-16 03:19 UTC](https://github.com/cmk/riffgrep/pull/1#discussion_r3090612369))

Fixed in 25d8b11 — row values are now decoded to proper Lua types (string, integer, float) with a cascade fallback to Nil, instead of forcing everything through Option<String>.

<!-- gh-id: 3090612687 -->
#### ↳ cmk ([2026-04-16 03:20 UTC](https://github.com/cmk/riffgrep/pull/1#discussion_r3090612687))

Fixed in 25d8b11 — removed the duplicate comment.

<!-- gh-id: 3090612769 -->
#### ↳ cmk ([2026-04-16 03:20 UTC](https://github.com/cmk/riffgrep/pull/1#discussion_r3090612769))

Fixed in 25d8b11 — `migrate()` now reads `PRAGMA user_version` and fails fast with a descriptive error if it doesn't match `SCHEMA_VERSION`. Fresh databases (version 0) get stamped. This also resolved 6 pre-existing test failures caused by schema mismatch.

<!-- gh-id: 3090612852 -->
#### ↳ cmk ([2026-04-16 03:20 UTC](https://github.com/cmk/riffgrep/pull/1#discussion_r3090612852))

Fixed in 25d8b11 — `load_embedding()` now explicitly catches `QueryReturnedNoRows` and returns `Ok(None)`, distinguishing "path not in DB" from actual query errors.

<!-- gh-id: 3090612921 -->
#### ↳ cmk ([2026-04-16 03:20 UTC](https://github.com/cmk/riffgrep/pull/1#discussion_r3090612921))

Fixed in 25d8b11 — the query path is now canonicalized before lookup so it matches how paths were stored during indexing.

<!-- gh-id: 3090613013 -->
#### ↳ cmk ([2026-04-16 03:20 UTC](https://github.com/cmk/riffgrep/pull/1#discussion_r3090613013))

Fixed in 25d8b11 — renamed to `no_program_markers` with a clarifying comment. The `None` case (no markers loaded) intentionally falls through to whole-file playback rather than attempting segment-program with missing data.

<!-- gh-id: 3090613061 -->
#### ↳ cmk ([2026-04-16 03:20 UTC](https://github.com/cmk/riffgrep/pull/1#discussion_r3090613061))

Fixed in 25d8b11 — the sort comparator now checks if either row is the query file and pins it to position 0 before falling through to the sim-score comparison.
