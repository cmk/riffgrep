# PR #27 — Plan 07: Playback reverse path unification

## Summary

Finishes the playback-FSM arc started in Plan 06. Two coordinated
changes land together because they rewrite the same call-sites:

1. **Collapse the dual reverse paths into one.** Previously, reversed
   segments from `play_with_segments` were pre-reversed into an
   appended scratch region (`path 1`) and played forward; the global
   reverse toggle on a forward segment went through a runtime atomic
   (`path 2`). The two paths could compose incorrectly and duplicated
   per-frame buffer memory. Plan 07 replaces both with a per-segment
   `reversed: bool` flag and the XOR identity
   `effective = segment.reversed ^ SourceControl::reversed`, evaluated
   on every frame against the current segment. Reversed segments are
   now stored forward-oriented; `PlaySegment::logical_start` and the
   scratch region are gone.

2. **Wire `PlaybackEngine` through `PlaybackFsm`.** Task 3 of Plan 06
   was deferred to this sprint precisely because it touches the same
   mutators the reverse unification rewrites. Each UI-side mutator now
   dispatches an `FsmInput` (Play / Pause / Resume / Stop / Seek /
   Restart / SetReverse / SetLoop / ProgramEnded on drain), mirrors
   the resulting FSM state into the mixer-thread atomics on
   `SourceControl`, and applies the `MixerCommand` returned by the
   FSM. The atomics remain the lock-free wire to the mixer thread;
   the FSM is the UI-side source of truth. Intents consumed by the
   mixer (`pending_seek`, `pending_restart`) are reflected back via
   `ConsumeSeek` / `ConsumeRestart` from the TUI tick path
   (`update_sample_offset`).

**Reverse-mode `pending_restart` fix** (T6). The restart origin is
now resolved against the first segment's *effective* direction —
`first.end - 1` when reversed, `first.start` otherwise — not the
bare `first.start` / `first.logical_start` the pre-sprint code used.

