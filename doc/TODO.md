# TODO: Reverse Playback Unification

## Current State

There are two independent code paths for reversing audio playback:

### Path 1: Pre-reverse buffer copy (marker-ordered segments)

**Location:** `src/engine/playback.rs`, `play_with_segments()`

When a segment has `reverse: true` (because m1 > m2 in the marker bank),
`play_with_segments` copies that segment's frames into a scratch region
appended to the decode buffer, in reverse order. A forward-reading
`PlaySegment` then points into this scratch region. The `logical_start`
field maps buffer-space frames back to file-space for the UI cursor.

**Pros:**
- Crossfade logic is unchanged (always reads forward)
- `on_frame_boundary` doesn't need direction awareness
- No per-sample branch in the hot path

**Cons:**
- Allocates extra buffer space (up to 2x for a fully reversed file)
- Can't toggle mid-playback without restarting
- `logical_start` mapping adds complexity to frame reporting

### Path 2: Atomic reversed flag (runtime toggle)

**Location:** `src/engine/playback.rs`, `SourceControl::reversed` + `SegmentSource::next()`

The `reversed` `AtomicBool` on `SourceControl` is toggled by the TUI's
`ReversePlayback` action. When true, `next()` steps backwards by 2 frames
after emitting each frame (one to undo the forward step from channel
iteration, one to actually go backwards). `on_frame_boundary` uses
direction-aware origin/boundary calculations.

**Pros:**
- Toggles instantly mid-playback, no restart or re-decode
- Zero extra allocation
- Works with any segment configuration

**Cons:**
- Per-frame atomic load in the hot path (relaxed ordering, ~free on ARM)
- Boundary logic is more complex (origin/boundary swap)
- Crossfade ramps may behave differently in reverse (untested edge case)

## Interaction Between Paths

Both paths can be active simultaneously. A marker-ordered reversed segment
(path 1) that is also runtime-reversed (path 2) would double-reverse,
effectively playing forward. This is probably fine but is untested.

## Unification Plan

Collapse both paths into the atomic flag approach:

1. Remove the pre-reverse buffer copy from `play_with_segments`
2. Instead, store a per-segment `reversed: bool` on `PlaySegment`
3. When entering a segment, set `SourceControl::reversed` from the segment's flag
4. XOR with the global toggle: `effective = segment.reversed ^ global_reversed`
5. Remove `logical_start` mapping — frame reporting uses the real buffer position
   and the UI maps it based on segment direction
6. Test crossfade behavior in reverse (fade-out near seg.start, fade-in after jump to seg.end)

### Risks
- Crossfade ramp direction needs verification — currently fade-out counts
  down frames before boundary, which in reverse means frames *after* the
  boundary in buffer space
- Seek behavior: `pending_seek` targets a file-space frame; in reverse mode
  the source needs to map this correctly
- The `pending_restart` logic assumes forward segment ordering

### Testing
- Add unit test: forward segment + global reverse = reversed playback
- Add unit test: reversed segment + global reverse = forward playback (double-reverse)
- Add unit test: crossfade at loop boundary in reverse mode
- Verify UI cursor tracks correctly during reversed playback
