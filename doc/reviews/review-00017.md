# Review 0000 — sprint/markers-fsm (Tier 1, local)

**Date:** 2026-04-18
**Scope:** 8 commits, +1972/-7 LOC, Plan 03 implementation
**Reviewer:** code-reviewer agent (pre-push local review)

## Summary

The FSM module is tight and well-reasoned: `StateMachineImpl` is clean,
every write-path correctly branches on `bank_sync` and `active_bank`, and
the `is_edit()` gate is scoped to exactly the right set of inputs. The
property suite wires P1/P3–P8 correctly with no tautologies observed;
`SharedInvariants` locks SUT-reference drift from the first step. Zero
must-fix items. Four follow-ups are noted below, all deferrable to the
Task 4b App-integration sprint.

## Must-fix

None.

## Follow-ups

- **P3/P4/P8 `prop_assume!` rejection rate**
  (`tests/engine/marker_fsm/prop.rs:171, 187, 305`): All three properties
  gate on `selection_is_at_defined_slot`, but the initial FSM state has
  `selection = None` and `any_input` generates `SelectNextMarker` with
  only 1/18 weight. On short prefixes the reject rate can exceed 50%,
  meaning the invariant may be exercised on fewer than 30 actual cases
  at the 64-case default. Consider a generator that guarantees the
  prefix ends with a defined selection, or raise
  `RIFFGREP_PROPTEST_CASES` in CI to compensate. Task 6 / follow-up
  sprint.

- **`reset_to_quartiles` does not clamp `sof`**
  (`src/engine/marker_fsm.rs:394–402`): If a caller passes
  `sof > MAX_MARKER_POS`, the computed marker positions exceed the
  sentinel guard without hitting the `NudgeForward` saturation path.
  The proptest generator caps `sof` at 1M (far below `MAX_MARKER_POS ≈ 4B`)
  so no test exercises this. Add `let sof = sof.min(MAX_MARKER_POS);`
  at the top of the function; fix in the App-integration sprint when
  live sample counts flow in.

- **Dead guard `if seg > 3`** (`src/engine/marker_fsm.rs:413`):
  `Selection::as_index()` returns only 0..=3 by construction, making
  this branch statically unreachable. Remove when `#[allow(dead_code)]`
  comes off in the Task 4b sprint.

- **`unit.rs` regressions are a stub**
  (`tests/engine/marker_fsm/unit.rs`): The plan's Verification table
  lists `3fa73fc`, SPRINT12 F2, SPRINT11, and SPRINT7 F7 as required
  regression tests in this file. The inline `#[cfg(test)]` block in
  `marker_fsm.rs` covers `6d23741`, `46168e6`, and SPRINT12 F1; the
  remaining four are unported. Defer to Task 4b when the legacy `App`
  behavior is directly exercisable through the FSM.

## Spot checks performed

- `is_edit()` gate: confirmed it includes all marker/rep data-mutating
  inputs and excludes `SelectNextMarker`, `SelectPrevMarker`,
  `ToggleBank`, `ToggleBankSync`, `ToggleMarkerDisplay` — no stray
  non-edit input is silently blocked when `visible=false`.
- `write_slot` mirroring: traced the `bank_sync=true` and
  `bank_sync=false` paths in `write_slot`, `clear_active`, and
  `adjust_rep` — all three consistently apply the same branching logic.
- `nudge_selected`: verified early-return on `MARKER_EMPTY` and on
  no-selection; forward path saturates at `MAX_MARKER_POS`, backward
  path floors at 0.
- `cycle_selection` wrap: `None + Next` → first defined (SOF);
  `None + Prev` → last defined; empty `defined` vec returns current
  selection unchanged without panicking.
- P7 `DisabledFixedPointTest`: `transitions_no_display_toggle` excludes
  `ToggleMarkerDisplay`; `check_invariants` asserts only `config`
  (correct — `selection`, `bank_sync`, `active_bank` can legitimately
  mutate while `visible=false` because they are not edits).
  Precondition assert is belt-and-suspenders.
- P6 `BankSyncPreservationTest`: generator excludes `ToggleBankSync`;
  asserts `bank_a == bank_b` after every step — correctly states the
  invariant.
