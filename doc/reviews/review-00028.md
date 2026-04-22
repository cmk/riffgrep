# PR #28 — Plan 07 follow-up: post-merge review items

## Summary

Small follow-up PR for the two actionable code findings Copilot posted
on PR #27 after it had already merged, plus three plan-doc drift
corrections on `plan-2026-04-22-02.md`.

**Code fixes:**

1. **`PlaybackEngine::stop()` cleans up the sink unconditionally.**
   Previously the `sink.stop()` branch gated on the FSM returning
   `Some(MixerCommand::Stop)`, which doesn't happen when transport is
   already `Stopped` — e.g., after `state()`'s drain-grace path
   dispatched `Input::ProgramEnded` on a natural program end. In that
   window the sink was still allocated and leaked. The fix keeps the
   FSM dispatch (for transport + `pending_*` bookkeeping) but
   unconditionally takes the sink and calls `sink.stop()` if it was
   `Some`.

2. **`PlaybackEngine::restart_program()` no longer hard-zeros
   `sample_offset`.** In reverse mode the mixer's restart origin is
   `first.end - 1`, not 0; the old write snapped the UI cursor to 0
   for 1–2 ticks until the next `update_sample_offset()` read the
   mixer-updated `control.frame`. The fix drops the `= 0` line and
   lets the tick loop pick up the correct origin from the mixer
   atomic. `paused_elapsed` and `play_start` stay — they're
   wall-clock bookkeeping, unrelated to frame position.
   `restart_program` is still `#[allow(dead_code)]` (no UI caller),
   so this is preemptive correctness — any future binding of the
   action will render the cursor properly from the first tick.

**Plan-doc drift** on `doc/plans/plan-2026-04-22-02.md` (in-repo
historical artifact, still worth keeping accurate):

- Context paragraph reframed to describe the *pre-sprint*
  `#[allow(dead_code)]` state rather than asserting it as current.
- T1's `PlaySegment` snippet corrected — the struct is module-private
  with `reps: u8` (255 is the infinite-loop sentinel), not
  `pub reps: u32`.
- T3's solution text rewritten to match the implemented design:
  `SourceControl::reversed` stays the bare global flag, and the XOR
  is computed per frame via `SegmentSource::effective_reversed()`,
  not stored on the atomic.

**Scope note.** This sprint is tiny on purpose. PR #27 already landed
the substantive Plan 07 work (~1000 LOC across the reverse-path
rewrite, FSM wiring, and R1–R6 property tests); the round-2 Copilot
findings just arrived too late to ride with it. Grouping them as a
follow-up keeps the audit trail clean and avoids letting the
findings rot on a branch.

## Test plan

- [x] `cargo test --lib engine::playback` — 66 tests pass, unchanged
  from PR #27.
- [x] `cargo clippy --all-targets -- -D warnings` — clean.
- [x] `cargo fmt --check` — clean.
- [ ] Manual spot-check that no existing UI flow regressed on the
  `stop()` change (sink cleanup happens whether the FSM returned
  `MixerCommand::Stop` or not; other state resets are unchanged).

## Local review (2026-04-22)

**Branch:** plan/2026-04-22-03
**Commits:** 3 (origin/main..plan/2026-04-22-03)
**Reviewer:** Claude (sonnet, independent)

---

### Commit Hygiene

Three commits, conventional prefixes (`plan:` → `fix:` → `doc:`),
`plan:` first per TDD workflow. Pieces cleanly split. No issues.

### Code Quality

**T1 (`stop()` unconditional sink cleanup).** FSM dispatch is
idempotent when already Stopped; no downstream consumer depends on
the discarded `MixerCommand` return. `sample_offset = 0` at the end
of `stop()` is unchanged (full-reset semantics, unrelated to T2).
Back-to-back `stop()` calls remain safe: second call sees sink=None
and the `if let Some` branch short-circuits. No issues.

**T2 (`restart_program()` drops `sample_offset = 0`).** Under
Paused, `update_sample_offset()` is gated on `Playing`, so
`sample_offset` stays at its pre-restart value until resume — no
worse than the pre-fix behaviour (which snapped to the wrong value
of 0). `restart_program` still has no live UI caller
(`#[allow(dead_code)]`), so this is pre-emptive correctness.
`paused_elapsed`/`play_start` bookkeeping stays (wall-clock, not
frame-space). No issues.

### Plan Conformance

T1, T2, T3 match the code and doc changes exactly. The drift-fix
corrections on `plan-2026-04-22-02.md` are accurate: Context
paragraph reframes pre-sprint state, T1 snippet shows the real
module-private struct with `reps: u8`, T3 solution text matches the
actual `effective_reversed()` per-frame XOR implementation.
`Deferred: None` is accurate.

### Risks

- Double `stop()`: safe (sink.take()=None, FSM Stopped idempotent).
- Paused restart with stale `sample_offset`: no worse than the prior
  behaviour (0 was also wrong for reverse mode); acceptable for a
  dead-code function.
- T3 doc drift re-sync: corrections target an already-merged
  historical plan; no live code depends on it.

### Must fix before push

None.

### Follow-up (future work)

**Test-coverage precision for T1.** The plan's Verification section
lists three existing tests (`test_stop_when_already_stopped`,
`test_stop_clears_drain_start`, `test_playback_state_transitions`)
as covering the T1 fix. None of them exercises the specific scenario
being fixed — sink allocated, FSM already `Stopped` after
drain-grace `ProgramEnded` — because they either start with no sink
or call `stop()` from `Playing`. A targeted test that calls
`test_create_empty_sink()`, dispatches the FSM to `Stopped` via
`ProgramEnded`, then calls `stop()` and asserts the sink was
cleaned up would close the gap. Not blocking — the fix is correct
and the edge has no live caller today — but worth tracking.