**Verification.** R1–R6 pin down the XOR unification with inline
`SegmentSource` tests in `playback.rs` (the struct is
module-private; integration tests under `tests/` can't reach it):

| # | Invariant |
|---|---|
| R1 | `forward_seg ⊕ global_rev` produces reverse traversal |
| R2 | `reversed_seg ⊕ global_rev` produces forward traversal (XOR identity) |
| R3 | Reverse-loop boundary engages both fade_out and fade_in ramps |
| R4 | `Seek(p)` during reverse playback lands `control.frame` at `p` |
| R5 | `Restart` with a reversed first segment starts at `first.end - 1` |
| R6 | Toggle-global-reverse pair preserves the frame counter |

A `r1_r2_xor_identity_sample_count` proptest covers randomized
`(start, length, seg_reversed, global_reversed)` vectors and asserts
every segment emits exactly `end - start` samples regardless of XOR
combination — catches any boundary off-by-one.

Plan 06's Q1–Q8 FSM property suite stays green under the new wiring.
`doc/designs/debt-playback.md`'s Testing checklist items 1–5 are
ticked and a `Status (2026-04-22)` block points at this sprint as
the completer.

**Known scope exclusions** (deferred in the plan, confirmed still
right for this sprint):
- Markers Task 5c — independent region.
- Serde retrofit on FSM states (Plan 10).
- TUI FSM refactor (Plan 11+).
- Spectral-null audio-quality harness — R3 asserts ramp engagement
  but not spectral continuity at reverse loop boundaries. Deferred
  until we have a tolerant analysis harness.

**Design deviations from the pre-sprint plan** (details in the plan's
Review section):
- Added a one-shot `past_reverse_start` sentinel on `SegmentSource`
  to make `frame == seg.start` emit inclusively in reverse without
  looping forever at `seg.start == 0`.
- `Input::{ToggleReverse, ToggleLoop, SegmentEnded}` retain narrow
  `#[allow(dead_code)]` — no engine callers, but the proptest
  generator constructs them and they're public FSM API.
- Reverse-path properties landed inline in `playback.rs`, not in
  `tests/engine/playback_fsm/prop.rs`, because `SegmentSource` is
  private to the module.

## Test plan

- [x] `cargo build`, `cargo test --workspace`, `cargo clippy
  --all-targets -- -D warnings`, `cargo fmt --check` — all clean
  locally (the pre-existing env-sensitive `sqlite_count_mode`
  integration test fails on main too; unrelated to this sprint).
- [x] Plan 06 FSM property suite (Q1–Q8) still green after the
  engine wiring — the FSM transitions are unchanged; only the
  caller changed.
- [x] `SegmentSource` inline tests (both existing crossfade /
  sequential / pending-restart and the new R1–R6) all green.
- [ ] Manual TUI smoke: Space (play/pause), Ctrl-P (toggle loop),
  Ctrl-R (reverse), Ctrl-O (restart), scrub during reversed
  playback. Listen for clicks/pops at reverse loop boundaries.

## Local review (2026-04-22)

**Branch:** plan/2026-04-22-02
**Commits:** 6 (origin/main..plan/2026-04-22-02, including the fix
commit for the must-fix item below)
**Reviewer:** Claude (sonnet, independent)

---

### Commit Hygiene

Five commits (six after the fix below), all conventional prefixes.
T1+T2+T3+T6 bundling in `c3facda` is justified — removing
`logical_start` without the XOR identity leaves `cargo test` red.
Linear history, no merge commits. Clean.

### Critical

**Issue 1 — `next()` inlines the XOR instead of calling
`effective_reversed()`.** `src/engine/playback.rs` lines 345–350. The
`on_frame_boundary` path uses the helper everywhere; `next()`
duplicates the two-operand XOR inline. Not a bug today — the `seg_idx`
is always in range at this point — but violates the plan's stated
"keep the XOR identity in one place" invariant. A future change to
direction computation would need to update both sites.

Classification: **follow-up**. The inline is currently correct.

**Issue 2 — `play_with_segments` initial position ignores the FSM's
`reversed` flag.** `src/engine/playback.rs` lines 631–640 (pre-fix).
`Input::Stop` does not clear the FSM's `reversed`, so
`set_reversed(true)` followed by `play_with_segments(...)` entered at
`seg.start` for a forward segment while the mixer's effective
direction was reverse. The first frame boundary tripped
`past_reverse_start` at frame 0 and advanced past the segment without
emitting it.

Classification: **must fix before push**. Fixed in commit `13412f9`
by reading the FSM's `reversed` after the internal `stop()` and
XORing with `seg.reversed` when choosing the entry frame. Regression
test `initial_pos_xors_fsm_reversed_with_segment_reversed` locks it
in (fixture-gated per repo convention).

### Important (assertion-strength follow-ups)

**Issue 3 — R3 asserts ramp engagement but not symmetry or cause.**
`r3_reverse_loop_crossfade_engages_fade_ramps` checks that `fade_out`
became nonzero and that `fade_in` became nonzero afterwards, but it
doesn't verify that a reverse-loop boundary actually caused the
`fade_in` (a seek or other jump could produce the same transition)
or that the two ramps are symmetric (matching `fade_len`). The PR
summary claims R3 asserts "symmetric fade-out/fade-in ramps" — the
test is weaker than the claim.

**Issue 4 — R6 uses `>=` where the stated invariant is equality.**
`r6_toggle_reverse_pair_preserves_frame` asserts `frame_after >=
frame_before`, which passes for any forward drift. The plan's
invariant is position preservation under a double-toggle; the test
should assert `frame_after == frame_before + 2` (one frame per
emit, two emits).

Classification for Issues 3 and 4: **follow-up**. Neither masks a
bug in the engine; both under-specify the invariant and could let a
regression slip silently. A small follow-up PR that tightens the
assertions is worth opening after this lands.

### Plan Conformance

All of T1–T7 landed:
- T1/T2: `PlaySegment.reversed` added, `logical_start` gone, scratch
  region removed from `play_with_segments`.
- T3: `effective_reversed` helper used throughout `on_frame_boundary`.
  `next()` duplicates it inline (Issue 1 above).
- T4: engine routes through FSM with `mirror_fsm_to_atomics`; dead-code
  attrs removed from `Input`/`MixerCommand`/`PlaybackFsm` (narrow ones
  retained on specific unused-but-public variants, documented in plan
  Review).
- T5: audit-only — no callers outside `playback.rs` touch
  `SourceControl` (verified via grep).
- T6: `pending_restart` origin computed via `effective_reversed`.
- T7: R1–R6 present as inline tests; `r1_r2_xor_identity_sample_count`
  proptest covers the randomized vector.

`debt-playback.md` Testing checklist items 1–5 are ticked with
pointers; items 6–7 remain open with documented rationale.

### Risks

- No TODOs or stubs.
- Lock ordering: every site takes one lock at a time and releases
  before the next; `sync_consumed_intents` takes `source_control`
  first (clones `Arc`), releases, then takes `fsm`. No deadlock path.
- `sync_consumed_intents` only fires when `transport == Playing`.
  If the TUI backgrounds mid-playback, the FSM's pending-intent view
  stales until the next tick — documented trade-off, and a
  subsequent `seek_to_sample` safely overwrites a stale pending
  with the new target.

### Recommendations

**Must fix before push:**
- Issue 2: fixed in commit `13412f9`.

**Follow-up:**
- Issue 1: swap the inline XOR in `next()` for a call to
  `effective_reversed` (or cache the segment ref for the frame-emit
  block and pass it).
- Issue 3: tighten R3 to assert `fade_out == fade_in == fade_len` at
  the crossover and that `seg_idx` returned to its pre-loop value.
- Issue 4: tighten R6 to assert exact frame advance (`+ 2`).

<!-- gh-id: 3123967705 -->
### Copilot on [`src/engine/playback.rs:350`](https://github.com/cmk/riffgrep/pull/27#discussion_r3123967705) (2026-04-22 12:36 UTC)

`SegmentSource::next()` re-derives the effective reverse flag by inlining `segment.reversed ^ control.reversed`, which duplicates the XOR identity already centralized in `effective_reversed()`. Consider reusing `effective_reversed()` here (or grabbing the current segment once and passing it through) so future direction changes only need to be updated in one place.

<!-- gh-id: 3123967743 -->
### Copilot on [`src/engine/playback.rs:2062`](https://github.com/cmk/riffgrep/pull/27#discussion_r3123967743) (2026-04-22 12:36 UTC)

`r6_toggle_reverse_pair_preserves_frame`’s assertion (`frame_after >= frame_before`) is weaker than the invariant stated in the comment above it (“should have advanced by 2 …”). As written, it will pass even if the toggle pair causes extra drift forward. Tighten the assertion to match the intended invariant (e.g., exact expected delta) so regressions in frame accounting are caught.
```suggestion
        let expected_frame_after = frame_before + 2;
        // After the pair, frame should have advanced by 2 in the forward
        // direction (no direction change net) — not stuck, not reversed.
        assert_eq!(
            frame_after,
            expected_frame_after,
            "R6: toggle pair changed frame by an unexpected amount (expected {} → {}, got {})",
            frame_before,
            expected_frame_after,
```

<!-- gh-id: 4154687187 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-22 12:36 UTC](https://github.com/cmk/riffgrep/pull/27#pullrequestreview-4154687187))

## Pull request overview

This PR completes Plan 07 by unifying reverse playback semantics (segment-level `reversed` XOR global reverse) and making `PlaybackFsm` the UI-side source of truth, mirrored into mixer-thread atomics.

**Changes:**
- Removed the pre-reversed scratch-buffer path and replaced it with per-segment `reversed: bool` plus `effective = segment.reversed ^ global_reversed`.
- Routed `PlaybackEngine` mutators through `PlaybackFsm`, mirroring FSM state into `SourceControl` atomics and applying `MixerCommand`s.
- Added inline `SegmentSource` reverse-path property tests (R1–R6) and updated related design docs/review records.

### Reviewed changes

Copilot reviewed 5 out of 5 changed files in this pull request and generated 5 comments.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| `src/engine/playback_fsm.rs` | Updates module docs and narrows/removes `dead_code` allowances now that the FSM is wired into the engine. |
| `src/engine/playback.rs` | Implements XOR-based reverse unification, adds `PlaybackFsm` wiring, and expands inline tests for reverse/seek/restart behavior. |
| `doc/reviews/review-00027.md` | Adds an in-repo review record for Plan 07 with identified follow-ups and risk notes. |
| `doc/plans/plan-2026-04-22-02.md` | Adds the Plan 07 sprint plan and review notes for the reverse-path/FSM wiring work. |
| `doc/designs/debt-playback.md` | Updates the playback debt doc checklist/status to reflect Plan 07 completion and test pointers. |
</details>






<!-- gh-id: 3123967797 -->
### Copilot on [`doc/plans/plan-2026-04-22-02.md:184`](https://github.com/cmk/riffgrep/pull/27#discussion_r3123967797) (2026-04-22 12:36 UTC)

The “Properties (must pass)” table still claims the R1–R6 properties live in `tests/engine/playback_fsm/prop.rs` and that R3 asserts “symmetric fade-out/fade-in ramps”. In this PR the properties are implemented as inline `SegmentSource` tests in `src/engine/playback.rs`, and `r3_reverse_loop_crossfade_engages_fade_ramps` only checks ramp engagement (nonzero), not symmetry. Updating this table to match the implemented test location/strength would prevent it from misleading future readers.

<!-- gh-id: 3123967820 -->
### Copilot on [`src/engine/playback.rs:541`](https://github.com/cmk/riffgrep/pull/27#discussion_r3123967820) (2026-04-22 12:36 UTC)

In `PlaybackEngine::play()`, the single `PlaySegment` is always created as `reversed: false`, but global reverse (FSM.reversed mirrored into `SourceControl::reversed`) still makes the effective direction reversed. In that case, starting the source at frame 0 causes the reverse boundary logic (`past_reverse_start` → past_boundary) to terminate playback after emitting just the first frame. Initialize the entry frame/pos (and `control.frame`/`sample_offset`) based on the effective direction (start at `total_frames - 1` when reversed, else 0) before starting the sink.

<!-- gh-id: 3123967845 -->
### Copilot on [`src/engine/playback.rs:657`](https://github.com/cmk/riffgrep/pull/27#discussion_r3123967845) (2026-04-22 12:36 UTC)

`play_with_segments()` computes `first_pos_frame` using `fsm.reversed`, but the newly created `SourceControl` still has default atomics (`reversed=false`, `loop_enabled=false`) until after `sink.append(source)` when `mirror_fsm_to_atomics()` runs. Since `SegmentSource` reads these atomics on its very first frame boundary, there’s a race where initial playback can run with the wrong direction/loop setting. Set `control.reversed`/`control.loop_enabled` to the intended initial values before starting the sink (or mirror before appending) to make startup deterministic.

<!-- gh-id: 3124013407 -->
#### ↳ cmk ([2026-04-22 12:44 UTC](https://github.com/cmk/riffgrep/pull/27#discussion_r3124013407))

Fixed in 95be361 — `next()` now maps through the current segment with `self.effective_reversed(seg)` instead of duplicating the XOR inline, so the identity lives in one place.

<!-- gh-id: 3124013889 -->
#### ↳ cmk ([2026-04-22 12:44 UTC](https://github.com/cmk/riffgrep/pull/27#discussion_r3124013889))

Reframed and fixed in 95be361. The `frame_before + 2` form turns out to be the wrong invariant too: emitting between the two toggles commits a reverse step-back the second toggle can't undo (first emit flips direction mid-next() and decrements pos). The clean net-no-op invariant is "toggle pair with no emit between," so the test now double-toggles atomically and then emits one frame forward, asserting `frame_after == frame_before + 1`. Test comment updated to match.

<!-- gh-id: 3124014320 -->
#### ↳ cmk ([2026-04-22 12:44 UTC](https://github.com/cmk/riffgrep/pull/27#discussion_r3124014320))

Fixed in 95be361 — the Verification table now points at `src/engine/playback.rs` with an implementation note explaining the move (SegmentSource is module-private so integration tests can't reach it). R3's invariant is softened to "engages both ramps"; symmetry and spectral continuity stay in Deferred.

<!-- gh-id: 3124014973 -->
#### ↳ cmk ([2026-04-22 12:44 UTC](https://github.com/cmk/riffgrep/pull/27#discussion_r3124014973))

Real bug — same class as the pre-push Issue 2 fix but for the single-file `play()` path. Fixed in 95be361: `play()` reads `fsm.state().reversed` after the internal `stop()`, sets `entry_frame = total_frames - 1` when global reverse is on, and pre-seeds `control.reversed` + `control.loop_enabled` before `sink.append` so the mixer doesn't race with default atomics on its first frame boundary. Regression test `play_initial_pos_xors_fsm_reversed` locks it in.

<!-- gh-id: 3124015504 -->
#### ↳ cmk ([2026-04-22 12:44 UTC](https://github.com/cmk/riffgrep/pull/27#discussion_r3124015504))

Real race — fixed in 95be361. `play_with_segments` now pre-seeds `control.reversed` and `control.loop_enabled` directly from `fsm.state().reversed` and `global_loop` before constructing the source, so the mixer thread's first frame boundary reads the intended values rather than `SourceControl::new()` defaults. The late `mirror_fsm_to_atomics` call still runs for transport + pending_* atomics; it's idempotent for reversed/loop_enabled at that point.
