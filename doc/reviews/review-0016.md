# PR #16 — Auto-activate BEXT packed schema in workflow writer

## Local review (2026-04-18)

**Branch:** sprint/workflow-bext-activation
**Commits:** 2 (main..sprint/workflow-bext-activation) — at review time
**Reviewer:** Claude (sonnet, independent)

> **Status when pushed:** Must-fix items #1 (proptests) and #2 (data-loss
> docstring) were addressed in commit `51c7680` before push. Follow-up #4
> (preserve non-packed regions test) was also included. The original
> text below is preserved as the audit-trail snapshot; statements about
> "zero proptests" / "missing preservation tests" / "must fix before
> push" reflect the pre-commit-`51c7680` state and should be read as
> historical, not current.

---

### Commit Hygiene

Both commits are clean. `5864d5d` is a focused refactor (write-order only, no logic change). `a931efb` is a focused fix with its tests in the same commit. Conventional commit prefixes (`refactor:`, `fix:`) match repo style. Bodies are detailed and call out the ETL contract. Both commits are buildable and testable independently. No issues here.

---

### Code Quality

**BPM alignment fix — correct direction, but analogous fields may share the same bug.**

Confidence: 85

The reader (`parse_bpm_ascii`, `src/engine/bext.rs` line 394) calls `s.parse::<u16>().ok()` where `s = decode_fixed_ascii(bytes)`. The existing test fixture at line 912 writes `"164 "` (digit-then-space), and the test at line 953 asserts `bpm == Some(164)`, which passes — so the reader does handle trailing whitespace correctly via `str::parse`. The old writer used `{v:>4}` (right-align = `" 128"`), which would make `" 128".parse::<u16>()` return `Err` because Rust's `str::parse` does not trim leading whitespace. The fix to `{v:<4}` (left-align = `"128 "`) is correct.

However, no analogous field uses numeric formatting — `rating`, `subcategory`, `category`, `genre_id`, `sound_id`, `usage_id`, and `key` are all string fields written with `write_ascii` and read with `decode_fixed_ascii`, which trims trailing nulls/spaces. The BPM field is the only field that round-trips through a numeric parser; the bug and fix are isolated.

**`packed_diffs` expression structurally drifts from the write block.**

Confidence: 85

`src/engine/workflow.rs` lines 420–428 enumerate nine fields for `packed_diffs`. The write block (lines 471–500) also enumerates nine fields. The comment says "keep in sync." This is a real latent risk: adding a tenth packed field requires two edits in two places with no compiler enforcement. If `packed_diffs` is missing a new field, an unpacked file that only changed that field will skip activation and silently drop the write. The "keep in sync" comment is insufficient given that the consequence of drift is silent data loss. A structural fix (e.g., computing `packed_diffs` by comparing serialised packed blocks, or extracting a `packed_fields_differ(before, after) -> bool` helper that the write block calls into via the same closure chain) would eliminate the class. This is not a bug today but is a maintenance trap.

**`force` flag interaction with auto-activation.**

The bail at workflow.rs line 409 fires before the `packed_diffs` check:

```
if !force && before.file_id != 0 { bail!(...) }
```

So `--force` on an already-packed file bypasses the early exit and reaches the `if before.file_id == 0 && packed_diffs` guard — which is false for a packed file (`file_id != 0`). Auto-activation is skipped correctly; the packed writes happen directly. This is correct behavior. No issue.

**Re-scan after activation.**

`init_packed_and_write_markers` performs in-place surgical writes; the chunk map offsets are byte-stable. The re-scan is dead weight today but genuinely cheap and harmless. The comment accurately acknowledges this. Acceptable.

**Description[12:44] clobber on unpacked files with existing plain-text content.**

Confidence: 88

`MarkerConfig::default()` produces an empty config where `MarkerBank::empty()` fills each marker slot with `MARKER_EMPTY` (`u32::MAX = 0xFFFF_FFFF`), then `to_bytes()` writes 32 bytes of `0xFF`-heavy sentinel values to Description[12:44]. If the file had user-supplied plain-text content at those offsets (e.g., part of a 256-char description string), that content is silently destroyed by the activation step. The commit message does not mention this risk. There is no warning in the docstring or the call site. For a tool that targets a 1.2M-file library of real samples, this is a data-safety concern that should be documented at minimum. The docstring for `write_metadata_changes` says it "only writes fields that actually changed" — which is true for the standard fields, but activation unconditionally overwrites Description[0:44] regardless of what was there.

