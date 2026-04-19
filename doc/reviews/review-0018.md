# PR #18 — Markers FSM App Integration (Task 4b carve-out)

## Local review (2026-04-18)

**Branch:** sprint/markers-app-carve
**Base:** sprint/markers-fsm (PR #17)
**Commits:** 9 (sprint/markers-fsm..sprint/markers-app-carve) — Tasks 1–4, 5a–b, 6, 7 + plan/status doc
**Reviewer:** code-reviewer agent (pre-push local review)

## Summary

Zero must-fix. FSM/preview sync is complete and consistent across all
9 mutation sites; `MarkerConfig: Copy` confirms the `on_preview_ready`
ordering is safe; selection cycling, visibility gate, and file-change
reset all behave correctly under the new architecture. Three follow-ups
noted — all pre-existing or acknowledged deferred work for Plan 05.

## Must-fix

None.

## Critical path verification

### FSM / preview sync coverage

All 9 `preview.markers` mutation sites are accounted for.
`set_marker`, `nudge_marker`, `snap_zero_crossing`,
`clear_nearest_marker`, `clear_bank_markers`, and `adjust_rep` each
call `sync_fsm_from_preview()` unconditionally after mutating.
`ensure_markers`, `marker_reset`, `import_markers_csv`, and the save-BEXT
write path each dispatch `LoadConfig(cfg)` directly — equivalent,
without the bridge. `on_preview_ready` dispatches `LoadConfig` before
assigning `self.preview = Some(data)`. No sync gap exists.

### `MarkerConfig: Copy` in `on_preview_ready`

`MarkerConfig` derives `Copy` (`src/engine/bext.rs:696`); `Option<MarkerConfig>`
is therefore also `Copy`. `data.markers.unwrap_or_else(...)` copies the
option; `data` is fully intact when stored on the next line. The clean
`cargo build` confirms this. No partial-move hazard.

### Selection cycling correctness

`select_next_marker` / `select_prev_marker` call `ensure_markers()` (which
dispatches `LoadConfig` if markers were just initialized), then dispatch
`SelectNextMarker` / `SelectPrevMarker`, then call
`seek_to_selected_marker()`. The FSM's `cycle_selection` uses
`state.config` (kept in sync by the `sync_fsm_from_preview` /
`LoadConfig` pattern). `seek_to_selected_marker` reads the marker sample
from `preview.markers` via `active_bank_ref()` — the same source that was
synced. The seek lands at the correct sample.

### `toggle_marker_display` selection-clear delegation

FSM's `ToggleMarkerDisplay` transition clears `selection` when flipping
to hidden (`marker_fsm.rs:303`). `App::toggle_marker_display` dispatches
to the FSM and then reads `markers_visible()` for the status message. No
App-side duplicate clear needed or present. Behavior matches the
historical App logic.

### `set_selected_marker` FSM bypass

The production call site (`mod.rs:823`) passes `None` only — clears
selection on file change. This is unconditionally safe. Test call sites
pass `Some(n)` to seed non-default selections for isolated behavioral
tests; none of those test paths depend on the selection being consistent
with the FSM config invariants.

## Follow-ups

**F1 — Dead guard in `select_next_marker` / `select_prev_marker` (pre-existing)**

`defined_marker_indices()` unconditionally prepends `0` (SOF), so
`defined_marker_indices().is_empty()` can never be true. The guard at
`mod.rs` lines 1954 and 1969 is unreachable. This predates the sprint;
`defined_marker_indices` is expected to be removed in Task 5c, at which
point the guard disappears naturally.

**F2 — P3/P4/P8 `prop_assume!` rejection rate (carried from review-0017)**

The review-0017 `reset_to_quartiles` clamp and dead-guard fixups landed
in `b5d6b8b`. The `prop_assume!` rejection rate concern for the P3/P4/P8
property suite in `tests/engine/marker_fsm/prop.rs` is still outstanding.
The plan correctly defers new App-level properties to Plan 05; the
outstanding note should be captured in the Plan 05 scope document.

**F3 — Dual source of truth for defined-marker reads until Task 5c**

In `select_next_marker`, the guard reads `preview.markers` via
`defined_marker_indices()`. The FSM's subsequent `SelectNextMarker` reads
FSM config. Both are kept in sync by the `sync_fsm_from_preview` /
`LoadConfig` pattern, so behavior is correct today. This dual-source
pattern is the documented deferred state and will be resolved by Task
5c's full edit-dispatch migration.

## Commit hygiene

Nine atomic commits, each migrating a single bare field or closely-
related group: `fae3017` (add field), `e5e3154` (markers_visible),
`3f919fa` (bank_sync), `dba45a7` (active_bank), `2e4ed18` (LoadConfig
FSM input), `08b82c4` (sync bridge), `aa19376` (selected_marker),
`b5d6b8b` (cleanup + review-0017 fixups), `0fa454e` (plan status doc).
Conventional commit prefixes match repo style. No merge commits.

## Build gates

- `cargo build` — clean
- `cargo clippy --all-targets -- -D warnings` — clean
- `cargo test` — 1667 pass (806 + 822 + 5 + 26 + 8)
- `cargo fmt --check` — clean
- `cargo test --release --test marker_fsm` — 0.03 s (≪ 60 s budget)

## Recommendation

**Ship.** Zero must-fix. FSM/preview sync is complete and consistent
across all 9 mutation sites; selection cycling, visibility gate, and
file-change reset behave correctly under the new architecture. The three
follow-ups are pre-existing (F1) or documented deferred work for Plan 05
(F2, F3).

<!-- gh-id: 3106309693 -->
### Copilot on [`src/ui/mod.rs:1214`](https://github.com/cmk/riffgrep/pull/18#discussion_r3106309693) (2026-04-19 05:10 UTC)

`ensure_markers()` derives `total` via `duration_secs * sample_rate` (float math) even though `AudioInfo.total_samples` is available and explicitly documented as the exact integer to use for sample-position arithmetic. This can introduce rounding drift and makes the threshold/preset decision inconsistent with the rest of the code that uses `total_samples`. Use `ai.total_samples` instead of recomputing from floats here.
```suggestion
            let total = p.audio_info.as_ref().map(|ai| ai.total_samples);
```

<!-- gh-id: 4135496190 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-19 05:10 UTC](https://github.com/cmk/riffgrep/pull/18#pullrequestreview-4135496190))

## Pull request overview

Integrates the markers finite-state machine into the TUI `App` by moving marker scalar state (selection/bank/sync/visibility) onto `MarkerFsm`, and keeping the FSM’s `config` synchronized with `preview.markers` via a new `Input::LoadConfig`.

**Changes:**
- Migrates `selected_marker`, `active_bank`, `bank_sync`, and `markers_visible` off `App` fields and behind `MarkerFsm` accessors/inputs.
- Adds `Input::LoadConfig(MarkerConfig)` to `MarkerFsm` (bypassing the visibility gate) and unit tests covering replacement/selection-clearing behavior.
- Adds an `App::sync_fsm_from_preview()` bridge and wires it into several in-place `preview.markers` mutation paths.

### Reviewed changes

Copilot reviewed 4 out of 4 changed files in this pull request and generated 1 comment.

| File | Description |
| ---- | ----------- |
| `src/ui/widgets.rs` | Updates marker/bank display reads to use new `App` accessors backed by `MarkerFsm`. |
| `src/ui/mod.rs` | Removes marker scalar fields from `App`, adds `marker_fsm`, accessors, `LoadConfig` dispatch, and the `sync_fsm_from_preview` bridge. |
| `src/engine/marker_fsm.rs` | Introduces `Input::LoadConfig`, updates transitions (selection clearing on hide / on config swap), and adds unit tests. |
| `doc/plans/plan-2026-04-18-03.md` | Adds Plan 04 Task 4b status/plan document describing the carve-out and deferred follow-ups. |


<details>
<summary>Comments suppressed due to low confidence (1)</summary>

**src/ui/mod.rs:2063**
* `toggle_infinite_loop()` mutates `preview.markers` (reps) but does not call `sync_fsm_from_preview()` afterward. Other in-place marker mutations in this file do sync the FSM, so this leaves `marker_fsm.config` stale relative to `preview.markers` and can make subsequent FSM-driven reads inconsistent once reps are read through the FSM. Call `sync_fsm_from_preview()` after the mutation (in both the synced and single-bank branches).
```
        if self.bank_sync() {
            if let Some(ref mut preview) = self.preview
                && let Some(ref mut markers) = preview.markers
            {
                let cur = markers.bank_a.reps[seg];
                let new_val = if cur == 15 { 1 } else { 15 };
                markers.bank_a.reps[seg] = new_val;
                markers.bank_b.reps[seg] = new_val;
                let label = if new_val == 15 { "inf" } else { "1" };
                self.set_status(format!("Segment {} rep: {label}", seg + 1));
            }
```
</details>



<!-- gh-id: 3106346252 -->
#### ↳ cmk ([2026-04-19 05:42 UTC](https://github.com/cmk/riffgrep/pull/18#discussion_r3106346252))

Fixed in a4a44e6 — swapped the float arithmetic for `ai.total_samples` directly.

<!-- gh-id: 3106351096 -->
### Copilot on [`src/ui/mod.rs:1378`](https://github.com/cmk/riffgrep/pull/18#discussion_r3106351096) (2026-04-19 05:48 UTC)

`clear_bank_markers()` reports "Banks A+B cleared" and calls `sync_fsm_from_preview()` even when there is no `preview`/`preview.markers` to clear (the inner `if let` simply skips). This can silently reset the FSM config to `MarkerConfig::empty()` and give a misleading status. Consider early-returning with "No markers" when `preview.markers` is `None`, or only syncing / setting the cleared status when a clear actually occurred.
```suggestion
            let Some(ref mut preview) = self.preview else {
                self.set_status("No markers".to_string());
                return;
            };
            let Some(ref mut markers) = preview.markers else {
                self.set_status("No markers".to_string());
                return;
            };
            markers.bank_a = crate::engine::bext::MarkerBank::empty();
            markers.bank_b = crate::engine::bext::MarkerBank::empty();
```

<!-- gh-id: 3106351110 -->
### Copilot on [`src/engine/marker_fsm.rs:140`](https://github.com/cmk/riffgrep/pull/18#discussion_r3106351110) (2026-04-19 05:48 UTC)

The Plan/Task reference in this comment appears inconsistent with the rest of the PR docs (Plan 04 uses Task 5c / Plan 05 for the full edit-dispatch migration). Updating the pointer here would avoid sending readers to a non-existent/renamed task.
```suggestion
/// sync the result with LoadConfig. Plan 04 Task 5c / Plan 05
/// completes the carve-out by routing those dispatches through here,
/// at which point the allow below comes off.
```

<!-- gh-id: 3106351115 -->
### Copilot on [`src/engine/marker_fsm.rs:572`](https://github.com/cmk/riffgrep/pull/18#discussion_r3106351115) (2026-04-19 05:48 UTC)

This `#[allow(dead_code)]` note refers to "Plan 04 Task 4c", but the plan/status docs in this PR refer to the follow-up as Task 5c / Plan 05. Please align the task reference so it stays searchable and unambiguous.
```suggestion
    #[allow(dead_code)] // Consumed by Plan 05 Task 5c; read from tests today.
```

<!-- gh-id: 3106351122 -->
### Copilot on [`doc/plans/plan-2026-04-18-03.md:11`](https://github.com/cmk/riffgrep/pull/18#discussion_r3106351122) (2026-04-19 05:48 UTC)

The Status section references "Task 4 (preview.markers → FSM config migration)" and says the full removal is deferred to **Task 4c**, but the task table directly below calls preview.markers syncing Task 5b/5c. Please align the numbering (e.g., Task 5 / 5c) to avoid a contradiction in the plan.
```suggestion
deferred to **Task 5c**.
```

<!-- gh-id: 4135539356 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-19 05:48 UTC](https://github.com/cmk/riffgrep/pull/18#pullrequestreview-4135539356))

## Pull request overview

Copilot reviewed 5 out of 5 changed files in this pull request and generated 5 comments.





<!-- gh-id: 3106351126 -->
### Copilot on [`doc/reviews/review-0018.md:8`](https://github.com/cmk/riffgrep/pull/18#discussion_r3106351126) (2026-04-19 05:48 UTC)

The header says "PR #0000" but this file is `review-0018.md` and appears to describe PR #18. Please update the PR number in the title (and any other identifiers that depend on it) so the review record is accurately attributable.

<!-- gh-id: 3106368651 -->
#### ↳ cmk ([2026-04-19 06:08 UTC](https://github.com/cmk/riffgrep/pull/18#discussion_r3106368651))

Fixed in 18ed029 — the synced branch now short-circuits to "No markers" when the preview/config is absent, instead of reporting a phantom clear and reloading empty config into the FSM.

<!-- gh-id: 3106368689 -->
#### ↳ cmk ([2026-04-19 06:08 UTC](https://github.com/cmk/riffgrep/pull/18#discussion_r3106368689))

Fixed in 18ed029 — aligned all "Plan 04 Task 4c" references (Input enum comment, config() allow attr, sync_fsm_from_preview doc) to "Task 5c" to match the plan's status table.

<!-- gh-id: 3106368705 -->
#### ↳ cmk ([2026-04-19 06:09 UTC](https://github.com/cmk/riffgrep/pull/18#discussion_r3106368705))

Fixed in 18ed029 along with the other Task 4c → Task 5c alignments.

<!-- gh-id: 3106368739 -->
#### ↳ cmk ([2026-04-19 06:09 UTC](https://github.com/cmk/riffgrep/pull/18#discussion_r3106368739))

Fixed in 18ed029 — status line says "Task 5c" now, matching the task-status table.

<!-- gh-id: 3106368773 -->
#### ↳ cmk ([2026-04-19 06:09 UTC](https://github.com/cmk/riffgrep/pull/18#discussion_r3106368773))

Fixed in 18ed029 — header updated to "PR #18".
