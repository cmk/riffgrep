# PR #22 — Plan 05: Markers FSM data-mutation dispatch

## Local review (2026-04-20)

**Branch:** sprint/markers-dispatch
**Commits:** 2 (main..sprint/markers-dispatch)
**Reviewer:** Claude (sonnet, independent)

---

## Review: `sprint/markers-dispatch` — Plan 05 FSM Data-Mutation Dispatch

Reviewing `git diff origin/main...HEAD` (2 commits ahead of main).

### Commit Hygiene

Both commits are conventional and correctly prefixed (`doc:`, `refactor:`). The doc commit is a plan-only addition — no code. The refactor commit is the large migration. The commit ordering (plan first, code second) is acceptable. Both commits should leave `cargo test` passing; the pre-commit hook enforces this, and the test migration is mechanically complete. Atomicity is reasonable for the scope.

### Code Quality

**`unsafe_code = "forbid"`**: No unsafe code appears anywhere in the diff. Clean.

**The three kept `#[allow(dead_code)]` variants (NudgeForward, NudgeBackward, MarkerReset):**

The trade-off is reasonable and the rationale is sound. `nudge_marker` legitimately needs a ZC-snapped absolute position — dispatching `SetSelectedMarker` rather than a raw delta is architecturally correct because the FSM has no access to PCM data. `marker_reset` pre-computes ZC-snapped positions for the same reason. The delta-nudge and pure-quartile-reset variants remain as FSM API surface for future callers and are property-tested. The doc-comments on each variant clearly explain why they are unused from UI code. This is an acceptable and well-documented deviation from Plan 04's deliverable statement. **No action required**, but the plan's "Deferred: None" is technically inaccurate — these three variants could be noted there.

**Stale doc-comment (confidence: 95):**

`src/ui/mod.rs` line 266-267:

```
/// data ownership (MarkerConfig) remains in
/// `preview.markers` until Task 5 of Plan 04 lands.
```

This sprint is exactly Plan 05 (Task 5 of the FSM carve-out). The comment was never updated. The FSM now owns marker data, so this claim is false.

**Dead error path in `export_markers_csv` (confidence: 85):**

`src/ui/mod.rs`, the `else` branch after the FSM CSV dispatch:

```rust
self.set_status("Export failed: FSM produced no output".to_string());
return;
```

The FSM's `output()` always returns `Some(Output::WriteCsv(p.clone()))` for `ExportMarkersCsv`. The `consume()` returns `None` only if the state machine's transition errors out, which the current infallible FSM design cannot produce. This branch is permanently unreachable and can never be triggered in practice. Same applies to the equivalent `import_markers_csv` error branch. This is misleading dead code, not a safety net.

**`export_markers_csv` data source:**

`markers` is captured from `current_markers_or_default()` before the FSM dispatch. Since the FSM is the source of truth post-Plan-05 and `current_markers_or_default()` reads `marker_fsm.config()` as its primary source, this is correct. The fallback to `row.markers` only activates in test setups that bypass the preview path — a documented edge case. No bug.

**`adjust_rep` selection side-effect:**

When `selected_marker()` returns `None`, the code now calls `set_selected_marker(Some(s))` to force FSM selection before dispatching `IncrementRep`/`DecrementRep`. This is correct because `adjust_rep` in the FSM keys off `state.selection`. When `selected_marker()` returns `Some(idx)`, the FSM's `state.selection` is already set to `idx` by construction (both read from the same FSM state). No divergence.

**`clear_nearest_marker` dual-computation:**

The UI pre-computes `nearest_idx` from `bank_before` solely to construct the status message, then dispatches `ClearNearestMarker(sample)` to the FSM which re-computes nearest independently. Both computations use the same active bank snapshot and the same cursor value, so they are always consistent. The pre-computation is necessary because we can't query which slot the FSM cleared after the fact. This pattern is correct, if slightly redundant. The code comment explains it.

**`widgets.rs` direct field access:**

`app.marker_fsm.config()` accessed at `src/ui/widgets.rs:726`. `marker_fsm` is `pub` on `App`, so module-level access is fine.

### Test Coverage

**Migrated tests:** All ~30 test sites identified in the diff are mechanically correct migrations. The new assertions use `app.marker_fsm.config()` where the old code used `app.preview.unwrap().markers.unwrap()`. Semantics are preserved.

**New unit tests for `ToggleInfiniteLoop`:** Three focused tests cover: normal toggle (1→15→1), no-op without selection, and bank-sync mirroring. These are adequate spot checks.