- Three named regression tests in the inline `#[cfg(test)]` block:
  `6d23741` (nudge hits the selected slot, not a fixed slot),
  `46168e6` (cleared marker returns `MARKER_EMPTY`, not 0), SPRINT12 F1
  (rep increment targets `reps[selection.as_index()]`). All assert the
  specific field that was wrong before the fix — none are tautologies.

<!-- gh-id: 3106274162 -->
### Copilot on [`src/engine/marker_fsm.rs:73`](https://github.com/cmk/riffgrep/pull/17#discussion_r3106274162) (2026-04-19 04:29 UTC)

The doc comment says this `Bank` is “Re-exported from `crate::ui::Bank`”, but the PR changes `src/ui/mod.rs` to re-export `Bank` *from this module*. Consider rewording to avoid the direction-of-dependency inversion (e.g., “Re-exported by `crate::ui::Bank`”).
```suggestion
/// Re-exported by `crate::ui::Bank`; this is the canonical
```

<!-- gh-id: 3106274168 -->
### Copilot on [`tests/engine/marker_fsm/prop.rs:13`](https://github.com/cmk/riffgrep/pull/17#discussion_r3106274168) (2026-04-19 04:29 UTC)

This module-level doc references `crate::gen`, but the module is named `generators` (and imported as `use crate::generators;`). Updating the link/path avoids stale documentation and potential broken intra-doc links.

<!-- gh-id: 3106274170 -->
### Copilot on [`doc/plans/plan-2026-04-18-02.md:102`](https://github.com/cmk/riffgrep/pull/17#discussion_r3106274170) (2026-04-19 04:29 UTC)

This plan section describes a `state_machine!` design with a `BankPair<T>`, 23 `Input` variants, and an `Output` enum with `Noop/DidEdit/WroteCsv`, but the implementation in `src/engine/marker_fsm.rs` differs (e.g., uses `StateMachineImpl`, `MarkerConfig` for banks, fewer `Input` variants, and `consume` returns `Option<Output>`). Since the top of the doc marks these tasks as “Shipped”, consider updating this spec block (and the later `gen.rs`/`TestConfig` defaults sections) to match what actually landed so future readers don’t implement against stale requirements.
```suggestion
**Solution (shipped):** `src/engine/marker_fsm.rs` now contains a
concrete FSM implementation rather than the original `state_machine!`
sketch from this plan.

- **State:** the FSM owns marker selection, active bank, bank-sync,
  visibility, and the per-bank marker data used by the UI. The shipped
  implementation uses the module's concrete types (including
  `MarkerConfig` for banked marker storage), not the earlier
  `BankPair<T>` sketch shown in this draft.
- **Inputs:** the shipped `Input` enum covers the marker-editing and
  navigation operations needed by the UI/property tests, but it does
  not match the earlier 23-variant sketch one-for-one. Keep this plan
  aligned to the enum in `src/engine/marker_fsm.rs` rather than this
  original exhaustive list.
- **Outputs:** the implementation reports side effects via
  `Option<Output>` from `consume`, rather than always returning an
  `Output`. Tests can still assert on edits and write-like effects
  without performing real I/O.
- **Transition rules:** implement the `debt-fsm.md` invariants,
  including the visibility/edit guards and bank-sync behaviour used by
  the shipped FSM.

**API surface (shipped):**
- `pub struct MarkerFsm { machine: StateMachineImpl<...> }`
- `pub fn consume(&mut self, input: Input) -> Option<Output>`
```

<!-- gh-id: 3106274174 -->
### Copilot on [`doc/designs/debt-fsm.md:25`](https://github.com/cmk/riffgrep/pull/17#discussion_r3106274174) (2026-04-19 04:29 UTC)

The roadmap claims a per-FSM file layout of `gen.rs`/`prop.rs`/`unit.rs`/`test.rs`, but this PR’s marker suite uses `generators.rs` (because `gen` is a Rust 2024 keyword, as noted in `tests/engine/marker_fsm/test.rs`). Consider updating this bullet to reflect the actual convention to prevent future FSM suites from following an un-compilable layout name.

