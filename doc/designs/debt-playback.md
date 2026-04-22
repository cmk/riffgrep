## Status (2026-04-18)

This document's "Unification Plan" is folded into the FSM refactor
roadmap (see `debt-fsm.md`). Playback will be the **second** region
ported to `rust-fsm`, after markers.

Mapping:
- The **"Unification Plan"** (below) becomes the transition table for the
  playback FSM: `PlaybackState × SegmentDirection × {Loop, Seek, Restart}`.
- The **"Testing"** checklist (items 1–7 below) becomes the must-pass
  property set in `tests/engine/playback_fsm/prop.rs`.
- The **"Risks"** section becomes the list of qualified-unreachable
  states that the property suite verifies (e.g. "if `global_reversed`
  is false and all segments have `reversed=false`, no reverse code path
  executes").

Sprint plan: not yet written. Will be authored after markers ships.

---

# TODO: Reverse Playback Unification

## Current State (as of 2026-03-26)

There are two independent code paths for reversing audio playback:

### Path 1: Pre-reverse buffer copy (marker-ordered segments)

**Location:** `src/engine/playback.rs`, `play_with_segments()`

When a segment has `reverse: true` (because m1 > m2 in the marker bank),
`play_with_segments` copies that segment's frames into a scratch region
appended to the decode buffer, in reverse order. A forward-reading
`PlaySegment` then points into this scratch region. The `logical_start`
field maps buffer-space frames back to file-space for the UI cursor.

**Status:** Existing code, fully tested, works correctly for marker-ordered
reversed segments. This is the only path that handles reversed segments
created by the marker system.

### Path 2: Atomic reversed flag (runtime toggle)

**Location:** `src/engine/playback.rs`, `SourceControl::reversed` +
`SegmentSource::next()` + `SegmentSource::on_frame_boundary()`

The `reversed` `AtomicBool` on `SourceControl` is toggled by the TUI's
`ReversePlayback` action (via `PlaybackEngine::set_reversed()`). The TUI
tracks `App::reversed` and calls `engine.set_reversed()`.

When true:
- `next()` steps backwards by 2 frames after emitting each frame (one to
  undo the forward channel iteration, one to go backwards). Channels
  within a frame are still emitted L→R.
- `on_frame_boundary()` swaps origin/boundary: traversal runs from
  `seg.end-1` down to `seg.start` instead of `seg.start` up to `seg.end`.
  Fade-out triggers near `seg.start`, loop jumps back to `seg.end-1`.

**Status:** Implemented but only lightly tested via manual TUI use. No
unit tests for the reversed boundary logic or crossfade behavior.

## Interaction Between Paths

Both paths can be active simultaneously. A marker-ordered reversed segment
(path 1, pre-reversed buffer) that is also runtime-reversed (path 2,
atomic flag) would double-reverse — the pre-reversed buffer is then
traversed backwards, effectively playing forward. **This is untested.**

## Known Issues

1. **No unit tests for path 2.** The `on_frame_boundary` direction-aware
   logic (origin/boundary swap, reversed fade timing) has zero test coverage.

2. **Crossfade ramps in reverse are unverified.** Forward: fade-out triggers
   `CROSSFADE_FRAMES` before `seg.end`. Reverse: fade-out should trigger
   `CROSSFADE_FRAMES` after `seg.start`. The current implementation attempts
   this but hasn't been verified with audio output.

3. **`pending_seek` in reverse mode.** Seek targets a file-space frame and
   `jump_to()` sets `pos` directly. In reverse mode, the source then reads
   backwards from that point, which is correct. But seeking near `seg.start`
   (the reverse-mode boundary) may trigger immediate segment advance.

4. **`pending_restart` in reverse mode.** Restart resets `seg_idx` to 0 and
   jumps to `playlist[0].start`. In reverse mode this should jump to
   `playlist[0].end - 1` instead. The current implementation does handle
   this via `next_origin` but only for segment advance, not restart.

## Unification Plan

Collapse both paths into the atomic flag approach:

1. Remove the pre-reverse buffer copy from `play_with_segments`
2. Store a per-segment `reversed: bool` on `PlaySegment`
3. When entering a segment, compute effective direction:
   `effective = segment.reversed ^ global_reversed`
   and store it on `SourceControl::reversed`
4. Remove `logical_start` mapping — frame reporting uses the real buffer
   position, and the UI maps it based on segment direction
5. Test crossfade behavior in reverse (fade-out near `seg.start`, fade-in
   after jump to `seg.end`)

### Prerequisites
- Unit tests for path 2 (see Testing below)
- Verify crossfade audio quality in reverse mode
- Verify seek and restart behavior in reverse mode

### Risks
- Crossfade ramp direction: fade-out counts down frames before boundary,
  which in reverse means counting *up* from `seg.start`. The `at_boundary`
  and `past_boundary` calculations need careful verification.
- Seek behavior: `pending_seek` targets file-space frames. In reverse mode
  after the buffer copy is removed, seek targets map directly to buffer
  positions (no more scratch region offset).
- The `pending_restart` logic currently uses `first.start` / `first.logical_start`.
  After unification it needs to use `first.end - 1` when reversed.

### Testing
- [x] Unit test: forward segment + global reverse = reversed playback
  — `r1_forward_seg_plus_global_reverse_plays_backwards` in `src/engine/playback.rs`
- [x] Unit test: reversed segment + global reverse = forward playback (XOR)
  — `r2_reversed_seg_plus_global_reverse_plays_forwards` in `src/engine/playback.rs`
- [x] Unit test: crossfade at loop boundary in reverse mode
  — `r3_reverse_loop_crossfade_engages_fade_ramps` in `src/engine/playback.rs`
- [x] Unit test: seek during reversed playback lands at correct position
  — `r4_seek_during_reverse_lands_at_target` in `src/engine/playback.rs`
- [x] Unit test: restart during reversed playback starts from correct end
  — `r5_restart_reversed_first_starts_at_end_minus_1` in `src/engine/playback.rs`
- [ ] Verify UI cursor tracks correctly during reversed playback — manual TUI smoke, not automated.
- [ ] Audio quality test: no clicks/pops at reverse loop boundaries — R3 asserts ramp engagement; a spectral-null harness is deferred (see Plan 07 Review).

## Status (2026-04-22)

Completed by Plan 07 (`doc/plans/plan-2026-04-22-02.md`). The
pre-reverse buffer copy ("path 1") is gone; segments now carry a
`reversed: bool` flag and the effective direction at every frame is
`segment.reversed ^ SourceControl::reversed` (XOR identity). The
`pending_restart` reverse-origin bug listed above (items 3–4 of the
Bugs section) is fixed — reverse restart lands at `first.end - 1`.