**Property test coverage:** `ToggleInfiniteLoop` is added to all three generator strategies (`any_input`, `transitions_no_sync_toggle`, `transitions_no_display_toggle`). The existing P1–P8 property suite will exercise the new variant against invariants like no-panic and state consistency.

**Missing property test for `ToggleInfiniteLoop` invariant:** No dedicated property asserting that `ToggleInfiniteLoop` only touches the selected segment's rep and only toggles between 1 and `REP_MAX`. The existing properties will catch panics and obvious regressions but won't assert the toggle semantics under adversarial inputs. Per CLAUDE.md ("property-based testing is mandatory for any module that parses, encodes, or transforms data"), this is a gap. This falls under transformation — the FSM transforms rep state. Low severity since the unit tests cover it, but it's a convention gap.

**`toggle_infinite_loop` — no `set_selected_marker` call in `None` branch:**

Unlike `adjust_rep`, `toggle_infinite_loop` returns early with "Select a marker first" when `selected_marker()` is `None`. It does not force a selection. This is intentional (different UX contract) and matches the pre-existing behavior. No issue.

### Plan Conformance

| Task | Status |
|------|--------|
| 1. Rewrite each mutation method to dispatch through FSM | Complete. All 9 targets dispatched. `nudge_marker` and `marker_reset` use `SetSelectedMarker`/`LoadConfig` rather than the delta/quartile variants, with documented rationale. `toggle_infinite_loop` uses a new dedicated `ToggleInfiniteLoop` variant (the plan anticipated this). |
| 2. Remove `PreviewData.markers` | Complete. Field removed; all read sites migrated to `marker_fsm.config()`. |
| 3. Remove `sync_fsm_from_preview` | Complete. Bridge deleted with all call-sites. |
| 4. Remove `#[allow(dead_code)]` from data-mutating variants | Partially complete. The block-level allow is removed; three narrow per-variant allows remain for NudgeForward, NudgeBackward, MarkerReset. These have documented rationale. The plan's stated deliverable ("they all have live callers") is not met for these three. |
| 5. Migrate unit tests | Complete. All tests updated; two tests for `PreviewData.markers` field existence deleted as they tested a removed abstraction. |

**Out-of-plan additions:**

- `Input::ToggleInfiniteLoop` is a new FSM variant introduced by this sprint. The plan anticipated it ("may need a dedicated `Input::ToggleInfiniteLoop` — evaluate during implementation"). It's a justified emergent requirement, not scope creep.
- `toggle_infinite_rep()` private helper in `marker_fsm.rs` is the corresponding implementation. Also justified.
- `set_selected_marker` side-effect added to the `None` branches of `adjust_rep`. The old code used `active_bank_mut()` which didn't require selection; the FSM's `adjust_rep` does require selection. This is a necessary implementation change not called out in the plan. It's correct and small.

### Risks

**No TODOs or stubs introduced.**

**Behavioral change in `adjust_rep` when no marker selected (confidence: 80):**

When `selected_marker()` is `None` and `current_segment_index()` returns a valid segment, the new code calls `self.set_selected_marker(Some(s))` as a side effect before dispatching `IncrementRep`. The old code used `active_bank_mut()` to target the segment index directly without touching selection state. This means `adjust_rep` now has the side effect of setting the selected marker when called from the cursor-segment fallback path. This is new behavior — after a rep adjustment without an explicit selection, the TUI will now show the cursor segment as "selected." Depending on UX intent this may or may not be desirable, but it is an undocumented behavioral change in the diff.

**`clear_nearest_marker`: no FSM-side guard for empty bank after dispatch:**

The UI checks `bank_before.is_empty()` before dispatching. The FSM's `ClearNearestMarker` handler calls `nearest_defined_slot(active_ref(&next), cursor)` which returns `None` if all slots are `MARKER_EMPTY`, and the `if let Some(slot) = ...` guard means it's a no-op. So even if `bank_before.is_empty()` passed, the FSM would safely no-op. Belt-and-suspenders. No risk.

**Path traversal / injection in CSV export/import:**

The CSV path is derived from `row.meta.path.with_extension("markers.csv")` — the source path comes from the indexed WAV file paths, not from user text input. No injection risk.

**`on_preview_ready` signature change:**

This is a public method (`pub fn on_preview_ready`). Callers in `run_tui` and tests have all been updated. No external consumers visible in the codebase.

---

### Recommendations

**Must fix before push:**