<!-- gh-id: 4158495752 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-22 23:33 UTC](https://github.com/cmk/riffgrep/pull/28#pullrequestreview-4158495752))

## Pull request overview

Follow-up to Plan 07 (PR #27) addressing two post-merge code review findings in the playback engine and correcting drift in the Plan 07 documentation.

**Changes:**
- Make `PlaybackEngine::stop()` always take and stop the sink (even when the FSM is already `Stopped`).
- Remove eager `sample_offset = 0` in `PlaybackEngine::restart_program()` to avoid incorrect cursor snaps in reverse mode.
- Update Plan 07 documentation to match the implemented design (e.g., `PlaySegment` shape and per-frame XOR reversal).

### Reviewed changes

Copilot reviewed 4 out of 4 changed files in this pull request and generated 3 comments.

| File | Description |
| ---- | ----------- |
| src/engine/playback.rs | Fix sink cleanup in `stop()`; adjust `restart_program()` cursor handling comment/logic context. |
| doc/reviews/review-00028.md | Adds a PR review record describing the follow-up items and verification notes. |
| doc/plans/plan-2026-04-22-03.md | Adds a plan doc for this follow-up sprint, including a verification section. |
| doc/plans/plan-2026-04-22-02.md | Corrects Plan 07 doc drift to match the merged implementation details. |





<!-- gh-id: 3127428053 -->
### Copilot on [`doc/plans/plan-2026-04-22-03.md:72`](https://github.com/cmk/riffgrep/pull/28#discussion_r3127428053) (2026-04-22 23:33 UTC)

The Verification section claims the `stop()` leak fix is covered by existing tests, but none of the listed tests exercises the specific scenario being fixed (sink allocated while the FSM transport is already `Stopped` after the drain-grace `ProgramEnded` path). Either add a targeted regression test and list it here, or adjust this text to avoid overstating coverage.
```suggestion
No new property tests in this follow-up. Existing tests cover adjacent
behavior for the touched code paths, but they do not separately pin
down T1's exact post-drain-grace `ProgramEnded` -> already-`Stopped`
sink-cleanup edge:

- `stop()` behavior: existing `test_stop_when_already_stopped`,
  `test_stop_clears_drain_start`, `test_playback_state_transitions`
  cover related stop-state bookkeeping and transitions; this plan does
  not claim they exercise the specific leaked-sink scenario described
  in T1.
```

<!-- gh-id: 3127428076 -->
### Copilot on [`src/engine/playback.rs:777`](https://github.com/cmk/riffgrep/pull/28#discussion_r3127428076) (2026-04-22 23:33 UTC)

Consider adding a regression test for the scenario described in the PR: after `state()` transitions to `Stopped` via the drain-grace `ProgramEnded` path, the FSM transport is already `Stopped` while `self.sink` is still `Some`. A test that reproduces this and then calls `stop()` should assert that the sink is taken/cleaned up, preventing the leak from regressing.

<!-- gh-id: 3127428082 -->
### Copilot on [`src/engine/playback.rs:971`](https://github.com/cmk/riffgrep/pull/28#discussion_r3127428082) (2026-04-22 23:33 UTC)

This comment implies the next `update_sample_offset()` tick will pick up the mixer-updated restart origin, but `update_sample_offset()` is gated on `PlaybackState::Playing` (it returns early when Paused). Please clarify the comment (or adjust behavior) so it’s accurate about when the UI cursor will actually resync after a restart, especially if `restart_program()` is invoked while paused.
```suggestion
        // frame boundary after it consumes `pending_restart`; while
        // playback is running, the next `update_sample_offset()` tick
        // picks that up and resyncs the UI cursor. If restart is requested
        // while paused, `update_sample_offset()` will not run until
        // playback resumes, so the cursor stays at its paused position
        // until then.
```

<!-- gh-id: 3127484525 -->
#### ↳ cmk ([2026-04-22 23:53 UTC](https://github.com/cmk/riffgrep/pull/28#discussion_r3127484525))

Fixed in 917a7ae — the Verification section now lists the new regression test explicitly (`test_stop_cleans_sink_after_drain_grace_program_end`) and frames the older three tests as adjacent coverage rather than claiming they exercise the drain-grace edge.

<!-- gh-id: 3127484710 -->
#### ↳ cmk ([2026-04-22 23:53 UTC](https://github.com/cmk/riffgrep/pull/28#discussion_r3127484710))

Added in 917a7ae — `test_stop_cleans_sink_after_drain_grace_program_end` reproduces the scenario exactly: create an empty sink, force state=Playing with an expired `drain_start`, call `state()` to trip the drain-grace path (which dispatches `Input::ProgramEnded` and lands FSM + state atomic at Stopped while the sink stays allocated), then `stop()` and assert the sink is taken. Also adds a `test_has_sink()` helper so the assertion is observable without exposing the sink field.

<!-- gh-id: 3127484859 -->
#### ↳ cmk ([2026-04-22 23:53 UTC](https://github.com/cmk/riffgrep/pull/28#discussion_r3127484859))

Fixed in 917a7ae — rewrote the comment to be explicit about the gating. The next `update_sample_offset()` tick resyncs the UI cursor *while transport is Playing*; if restart is invoked while Paused, `update_sample_offset()` short-circuits (it's gated on Playing), so the cursor stays at its paused position until playback resumes, at which point the tick catches up naturally.
