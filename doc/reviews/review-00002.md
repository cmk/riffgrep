# PR #2 — Add dev patterns, recover old files, restore ID3 fields

## Local review (2026-04-15)

**Branch:** devops
**Commits:** 3 (a80a79f..devops)
**Reviewer:** Claude (sonnet, independent)

---

### Commit Hygiene

All three commit messages use valid conventional prefixes (`fix:`, `task:`, `task:`), are under 72 characters, and use present-tense imperative subject lines. Commits appear atomic — the ID3 logic changes are isolated in commit 1, recovered files in commit 2, and dev tooling in commit 3. No merge commits detected. Acceptable.

---

### Code Quality

**`src/engine/id3.rs` — `rating` field is a permanent stub**

The `Id3Tags.rating` field is unconditionally assigned `String::new()` at construction. The struct doc comment says "POPM — Rating (as string)", but POPM is a binary frame — it carries an email string, a rating byte, and a play counter, none of which are accessible via `tag.get_string(&ItemKey::…)`. Lofty does not expose a string key for POPM; `ItemKey` has no `Popularimeter` variant in the get_string API. The field will always be empty regardless of what's in the file. The merge logic consequently dead-codes itself — the `!id3.rating.is_empty()` branch can never be taken.

This is distinct from `UnifiedMetadata.rating`, which is populated from the packed BEXT Description block and works correctly. The ID3 path for rating is silent dead code that implies working functionality it does not have.

**No other code quality issues.** The BPM fallback (`ItemKey::Bpm` then `ItemKey::IntegerBpm`), the AlbumArtist-to-AlbumTitle library fallback, and the `ContentGroup`-to-`TrackSubtitle` mapping for `usage_id` are all correct and consistent with the field table in `UnifiedMetadata`. The merge pattern (`is_empty() && !is_empty()`) is consistent with every pre-existing field.

---

### Test Coverage

**Missing property tests for `id3.rs` — mandatory per CLAUDE.md**

`id3.rs` contains a parse function (`parse_tag`) and a data transform function (`merge_id3_into_unified`). Per the project rule: "Property-based testing is mandatory for any module that parses, encodes, or transforms data." There are zero proptest tests in `id3.rs`. The existing `#[cfg(test)]` block has only unit tests and fixture-dependent integration tests.

The minimum required property would be: for any `Id3Tags`, `merge_id3_into_unified` must not overwrite a non-empty `UnifiedMetadata` field (the no-overwrite invariant).

**Fixture-dependent tests correctly return early.** Tests guard on `test_files_exist()` and return rather than panicking or using `#[ignore]`. Correct per convention.

**`merge_fills_empty_fields` does not cover the 5 new fields.** The test asserts on `vendor`, `category`, `bpm`, and `key`, but not on `date`, `subcategory`, `genre_id`, `track`, or `rating`.

---

### Risks

**`claude.yml` has `contents: write` permission — overly broad for a comment-trigger workflow.** Grants `contents: write` for a workflow triggered on issue comments and PR review comments — untrusted input surfaces. A `contents: write` token can push branches, create tags, and modify files. Should be scoped to `contents: read` unless the action specifically needs write access.

**CI only runs on `main` and PRs targeting `main`.** Feature branches are not validated by CI until a PR is opened.

**`claude-code-review.yml`** references a plugin marketplace URL and is disabled (`if: false`). Should be validated before re-enabling.

---

### Recommendations

**Must fix before push:**

1. **`src/engine/id3.rs` — `rating: String::new()` is permanently dead.** Either implement actual POPM parsing or remove the field from `Id3Tags` and the dead merge arm. Leaving a struct field claiming POPM parsing while always returning empty is misleading.

2. **`src/engine/id3.rs` — Add proptest for `merge_id3_into_unified`.** Minimum viable property: given arbitrary `Id3Tags` and a `UnifiedMetadata` with pre-populated fields, merge must never overwrite a field that was non-empty before the call.

3. **`.github/workflows/claude.yml` — Reduce `contents: write` to `contents: read`.** The `@claude` mention handler does not need write access to respond to comments.

**Follow-up (future work):**

- Extend `merge_fills_empty_fields` unit test to cover the 5 new fields.
- CI triggers: add feature branches to the branch filter in `ci.yml` or adopt `push: branches: ['**']`.
- `claude-code-review.yml`: validate plugin marketplace URL and token scoping before re-enabling.

<!-- gh-id: 3090628231 -->
### Copilot on [`.github/workflows/claude.yml:26`](https://github.com/cmk/riffgrep/pull/2#discussion_r3090628231) (2026-04-16 03:26 UTC)

This workflow grants `contents: write` to a job triggered by issue/pr comment content. That permission allows pushing commits/tags and modifying the repository, which is broader than needed for a comment-response automation and increases the impact of any action bug or prompt-injection. Consider scoping to `contents: read` (and only adding narrower permissions that are strictly required).

