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