1. **Stale doc-comment on `App.marker_fsm` field** (`src/ui/mod.rs`, lines 266-267). Update to state that the FSM owns `MarkerConfig` as of Plan 05. Confidence: 95. One-line fix.

**Follow-up (future work):**

2. **Dead error paths in `export_markers_csv` / `import_markers_csv`**. The `else` branches guarding against `None` FSM output for CSV inputs are unreachable given the FSM's infallible output contract. Consider removing them or replacing with `unreachable!()` to communicate intent. Not a bug, but misleading.

3. **Property test for `ToggleInfiniteLoop` semantics**. Add a proptest asserting that a double-toggle is idempotent and that only the selected segment's rep changes. CLAUDE.md mandates property tests for transformation modules.

4. **`adjust_rep` selection side-effect** is undocumented. Add a doc-comment note that the cursor-segment fallback path sets the FSM selection as a side effect, so callers should be aware this changes TUI selection state.

5. **Plan 05 "Deferred: None" statement** is inaccurate — NudgeForward/NudgeBackward/MarkerReset remain unused from UI. If these variants are intentionally part of the API for future callers, add a brief deferred entry.

---

### Resolution

- Must-fix #1 (stale doc-comment): fixed in this branch; the `App.marker_fsm` doc-comment now states the FSM owns `MarkerConfig` as of Plan 05.
- Follow-ups #2–#5 deferred for a future sprint — none block merge.

<!-- gh-id: 3114097296 -->
### Copilot on [`src/ui/mod.rs:2193`](https://github.com/cmk/riffgrep/pull/22#discussion_r3114097296) (2026-04-20 22:53 UTC)

`ExportMarkersCsv` always produces `Some(Output::WriteCsv(_))` in `marker_fsm` (see `MarkerBankMachine::output`), and the transition for this input is state-preserving. That makes the `else` branch here effectively unreachable and can mislead future readers into thinking export can fail at the FSM-dispatch step. Consider removing the `else` branch and treating a non-`WriteCsv`/`None` return as an invariant violation (e.g., `debug_assert!`/`unreachable!()`).
```suggestion
        let out_path = match self
            .marker_fsm
            .consume(Input::ExportMarkersCsv(csv_path.clone()))
        {
            Some(Output::WriteCsv(out_path)) => out_path,
            other => {
                debug_assert!(
                    false,
                    "ExportMarkersCsv must produce Some(Output::WriteCsv(_)); got {:?}",
                    other
                );
                unreachable!(
                    "ExportMarkersCsv must produce Some(Output::WriteCsv(_))"
                );
            }
```

<!-- gh-id: 3114097314 -->
### Copilot on [`src/ui/mod.rs:2234`](https://github.com/cmk/riffgrep/pull/22#discussion_r3114097314) (2026-04-20 22:53 UTC)

Same issue as export: `ImportMarkersCsv` deterministically yields `Some(Output::ReadCsv(_))` from the FSM, so the `else` branch (“FSM produced no output”) should be unreachable under the current `marker_fsm` contract. Consider removing this branch or making it an invariant check so the control flow reflects reality.
```suggestion
            unreachable!(
                "marker_fsm contract violated: ImportMarkersCsv must produce Some(Output::ReadCsv(_))"
            );
```

<!-- gh-id: 3114097323 -->
### Copilot on [`src/engine/marker_fsm.rs:161`](https://github.com/cmk/riffgrep/pull/22#discussion_r3114097323) (2026-04-20 22:53 UTC)

This comment mentions “riffgrep's `Ctrl-L/K` nudge”, but the default key bindings for nudge are on arrow keys (Ctrl-Left/Right and Alt-Left/Right) while Ctrl-L is bound to marker selection. This mismatch makes the rationale harder to trust; consider removing the specific key chords or updating them to match `src/ui/actions.rs`.
```suggestion
    /// Not currently wired up from the UI — the current marker-nudge
    /// path is zero-crossing based and dispatches
```

<!-- gh-id: 3114097341 -->
### Copilot on [`doc/plans/plan-2026-04-19-02.md:32`](https://github.com/cmk/riffgrep/pull/22#discussion_r3114097341) (2026-04-20 22:53 UTC)

The plan claims the `#[allow(dead_code)]` on data-mutating `marker_fsm::Input` variants “comes off” because all variants have live callers, but the implementation keeps per-variant `#[allow(dead_code)]` for `NudgeForward`, `NudgeBackward`, and `MarkerReset`. Also, the `## Deferred` section says “None” even though these variants remain intentionally unused from the UI. Please update the plan to reflect the actual delivered state (and rationale).