<!-- gh-id: 4118186925 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-16 03:26 UTC](https://github.com/cmk/riffgrep/pull/2#pullrequestreview-4118186925))

## Pull request overview

This PR expands riffgrep’s developer workflow and feature set by adding a Lua-based workflow engine, embedding similarity search (DB + CLI + TUI hooks), and a format-agnostic audio source dispatch layer, along with recovered historical fixtures/docs and CI/dev tooling.

**Changes:**
- Add embedding storage + brute-force similarity search, exposed via `--similar` and a TUI `sim` column/sort.
- Introduce `AudioSource`/`AudioRegistry` to dispatch metadata/peaks/PCM/audio-info across WAV (RIFF fast path) and decoder-backed formats.
- Add Lua workflow engine scaffolding (mlua + optional SQLite access) plus dev tooling/docs/fixtures.

### Reviewed changes

Copilot reviewed 13 out of 14 changed files in this pull request and generated 1 comment.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| `tests/ui/main.rs` | Adds placeholder for future UI integration tests. |
| `tests/proptest-regressions/prop.txt` | Checks in proptest regression seeds for deterministic replays. |
| `tests/integration.rs` | Forces filesystem mode in most integration tests (`--no-db`) and introduces a raw helper for DB-specific tests. |
| `tests/engine/workflow.rs` | Adds placeholder for future workflow engine integration tests. |
| `tests/engine/main.rs` | Adds placeholder for future engine integration tests. |
| `tests/edge_cases.rs` | Forces filesystem mode in edge-case tests (`--no-db`). |
| `test_files/riff+defaults-info_reaper-sm.wav` | Adds new WAV fixture via Git LFS pointer. |
| `test_files/packed_markers.wav` | Adds packed markers WAV fixture via Git LFS pointer. |
| `src/ui/widgets.rs` | Adds `sim` column rendering and updates TableRow construction in tests for new field. |
| `src/ui/search.rs` | Uses `AudioRegistry` for audio-info loading; adds dead_code allowance comment. |
| `src/ui/mod.rs` | Adds similarity sorting, reverse playback toggle wiring, zoom-center logic changes, and stores `db_path` on the app. |
| `src/ui/actions.rs` | Adds `SortBySimilarity` action plus docs/metadata wiring and updates action counts. |
| `src/engine/workflow.rs` | Introduces Lua workflow engine scaffolding, Lua-exposed metadata setters/getters, and BEXT write-back routine. |
| `src/engine/wav.rs` | Adds decoder-based peaks/PCM helpers and routes some operations through `AudioRegistry`. |
| `src/engine/sqlite.rs` | Adds embedding + metadata table support, adjusts schema versioning/migration approach, and wires `sim` into TableRow. |
| `src/engine/source.rs` | Adds `AudioSource` trait + `AudioRegistry` with RIFF and decoder-backed implementations. |
| `src/engine/similarity.rs` | Adds brute-force L2 similarity search utilities + tests/proptests. |
| `src/engine/playback.rs` | Adds atomic reverse flag in shared control, reverse traversal logic, and improves poisoned-lock messages. |
| `src/engine/mod.rs` | Exposes new modules, adds `TableRow.sim`, routes metadata/peaks through registry, and adds `--similar` execution path. |
| `src/engine/marks.rs` | Uses `expect()` for poisoned lock handling; annotates trait method as dead_code. |
| `src/engine/id3.rs` | Adds ID3v2 parsing via lofty, restores additional fields, and adds unit/proptest coverage for merge logic. |
| `src/engine/filesystem.rs` | Builds ignore walker file types from registry-supported extensions (instead of WAV-only). |
| `src/engine/config.rs` | Adds `sim` to column configuration. |
| `src/engine/cli.rs` | Adds `--similar` flag parsing and tests. |
| `src/engine/bext.rs` | Updates docs/annotations around packed schema/markers and some comment formatting. |
| `scripts/inject_synthetic_embeddings.py` | Adds helper to inject synthetic embeddings into an index DB for manual testing. |
| `scripts/etl_soundminer_ref.lua` | Adds reference SoundMiner ETL Lua script using the new Lua+SQLite plumbing. |
| `scripts/enrich_from_ref.sql` | Adds SQL enrichment script to pull fields from SoundMiner REF DB into riffgrep DB. |
| `rust-toolchain.toml` | Pins stable toolchain with clippy/rustfmt components for consistent dev/CI. |
| `doc/todo/WORKFLOWS.md` | Adds workflow DSL notes/design discussion document. |
| `doc/reviews/review-2026-04-15-01.md` | Captures Tier 1 review artifact referenced in PR description. |
| `doc/plans/plan-2026-03-27.md` | Adds embedding phase plan documentation. |
| `doc/plans/plan-2026-03-26.md` | Adds AudioSource refactor plan documentation. |
| `doc/misc/SOUNDMINER_SCHEMA_ANALYSIS.md` | Adds SoundMiner schema analysis documentation. |
| `doc/designs/EMBEDDING.md` | Adds embedding architecture/design document. |
| `doc/TODO.md` | Adds reverse playback unification TODO/design notes. |
| `bin/make_packed_test_wav.py` | Adds fixture generator for packed-schema marker WAVs. |
| `Cargo.toml` | Expands `symphonia` features to include additional decoders (aiff/mp3/flac/ogg). |
| `CLAUDE.md` | Adds repo conventions, workflow, and review/testing expectations. |
| `.github/workflows/claude.yml` | Adds Claude Code workflow triggered by comments/reviews/issues. |
| `.github/workflows/claude-code-review.yml` | Adds (currently disabled) Claude Code Review workflow definition. |
| `.github/workflows/ci.yml` | Adds CI pipeline (build/clippy/test/fmt) for main/PRs targeting main. |
| `.gitattributes` | Ensures WAVs are handled via Git LFS (plus a specific fixture path). |
| `.claude/skills/sprint-review.md` | Adds local Tier 1 review “skill” documentation/procedure. |
| `.claude/settings.json` | Adds pre-commit hook definition (cargo test + clippy) via Claude tooling. |
</details>






---

💡 <a href="https://github.com/cmk/riffgrep/new/main?filename=.github/instructions/*.instructions.md" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Add Copilot custom instructions</a> for smarter, more guided reviews. <a href="https://docs.github.com/en/copilot/customizing-copilot/adding-repository-custom-instructions-for-github-copilot" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Learn how to get started</a>.