**Security — path traversal.**

No new risk. `path` comes from `UnifiedMetadata.path` which is ultimately derived from filesystem walk or SQLite index entries, not from unsanitised user string input at this layer. Lua scripts receive a pre-resolved path from the host. No change from previous surface area.

**Dead code / clippy.**

No obvious dead code in the diff. The `build_riff` / `temp_unpacked_wav` / `temp_wav_no_bext` helpers in the test module shadow `make_riff` and related helpers already in `bext.rs` tests. Duplication is acceptable within test modules.

---

### Test Coverage

**Property tests are absent from `workflow.rs` — a convention violation.**

Confidence: 92

`write_metadata_changes` transforms data (BPM formatting, ASCII truncation, field selection). CLAUDE.md states: "Property-based testing is mandatory for any module that parses, encodes, or transforms data." The new test block in `workflow.rs` contains four unit tests and zero proptests. The BPM left-align fix in particular has a narrow correctness envelope (`{v:<4}` with `v` up to `9999`) that a proptest would trivially cover with arbitrary `u16` inputs. This is a direct convention violation, not a style preference.

**`test_init_packed_idempotent_after_partial_prior_state` is a single-example test for what should be a proptest.**

Confidence: 82

The partial-state scenario covers one specific combination: version=1, minor=2, markers=preset_shot, bext_version=2, file_id=0. The invariant is "for any partial state where file_id=0, re-running activation produces a fully-packed file." This is exactly the shape of a proptest: enumerate arbitrary marker content, arbitrary version bytes at [8:12], arbitrary bext_version — and confirm repair always succeeds. The test as written only validates the single most likely failure mode. A prop exercising random bytes at [0:8] (confirming anything with file_id=0 recovers) would be more robust.

**`write_activates_packed_schema_on_unpacked_wav` does not verify non-packed bytes are preserved.**

Confidence: 85

The test sets `category`, `key`, and `bpm` and reads them back. It does not verify that bytes outside Description[0:44] — specifically Originator (256–287), OriginatorReference (288–319), and UMID (348–411) — are unchanged after activation. The existing `test_init_packed_preserves_originator` in `bext.rs` covers this for `init_packed_and_write_markers` alone, but the workflow-level test does not cover the combined activation + field-write path. If a future change to `write_metadata_changes` accidentally clobbers those regions, the test would not catch it.

**Temp-file name uniqueness under parallel `cargo test`.**

`temp_unpacked_wav` and `temp_wav_no_bext` name files as `riffgrep_wf_{suffix}_{pid}_{nanos}`. PID is constant within a test binary run; nanos provides sub-microsecond separation. Two test functions calling the same helper (e.g., a hypothetical second caller of `temp_unpacked_wav("activate")`) would collide if executed within the same nanosecond. Currently only one caller per suffix exists, so there is no immediate collision. This is a low-severity fragility, not a current bug.

**Panic on cleanup failure.**

`std::fs::remove_file(&path).unwrap()` in all four new tests will panic and leave the file behind if the assertion before it fails — the test already failed, but a stray temp file is not harmful. Consistent with the existing test style in `bext.rs`. Not a bug.

**`write_no_bext_chunk_errors_and_preserves_bytes` byte-preservation assertion.**

Confidence: 92

This test correctly checks that the file bytes are unchanged after a refused write. However, the call to `init_packed_and_write_markers` inside `write_metadata_changes` will fail with `NoBextChunk` before any write occurs — so byte preservation is trivially guaranteed by the error path, not by any explicit rollback. The test is still valuable as a regression guard, but it does not exercise a case where activation partially succeeds before the error. That scenario (partial write then error on the BEXT fields) is not covered by any test.

---

### Risks

**Half-activation on no-BEXT files via the new activation path — false safety.**