<!-- gh-id: 4143980423 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-20 22:53 UTC](https://github.com/cmk/riffgrep/pull/22#pullrequestreview-4143980423))

## Pull request overview

This PR completes the Plan 05 refactor to make `MarkerFsm` the single source of truth for marker configuration and routes all UI marker mutations through FSM inputs instead of mutating `preview` state directly.

**Changes:**
- Removed `PreviewData.markers` and updated preview loading to inject marker configs via `App::on_preview_ready(..., markers)` using `Input::LoadConfig`.
- Routed marker mutation actions (set/clear/nudge/snap/reset/rep adjustments/CSV import-export) through `MarkerFsm::consume(Input::...)` and migrated UI reads to `app.marker_fsm.config()`.
- Extended the FSM with `Input::ToggleInfiniteLoop`, plus updated generators and unit tests to cover it.

### Reviewed changes

Copilot reviewed 6 out of 6 changed files in this pull request and generated 6 comments.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| tests/engine/marker_fsm/generators.rs | Adds `ToggleInfiniteLoop` into proptest input generators to exercise the new transition. |
| src/ui/widgets.rs | Switches waveform marker overlay to read marker config from `marker_fsm` instead of `preview`. |
| src/ui/mod.rs | Removes `PreviewData.markers`, updates preview readiness API, rewires all marker edits/CSV I/O through FSM dispatch, and migrates tests to read from `marker_fsm.config()`. |
| src/engine/marker_fsm.rs | Introduces `Input::ToggleInfiniteLoop`, implements its state transition, and adds unit tests. |
| doc/reviews/review-0022.md | Adds a local review record for the Plan 05 changes. |
| doc/plans/plan-2026-04-19-02.md | Adds the Plan 05 implementation plan document. |
</details>






<!-- gh-id: 3114097350 -->
### Copilot on [`doc/plans/plan-2026-04-19-02.md:45`](https://github.com/cmk/riffgrep/pull/22#discussion_r3114097350) (2026-04-20 22:53 UTC)

Task 1’s mapping says `nudge_marker` should dispatch `Input::NudgeForward/Backward` and `marker_reset` should dispatch `Input::MarkerReset`, but the PR intentionally uses `SetSelectedMarker` (after ZC search) and `LoadConfig` (after ZC-snapped preset). To avoid the plan becoming misleading reference material, update these bullets to match the chosen approach (or add a note explaining the divergence).
```suggestion
- `nudge_marker(forward, n)` → run the zero-crossing search in UI,
  then dispatch `Input::SetSelectedMarker(zc)`
- `snap_zero_crossing(forward)` → `Input::SetSelectedMarker(zc)`
- `marker_reset()` → build the zero-crossing-snapped reset preset,
  then dispatch `Input::LoadConfig(config)`
```

<!-- gh-id: 3114097360 -->
### Copilot on [`src/ui/mod.rs:5193`](https://github.com/cmk/riffgrep/pull/22#discussion_r3114097360) (2026-04-20 22:53 UTC)

This test comment still refers to `preview.markers`, but that field has been removed in this PR. Updating the comment to refer to the FSM state (e.g., empty `marker_fsm.config()` before `ensure_markers()`) will keep the test rationale accurate.

<!-- gh-id: 3114128684 -->
### Copilot on [`src/ui/mod.rs:1206`](https://github.com/cmk/riffgrep/pull/22#discussion_r3114128684) (2026-04-20 23:03 UTC)

`ensure_markers()` now installs a preset whenever the FSM config is empty, even if `self.preview` is `None` (no file/preview loaded). Since `SelectNextMarker`/`SelectPrevMarker` call `ensure_markers()` unconditionally, this can seed marker state in an otherwise empty app and make later marker actions behave as if markers exist without a preview context. Consider restoring the previous guard (return early when `self.preview.is_none()`) so presets are only installed once a preview is available (optionally still defaulting to `preset_shot()` when `audio_info` is missing).

<!-- gh-id: 3114128701 -->
### Copilot on [`src/ui/mod.rs:2194`](https://github.com/cmk/riffgrep/pull/22#discussion_r3114128701) (2026-04-20 23:03 UTC)

In `export_markers_csv`, the `else` branch for `consume(Input::ExportMarkersCsv(..))` looks unreachable with the current FSM: `MarkerBankMachine::output()` always returns `Some(Output::WriteCsv(..))` for this input and `transition()` always returns `Some(_)`, so `MarkerFsm::consume()` should never yield `None` here. Consider replacing this with an `expect`/`unreachable!()` (or removing the branch) to avoid carrying a misleading error path.

<!-- gh-id: 3114128709 -->
### Copilot on [`src/ui/mod.rs:2238`](https://github.com/cmk/riffgrep/pull/22#discussion_r3114128709) (2026-04-20 23:03 UTC)

Same as export: the `else` branch after `consume(Input::ImportMarkersCsv(..))` appears unreachable given the current FSM contract (`output()` always returns `Some(Output::ReadCsv(..))` for this input and `transition()` is infallible). Consider using `expect`/`unreachable!()` or returning the path directly from the UI without pattern-matching on `Option` to keep the control flow honest.
```suggestion
        let output = self
            .marker_fsm
            .consume(Input::ImportMarkersCsv(csv_path.clone()))
            .expect("marker_fsm contract: ImportMarkersCsv must yield Some(Output::ReadCsv(_))");
        let Output::ReadCsv(in_path) = output else {
            unreachable!(
                "marker_fsm contract: ImportMarkersCsv must yield Output::ReadCsv(_); got {output:?}"
            )
```

<!-- gh-id: 4144024292 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-20 23:03 UTC](https://github.com/cmk/riffgrep/pull/22#pullrequestreview-4144024292))

## Pull request overview

Copilot reviewed 6 out of 6 changed files in this pull request and generated 4 comments.





<!-- gh-id: 3114128725 -->
### Copilot on [`doc/plans/plan-2026-04-19-02.md:33`](https://github.com/cmk/riffgrep/pull/22#discussion_r3114128725) (2026-04-20 23:03 UTC)

Plan doc is now factually out of sync with the implementation in this PR: it states the `#[allow(dead_code)]` on data-mutating `marker_fsm::Input` variants comes off and that nothing is deferred, but the code keeps per-variant `#[allow(dead_code)]` on `NudgeForward`, `NudgeBackward`, and `MarkerReset` with rationale, and the UI dispatch paths use `SetSelectedMarker`/`LoadConfig` instead of the delta/quartile variants. Please update this plan to match what actually shipped (tasks and the "Deferred" section) so future readers aren’t misled.

<!-- gh-id: 3114130330 -->
#### ↳ cmk ([2026-04-20 23:03 UTC](https://github.com/cmk/riffgrep/pull/22#discussion_r3114130330))

Addressed in da5fa05: replaced the dead else branch with an exhaustive match whose non-WriteCsv arms hit `unreachable!()`, documenting the FSM contract explicitly.

<!-- gh-id: 3114130423 -->
#### ↳ cmk ([2026-04-20 23:03 UTC](https://github.com/cmk/riffgrep/pull/22#discussion_r3114130423))

Addressed in da5fa05: same pattern as the export path — exhaustive match on `Some(Output::ReadCsv(_))` with `unreachable!()` for any other variant.

<!-- gh-id: 3114130546 -->
#### ↳ cmk ([2026-04-20 23:03 UTC](https://github.com/cmk/riffgrep/pull/22#discussion_r3114130546))

Addressed in da5fa05: removed the hard-coded "Ctrl-L/K" keybinding from the doc comment; now points readers to `src/ui/actions.rs` for the actual keybindings.

<!-- gh-id: 3114130689 -->
#### ↳ cmk ([2026-04-20 23:03 UTC](https://github.com/cmk/riffgrep/pull/22#discussion_r3114130689))

Addressed in da5fa05: plan's Task 4 now notes that most `#[allow(dead_code)]` attrs come off but `NudgeForward`/`NudgeBackward`/`MarkerReset` stay. Deferred section documents the rationale (kept as FSM API surface, covered by proptest).

<!-- gh-id: 3114130821 -->
#### ↳ cmk ([2026-04-20 23:03 UTC](https://github.com/cmk/riffgrep/pull/22#discussion_r3114130821))

Addressed in da5fa05: Task 1 mapping for `nudge_marker` now reflects the ZC-aware `Input::SetSelectedMarker` dispatch, and `marker_reset` maps to `Input::LoadConfig` with an App-built SOF+EOF config.

<!-- gh-id: 3114130937 -->
#### ↳ cmk ([2026-04-20 23:03 UTC](https://github.com/cmk/riffgrep/pull/22#discussion_r3114130937))

Addressed in da5fa05: test comment now references the FSM's empty `MarkerConfig` and LoadConfig dispatch instead of the removed `preview.markers` field.
