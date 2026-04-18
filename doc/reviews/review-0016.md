# PR #16 — Auto-activate BEXT packed schema in workflow writer

## Local review (2026-04-18)

**Branch:** sprint/workflow-bext-activation
**Commits:** 2 (main..sprint/workflow-bext-activation)
**Reviewer:** Claude (sonnet, independent)

---

### Commit Hygiene

Both commits are clean. `5864d5d` is a focused refactor (write-order only, no logic change). `a931efb` is a focused fix with its tests in the same commit. Conventional commit prefixes (`refactor:`, `fix:`) match repo style. Bodies are detailed and call out the ETL contract. Both commits are buildable and testable independently. No issues here.

---

### Code Quality

**BPM alignment fix — correct direction, but analogous fields may share the same bug.**

Confidence: 85

The reader (`parse_bpm_ascii`, `/Users/cmk/Music/Software/Anarkhiya/riffgrep/src/engine/bext.rs` line 394) calls `s.parse::<u16>().ok()` where `s = decode_fixed_ascii(bytes)`. The existing test fixture at line 912 writes `"164 "` (digit-then-space), and the test at line 953 asserts `bpm == Some(164)`, which passes — so the reader does handle trailing whitespace correctly via `str::parse`. The old writer used `{v:>4}` (right-align = `" 128"`), which would make `" 128".parse::<u16>()` return `Err` because Rust's `str::parse` does not trim leading whitespace. The fix to `{v:<4}` (left-align = `"128 "`) is correct.

However, no analogous field uses numeric formatting — `rating`, `subcategory`, `category`, `genre_id`, `sound_id`, `usage_id`, and `key` are all string fields written with `write_ascii` and read with `decode_fixed_ascii`, which trims trailing nulls/spaces. The BPM field is the only field that round-trips through a numeric parser; the bug and fix are isolated.

**`packed_diffs` expression structurally drifts from the write block.**

Confidence: 85

`/Users/cmk/Music/Software/Anarkhiya/riffgrep/src/engine/workflow.rs` lines 420–428 enumerate nine fields for `packed_diffs`. The write block (lines 471–500) also enumerates nine fields. The comment says "keep in sync." This is a real latent risk: adding a tenth packed field requires two edits in two places with no compiler enforcement. If `packed_diffs` is missing a new field, an unpacked file that only changed that field will skip activation and silently drop the write. The "keep in sync" comment is insufficient given that the consequence of drift is silent data loss. A structural fix (e.g., computing `packed_diffs` by comparing serialised packed blocks, or extracting a `packed_fields_differ(before, after) -> bool` helper that the write block calls into via the same closure chain) would eliminate the class. This is not a bug today but is a maintenance trap.

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