Not a bug today, but worth noting: `write_no_bext_chunk_errors_and_preserves_bytes` passes because `init_packed_and_write_markers` fails at `NoBextChunk` before touching the file. The byte-preservation guarantee depends on `init_packed_and_write_markers` failing atomically. This is true because `write_bext_field` checks `bext_offset.ok_or(NoBextChunk)` as its first operation before opening for write. If that ever changes, the test would still pass (error returned) but the file could be partially written. Currently safe.

**Observable byte-order after successful `init_packed_and_write_markers`.**

The reorder (UUID last instead of first) does not change the bytes written on the success path — the same four writes occur, just in a different order. The final file state is identical. No regression on the success path.

---

### Recommendations

**Must fix before push:**

1. `src/engine/workflow.rs` — Add at minimum one proptest to the new `write_metadata_changes` test block. The BPM round-trip (`arbitrary u16 → write → re-read → same value`) and the ASCII truncation path (`arbitrary string → write → re-read → first N chars`) are the highest-value targets. Required by CLAUDE.md conventions.

2. `src/engine/workflow.rs` and `bext.rs` docstrings — Document that auto-activation via `write_metadata_changes` unconditionally overwrites Description[12:44] with `MarkerConfig::default()` (sentinel `0xFF` bytes), destroying any pre-existing plain-text content in that region. This is a data-safety behavior change that callers of the public API need to know about.

**Follow-up (future work):**

3. Extract a `packed_fields_differ(before: &UnifiedMetadata, after: &UnifiedMetadata) -> bool` helper and call it from both the activation guard and the write gate, eliminating the structural drift risk between `packed_diffs` and the write block.

4. Extend `write_activates_packed_schema_on_unpacked_wav` to assert that Originator/OriginatorReference/UMID bytes at fixed offsets outside Description[0:44] are unchanged after the combined activation + write path.

5. Promote `test_init_packed_idempotent_after_partial_prior_state` to a proptest that exercises arbitrary byte values at Description[0:44] with file_id forced to 0, confirming re-activation always succeeds regardless of what garbage was written there by a prior partial run.

<!-- gh-id: 3105033514 -->
### Copilot on [`doc/reviews/review-0016.md:69`](https://github.com/cmk/riffgrep/pull/16#discussion_r3105033514) (2026-04-18 10:57 UTC)

The statement that `workflow.rs` has “zero proptests” is now incorrect (the file includes a new `mod proptests` block with multiple properties). Please update this section to reflect the current test coverage so the review artifact doesn’t contradict the PR contents.


<!-- gh-id: 3105033519 -->
### Copilot on [`doc/reviews/review-0016.md:81`](https://github.com/cmk/riffgrep/pull/16#discussion_r3105033519) (2026-04-18 10:57 UTC)

This claim about missing preservation assertions is now stale: `workflow.rs` includes `write_preserves_non_packed_regions_during_activation`, which asserts Originator/OriginatorReference/OriginationDate/UMID bytes are unchanged across activation+write. Please update/remove this paragraph to match the current tests.


<!-- gh-id: 3105033526 -->
### Copilot on [`doc/reviews/review-0016.md:118`](https://github.com/cmk/riffgrep/pull/16#discussion_r3105033526) (2026-04-18 10:57 UTC)

The “Must fix before push” recommendations are out of date with the PR as written (proptests and activation data-loss documentation were added in code/docstrings). Please revise this section to reflect what remains outstanding vs what is already addressed, so the review record stays accurate.


