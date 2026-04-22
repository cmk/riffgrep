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