<!-- gh-id: 4135467856 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-19 04:29 UTC](https://github.com/cmk/riffgrep/pull/17#pullrequestreview-4135467856))

## Pull request overview

Introduces an explicit markers finite state machine (`rust-fsm`) plus a `proptest-state-machine` property suite to validate key marker invariants, as the first step in the broader FSM refactor roadmap (markers → playback → TUI).

**Changes:**
- Added `src/engine/marker_fsm.rs` implementing `MarkerFsm` + transition rules and regression unit tests.
- Added a dedicated marker FSM property test binary and supporting generators/models under `tests/engine/marker_fsm/`.
- Updated docs/plan/roadmap to reflect the FSM migration direction and deferred App wiring; unified `Bank` by re-exporting from the FSM module.

### Reviewed changes

Copilot reviewed 13 out of 13 changed files in this pull request and generated 7 comments.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| Cargo.toml | Adds `rust-fsm` + `proptest-state-machine` and registers the `marker_fsm` test binary. |
| src/engine/mod.rs | Exposes the new `marker_fsm` engine module. |
| src/engine/marker_fsm.rs | Implements the marker FSM, inputs/outputs, helpers, and unit regressions. |
| src/ui/mod.rs | Removes local `Bank` enum and re-exports `engine::marker_fsm::Bank`. |
| tests/engine/marker_fsm/test.rs | Entrypoint for marker-FSM property suite and proptest config. |
| tests/engine/marker_fsm/prop.rs | Reference model + P1/P3/P4/P5/P6/P7/P8 property tests/harness. |
| tests/engine/marker_fsm/generators.rs | Action generators for properties, including restricted transition streams. |
| tests/engine/marker_fsm/unit.rs | Placeholder module docs for future integration-level regressions. |
| tests/proptest-regressions/marker_fsm/test.txt | New pinned regression seed(s) (currently placed under `tests/`). |
| tests/proptest-regressions/marker_fsm/prop.txt | New pinned regression seed(s) (currently placed under `tests/`). |
| doc/plans/plan-2026-04-18-02.md | Plan 03 write-up, status, and implementation notes. |
| doc/designs/debt-playback.md | Updates playback design doc status/mapping into FSM roadmap. |
| doc/designs/debt-fsm.md | Records FSM refactor decision/roadmap and expected test layout. |
</details>






<!-- gh-id: 3106274180 -->
### Copilot on [`tests/engine/marker_fsm/generators.rs:56`](https://github.com/cmk/riffgrep/pull/17#discussion_r3106274180) (2026-04-19 04:29 UTC)

`#[allow(dead_code)]` here (and the similar one on `transitions_no_display_toggle`) looks stale: both generators are referenced from `tests/engine/marker_fsm/prop.rs` (P6/P7). Removing the allow keeps dead-code warnings meaningful and avoids papering over future unused helpers.

<!-- gh-id: 3106274184 -->
### Copilot on [`tests/proptest-regressions/marker_fsm/test.txt:5`](https://github.com/cmk/riffgrep/pull/17#discussion_r3106274184) (2026-04-19 04:29 UTC)

These proptest regression seeds are stored under `tests/proptest-regressions/...`, but proptest’s default failure persistence looks for `proptest-regressions/...` at the crate root (see existing `proptest-regressions/engine/id3.txt`). As-is, these seeds likely won’t be replayed. Move this file to the crate-root `proptest-regressions/marker_fsm/` directory (or configure `failure_persistence`).

<!-- gh-id: 3106274187 -->
### Copilot on [`tests/proptest-regressions/marker_fsm/prop.txt:4`](https://github.com/cmk/riffgrep/pull/17#discussion_r3106274187) (2026-04-19 04:29 UTC)

These proptest regression seeds are stored under `tests/proptest-regressions/...`, but proptest’s default failure persistence looks for `proptest-regressions/...` at the crate root. Unless you’re overriding `failure_persistence`, this file won’t be read and the saved cases won’t be replayed. Move it to `proptest-regressions/marker_fsm/` (or configure the persistence path explicitly).
```suggestion
# Seeds for failure cases proptest has generated in the past.
# This file is stored under tests/proptest-regressions/, which is not
# proptest's default auto-loaded persistence location.
#
# Move it to proptest-regressions/marker_fsm/prop.txt at the crate
# root, or configure failure_persistence explicitly, so these cases
# are replayed before novel cases are generated.
```