<!-- gh-id: 4134216795 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-18 10:57 UTC](https://github.com/cmk/riffgrep/pull/16#pullrequestreview-4134216795))

## Pull request overview

Enables the Lua workflow metadata writer to automatically activate the BEXT packed schema on previously-unpacked WAVs when packed-field diffs are present, preventing silent drops on `--commit` and improving safety/consistency around packed-field writes.

**Changes:**
- Auto-activate packed schema in `write_metadata_changes` when unpacked + packed-field diffs, and share ASCII sanitize/truncate behavior with Lua setters.
- Reorder `init_packed_and_write_markers` so `file_id` (UUID) is written last for recoverable/idempotent partial failures; add `PACKED_COMMENT_LEN`.
- Update SoundMiner ETL to preserve pre-existing plain-text BEXT Description into packed `comment`, and expand tests/proptests for round-trips and activation behavior.

### Reviewed changes

Copilot reviewed 4 out of 4 changed files in this pull request and generated 6 comments.

| File | Description |
| ---- | ----------- |
| src/engine/workflow.rs | Adds packed-schema auto-activation, shared ASCII sanitization, BPM formatting fix, and extensive tests/proptests for activation + round-trip. |
| src/engine/bext.rs | Introduces `PACKED_COMMENT_LEN` and reorders packed-schema init to write `file_id` last; adds an idempotence test for partial prior state. |
| scripts/etl_soundminer.lua | Rescues plain-text BEXT Description into packed comment before activation; clarifies UMID stamping behavior. |
| doc/reviews/review-0016.md | Adds a local review artifact (currently contains some stale/incorrect statements that need updating). |





<!-- gh-id: 3105033530 -->
### Copilot on [`src/engine/workflow.rs:467`](https://github.com/cmk/riffgrep/pull/16#discussion_r3105033530) (2026-04-18 10:57 UTC)

`packed_diffs` duplicates the packed-field list that’s immediately repeated in the write block below. This is easy to let drift when new packed fields are added, and drift would reintroduce silent drops (activation skipped but write expected). Consider extracting a single helper (e.g., `packed_fields_differ(before, after)`) or iterating over a shared list so the activation guard and write gate cannot diverge.


<!-- gh-id: 3105033533 -->
### Copilot on [`src/engine/workflow.rs:1172`](https://github.com/cmk/riffgrep/pull/16#discussion_r3105033533) (2026-04-18 10:57 UTC)

The proptest doc comment says “Any u16 BPM round-trips…”, but the strategy is `0u16..10_000` (i.e., 0..=9999). Please either widen the strategy to cover the full `u16` domain (and assert the expected truncation behavior), or adjust the comment to match the tested range.


<!-- gh-id: 3105033535 -->
### Copilot on [`doc/reviews/review-0016.md:25`](https://github.com/cmk/riffgrep/pull/16#discussion_r3105033535) (2026-04-18 10:57 UTC)

This review doc embeds absolute local filesystem paths (e.g., `/Users/.../src/engine/bext.rs`). Please change these to repo-relative paths (and avoid hard-coded line numbers where possible) so the document remains portable and doesn’t leak local environment details.

<!-- gh-id: 3105040651 -->
#### ↳ cmk ([2026-04-18 11:03 UTC](https://github.com/cmk/riffgrep/pull/16#discussion_r3105040651))

Addressed in 171ae50 — added a status banner at the top of the local-review section marking the must-fix items as resolved pre-push by commit 51c7680, so the snapshot doesn't read as current state.

<!-- gh-id: 3105040792 -->
#### ↳ cmk ([2026-04-18 11:03 UTC](https://github.com/cmk/riffgrep/pull/16#discussion_r3105040792))

Addressed in 171ae50 — same status banner at the top of the local-review section notes that the preservation test (write_preserves_non_packed_regions_during_activation) was added in 51c7680; the paragraph is preserved as the snapshot.

<!-- gh-id: 3105040962 -->
#### ↳ cmk ([2026-04-18 11:03 UTC](https://github.com/cmk/riffgrep/pull/16#discussion_r3105040962))

Addressed in 171ae50 — the status banner at the top of the local-review section names which must-fix items were resolved (by commit 51c7680) vs what's deferred. The original recommendations are kept verbatim as the audit-trail snapshot.

<!-- gh-id: 3105041178 -->
#### ↳ cmk ([2026-04-18 11:03 UTC](https://github.com/cmk/riffgrep/pull/16#discussion_r3105041178))

Fixed in 171ae50 — extracted `packed_fields_differ(before, after)` as the single source of truth; the activation guard and the packed-write gate both call through it, so drift between 'trigger activation' and 'perform the write' is structurally impossible now.

<!-- gh-id: 3105041373 -->
#### ↳ cmk ([2026-04-18 11:04 UTC](https://github.com/cmk/riffgrep/pull/16#discussion_r3105041373))

Fixed the docstring in 171ae50 rather than widening the strategy. The 0..10_000 range is correct: BPM >= 10000 renders as 5 chars and would be truncated to 4 by write_ascii (e.g., 10000 -> "1000" -> 1000 on read), so the full u16 domain is not a round-trip envelope. The docstring now documents that limit explicitly.

<!-- gh-id: 3105041630 -->
#### ↳ cmk ([2026-04-18 11:04 UTC](https://github.com/cmk/riffgrep/pull/16#discussion_r3105041630))

Fixed in 171ae50 — replaced the two absolute filesystem paths with repo-relative `src/engine/...` ones. Kept the line numbers since they still point to the correct context and rot is no worse than the commit SHA pinning; remove them if that policy changes.

<!-- gh-id: 3105047423 -->
### Copilot on [`src/engine/bext.rs:460`](https://github.com/cmk/riffgrep/pull/16#discussion_r3105047423) (2026-04-18 11:09 UTC)

`PACKED_COMMENT_LEN` is documented as the “sole authoritative source”, but the packed comment parsing logic still hard-codes `buf[44..76]` (32 bytes). Either update the parser to use `PACKED_COMMENT_LEN` (and derive the slice end from it) or soften this doc comment so it doesn’t claim full authority while other code paths can drift.


<!-- gh-id: 4134231444 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-18 11:09 UTC](https://github.com/cmk/riffgrep/pull/16#pullrequestreview-4134231444))

## Pull request overview

This PR prevents silent dropping of packed BEXT Description writes in the Lua workflow path by auto-activating the packed schema on previously-unpacked WAVs when any packed-field diffs are present, and it tightens safety/idempotence of packed-schema initialization.

**Changes:**
- Auto-activate packed schema in `write_metadata_changes` for unpacked files when packed fields differ; rescan chunk map after activation.
- Centralize ASCII sanitization/truncation and enforce packed `comment` width at the Rust boundary; fix BPM formatting to avoid leading-space parse failures.
- Update SoundMiner ETL to rescue existing plain-text BEXT Description into packed `comment`, and add workflow-layer tests/proptests for activation + round-trips.

### Reviewed changes

Copilot reviewed 4 out of 4 changed files in this pull request and generated 2 comments.

| File | Description |
| ---- | ----------- |
| src/engine/workflow.rs | Adds packed-schema auto-activation, shared ASCII sanitization/truncation, BPM formatting fix, plus extensive unit + property tests. |
| src/engine/bext.rs | Adds `PACKED_COMMENT_LEN` and reorders init writes so `file_id` is written last; adds idempotence test for partial prior state. |
| scripts/etl_soundminer.lua | Rescues pre-activation plain-text Description into packed `comment`; clarifies UMID stamping behavior. |
| doc/reviews/review-0016.md | Adds/updates local review artifact transcript for the PR. |





<!-- gh-id: 3105047445 -->
### Copilot on [`doc/reviews/review-0016.md:225`](https://github.com/cmk/riffgrep/pull/16#discussion_r3105047445) (2026-04-18 11:09 UTC)

nit: This embedded reply text includes an absolute local path fragment (`/Users/cmk/...`). Even as an example, it leaks a local username; consider redacting to `/Users/<user>/...` (or keeping only repo-relative paths) to keep the review artifact portable and non-identifying.


<!-- gh-id: 3105063956 -->
#### ↳ cmk ([2026-04-18 11:24 UTC](https://github.com/cmk/riffgrep/pull/16#discussion_r3105063956))

Fixed in-flight — the packed-comment parser at src/engine/bext.rs:318 now reads `buf[44..44 + PACKED_COMMENT_LEN]` so the length constant is the single source of truth in all three callers (parser, workflow writer, Lua setter). The `44` offset remains a literal because no other field shares it; adding a `PACKED_COMMENT_OFFSET` constant for one caller felt like over-engineering.

<!-- gh-id: 3105064094 -->
#### ↳ cmk ([2026-04-18 11:24 UTC](https://github.com/cmk/riffgrep/pull/16#discussion_r3105064094))

Redacted — edited the original reply via `gh api PATCH` to drop the user-identifying absolute-path prefix (now reads 'absolute filesystem paths'), and the local mirror in review-0016.md is updated to match. No code change, just the comment text.
