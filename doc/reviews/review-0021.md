# PR #21 — Search / Results / Similarity FSM (Plan 08)

## Local review (2026-04-19)

**Branch:** sprint/search-fsm
**Scope:** Plan 08 Tasks 1, 2, 5, 6, 7 shipped; Tasks 3, 4 deferred
**Reviewer:** code-reviewer agent (pre-push local) + in-sprint resolution

## Summary

Scaffold + property suite ship cleanly. The FSM implements the
transport × mode state space from Plan 08 §1; the runner holds the
data layer the FSM intentionally excludes per `tui-fsm.md` §3 and
returns `DispatchResult { output, action }` without performing any
I/O; the property suite covers R1–R7 plus a full
`prop_state_machine!` harness for R3 (no-spawn-in-Similarity). Tier 1
agent review flagged 2 must-fix + 3 follow-ups; all 5 resolved
in-sprint in `0671ff8` before push.

## Must-fix (resolved in-sprint)

**M1 — `ExitSimilarityMode` unconditionally reset transport.** The
arm ran `mode=Remote`, `transport=Idle`, `debounce_dirty=true`
regardless of current mode, so a defensive runner call from Remote
would clobber a live Settled/Running transport. Guarded on
`state.mode == Mode::Similarity`; no-op when already Remote.
Regression test `exit_similarity_from_remote_is_noop` added; the
`Input::ExitSimilarityMode` doc comment now explicitly says "no-op
when already in `Mode::Remote`."

**M2 — Plan spec + transition comment claimed DebounceTick emits
`CancelSearch + SpawnSearch`.** `rust-fsm`'s `output()` returns a
single `Option<Output>`, so the FSM emits only `SpawnSearch`; the
cancel-then-spawn pair is encoded in the `SpawnSearch` runner
contract (already documented on the enum variant). Corrected the
in-code transition comment and the plan §1 text with an explicit
note about the single-output constraint, so Task 4 implementers
don't wire cancel handling to a `CancelSearch` that never fires on
tick.

## Follow-ups (resolved in-sprint)

**F1 — R4 test comment corrected.** Tied to M1; the R4 prefix
comment now references `exit_similarity_from_remote_is_noop`
explicitly rather than the (formerly wrong) "no-op if already
Remote" assertion.

**F2 — R5 prefix generator was unnecessarily narrow.** `QueryChanged`
is mode-independent in the transition function, so excluding mode
toggles from the prefix restricted coverage without strengthening
the property. Swapped R5 to `input_seq_strategy`; the now-unused
`no_mode_toggle_seq_strategy` helper was deleted.

**F3 — `SearchFailed` coverage was asymmetric.** The inline
`search_failed_lands_settled_like_cancel` test only exercised from
`Pending`. Added `search_failed_always_lands_settled` covering all
four transport variants, mirroring the existing
`search_cancelled_always_lands_settled` — symmetry check so error
paths can never orphan Pending/Running in any reachable state.

## Spot checks performed

- `dispatch()` routing: `TypedAction::LoadSample` is synthesized via
  `selected_path().map(TypedAction::LoadSample)` so empty results
  produce `None` action (verified in
  `fire_selection_action_none_when_results_empty`).
- Similarity snapshot: case-insensitive path substring match, empty
  query restores full snapshot, `None` snapshot is a true no-op.
- `SearchFailed` vs `SearchCancelled`: identical Transport semantics
  (Any → Settled, `Output = None`), distinct variants so logs /
  telemetry can distinguish.
- Serde round-trip: R6 property + inline test cover `SearchFsmState`
  + every `Input` + every `Output` variant.
- Runner purity: no tokio spawn, no file read, no network — the
  runner is pure state + data with I/O handed off via `output`.
- `#[allow(dead_code)]` scope: narrow, on `Input`, `Output`,
  `SearchFsm`, `SearchRunner`, `TypedAction` only; each annotated
  with a sunset note pointing at the follow-up App-integration
  sprint.

## Commit hygiene

6 commits on the branch: plan doc, FSM scaffold, runner +
`TypedAction`, property suite, status update, and the Tier 1 fix
commit `0671ff8`. Conventional prefixes throughout. No merge commits.
Each commit leaves `cargo test` green per the pre-commit hook.

## Build gates

- `cargo build` — clean
- `cargo clippy --all-targets -- -D warnings` — clean
- `cargo test` — 878 lib + 862 bin + 8 search_fsm + 9 playback_fsm +
  8 marker_fsm + 26 workflow + others, all pass
- `cargo fmt --check` — clean
- `cargo test --release --test search_fsm` — well under the 60 s
  budget (~120 ms at default `cases=64`)

## Recommendation

**Ship.** All flagged issues resolved in-sprint.
