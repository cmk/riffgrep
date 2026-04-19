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

<!-- gh-id: 3106516839 -->
### Copilot on [`src/engine/search_fsm.rs:21`](https://github.com/cmk/riffgrep/pull/21#discussion_r3106516839) (2026-04-19 08:41 UTC)

The module doc claims `Input::SearchCancelled → Output::CancelSearch → runner aborts the JoinHandle`, but `output()` never emits `CancelSearch` for `SearchCancelled` (and the current runner type doesn’t own a JoinHandle). Please correct this mapping so future integrators don’t wire cancel handling to the wrong input/output.
```suggestion
//! - Cleanup (#6) is split across the runner/FSM boundary:
//!   [`Output::CancelSearch`] is the runner-facing cancellation effect,
//!   while [`Input::SearchCancelled`] reports back that cancellation
//!   has already occurred.
```

<!-- gh-id: 3106516852 -->
### Copilot on [`src/engine/search_runner.rs:187`](https://github.com/cmk/riffgrep/pull/21#discussion_r3106516852) (2026-04-19 08:41 UTC)

`apply_similarity_filter` always resets `selected` to 0. The existing TUI behavior (`App::filter_similarity_results` in `src/ui/mod.rs`) preserves the current selection when it still exists and only clamps when out of range. To keep behavior consistent for the Task 4 carve-out, consider clamping `selected` to `results.len()-1` instead of unconditionally resetting it.
```suggestion
        self.selected = if self.results.is_empty() {
            0
        } else {
            self.selected.min(self.results.len() - 1)
        };
```

<!-- gh-id: 3106516858 -->
### Copilot on [`doc/plans/plan-2026-04-19-01.md:47`](https://github.com/cmk/riffgrep/pull/21#discussion_r3106516858) (2026-04-19 08:41 UTC)

This plan references a local absolute path (`/Users/.../stdio/doc/designs/tui-fsm.md`). Please change it to a repo-relative path (e.g., `stdio/doc/designs/tui-fsm.md`) so the doc is portable and works for other contributors.
```suggestion
Per `stdio/doc/designs/tui-fsm.md`, the long-term goal is to embed
riffgrep's TUI as a "form" inside stdio ("Claude Code for studios").
Six design considerations drive this plan:
```

<!-- gh-id: 3106516865 -->
### Copilot on [`doc/plans/plan-2026-04-19-01.md:207`](https://github.com/cmk/riffgrep/pull/21#discussion_r3106516865) (2026-04-19 08:41 UTC)

The runner design described here includes owning a `tokio::JoinHandle` and performing `spawn_search()`/`abort()` directly, but the implemented `SearchRunner` explicitly does **not** perform I/O or manage task handles (it returns effect descriptors instead). Please update this section’s example/field list to match the current `SearchRunner` contract so Task 4 implementers don’t build against the wrong runner responsibilities.
```suggestion
- `similarity_snapshot: Option<Vec<TableRow>>`.
- `results: Vec<TableRow>`, `selected: usize`, `scroll_offset: usize`,
  `total_matches: usize`.
- `columns`, `sort_column`, `sort_ascending` (remain under runner for
  now; may move to their own sub-FSM later).
- no task handles or direct I/O; async work is requested by returning
  effect descriptors for the App/integration layer to execute.

```rust
impl SearchRunner {
    pub fn handle(&mut self, input: Input) -> Option<SearchEffect> {
        let output = self.fsm.consume(input);
        match output {
            Some(Output::SpawnSearch { query }) => {
                Some(SearchEffect::SpawnSearch { query })
            }
            Some(Output::CancelSearch) => {
                Some(SearchEffect::CancelSearch)
            }
            Some(Output::FilterSimilarity { query }) => {
                self.apply_similarity_filter(&query);
                Some(SearchEffect::PublishVisibleResults)
            }
            Some(Output::FireSelection) => {
                Some(SearchEffect::Dispatch(
                    TypedAction::LoadSample(self.selected_path()?),
                ))
            }
            None => None,
        }
    }
    // ... result-append/state-sync wrapper methods ...
```

<!-- gh-id: 4135695540 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-19 08:41 UTC](https://github.com/cmk/riffgrep/pull/21#pullrequestreview-4135695540))

## Pull request overview

This PR scaffolds a new Search/Results/Similarity finite state machine (FSM) plus a non-I/O runner wrapper and a dedicated property-test suite, as the first sub-region of the planned TUI refactor.

**Changes:**
- Adds a pure `rust-fsm`-based `SearchFsm` (`Input`/`Output` + serializable `SearchFsmState`) with inline unit tests.
- Introduces `SearchRunner` to hold non-FSM data (results, selection, similarity snapshot, columns/sort) and synthesize `TypedAction`.
- Adds a new `tests/engine/search_fsm/` property suite (R1–R7) and wires it as a standalone Cargo integration test target.

### Reviewed changes

Copilot reviewed 10 out of 10 changed files in this pull request and generated 8 comments.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| `src/engine/search_fsm.rs` | New search FSM implementation with inputs/outputs, serde state, and unit tests. |
| `src/engine/search_runner.rs` | New runner wrapper holding results/snapshot/selection state and synthesizing typed actions. |
| `src/engine/mod.rs` | Exports the new `search_fsm` and `search_runner` modules. |
| `tests/engine/search_fsm/test.rs` | Property-suite entrypoint + `TestConfig` and `prop_state_machine!` harness wiring. |
| `tests/engine/search_fsm/prop.rs` | Reference model + R1–R7 property tests for the search FSM. |
| `tests/engine/search_fsm/generators.rs` | Proptest generators for `Input`, including mode-toggle-excluding streams. |
| `tests/engine/search_fsm/unit.rs` | Placeholder for future App-level regressions once integration lands. |
| `doc/plans/plan-2026-04-19-01.md` | Plan 08 design/verification doc accompanying the scaffold. |
| `Cargo.toml` | Registers `search_fsm` as a standalone integration test target. |
</details>






<!-- gh-id: 3106516870 -->
### Copilot on [`doc/plans/plan-2026-04-19-01.md:150`](https://github.com/cmk/riffgrep/pull/21#discussion_r3106516870) (2026-04-19 08:41 UTC)

This transition rule says `QueryChanged(q)` in Similarity mode emits `Output::FilterSimilarity` immediately, but the shipped FSM only emits `FilterSimilarity` on `DebounceTick` (matching the current TUI debounce behavior). Please align this plan section with the implemented behavior to prevent miswiring during the App integration sprint.

<!-- gh-id: 3106516873 -->
### Copilot on [`doc/plans/plan-2026-04-19-01.md:256`](https://github.com/cmk/riffgrep/pull/21#discussion_r3106516873) (2026-04-19 08:41 UTC)

R6 is specified as a `bincode` serialization round-trip, but the implemented properties/unit tests in this PR use `serde_json` for round-tripping `SearchFsmState`. Please update the property definition (or the tests) so the plan and the verification suite are consistent about the intended serialization format.

<!-- gh-id: 3106516877 -->
### Copilot on [`src/engine/search_fsm.rs:104`](https://github.com/cmk/riffgrep/pull/21#discussion_r3106516877) (2026-04-19 08:41 UTC)

The `Input::QueryChanged` doc says that in `Mode::Similarity` it triggers an *immediate* `Output::FilterSimilarity`, but the FSM implementation only emits `FilterSimilarity` on `DebounceTick` (and the transition comment below explicitly says it's kept debounced). Please update the doc (or the output logic) so behavior and documentation match.
```suggestion
    /// `debounce_dirty`. The follow-up action remains debounced in
    /// both modes: `DebounceTick` later emits either
    /// `Output::SpawnSearch` (Remote) or `Output::FilterSimilarity`
    /// (Similarity).
```

<!-- gh-id: 3106516880 -->
### Copilot on [`src/engine/search_fsm.rs:19`](https://github.com/cmk/riffgrep/pull/21#discussion_r3106516880) (2026-04-19 08:41 UTC)

The module-level design notes reference `Action::LoadSample(path)`, but this PR introduces `TypedAction::LoadSample(PathBuf)` in `search_runner.rs`. To avoid confusion during Task 4 wiring, align this comment with the actual type name (`TypedAction`) used by the runner.
```suggestion
//!   wrapper state and synthesizes a typed
//!   `TypedAction::LoadSample(PathBuf)` (#5).
```

<!-- gh-id: 3106537458 -->
#### ↳ cmk ([2026-04-19 08:51 UTC](https://github.com/cmk/riffgrep/pull/21#discussion_r3106537458))

Fixed in df2bd29 — module doc now has the correct directional mapping: `Output::CancelSearch` goes FSM→runner (emitted from `EnterSimilarityMode`, honored by the App integration's cancel path), while `Input::SearchCancelled` goes runner→FSM after the cancel has occurred. No mention of JoinHandle — the runner doesn't own one.

<!-- gh-id: 3106537511 -->
#### ↳ cmk ([2026-04-19 08:51 UTC](https://github.com/cmk/riffgrep/pull/21#discussion_r3106537511))

Fixed in df2bd29 — now clamps `selected` to `results.len()-1` when the filter shrank past it, preserves otherwise. Matches App::filter_similarity_results exactly; the old reset-to-0 was a real UX regression for the Task 4 carve-out.

<!-- gh-id: 3106537552 -->
#### ↳ cmk ([2026-04-19 08:51 UTC](https://github.com/cmk/riffgrep/pull/21#discussion_r3106537552))

Fixed in df2bd29 — path is now repo-relative `stdio/doc/designs/tui-fsm.md`.

<!-- gh-id: 3106537613 -->
#### ↳ cmk ([2026-04-19 08:52 UTC](https://github.com/cmk/riffgrep/pull/21#discussion_r3106537613))

Fixed in df2bd29 — Task 2 runner section dropped the `current_search: Option<JoinHandle>` field and the `self.spawn_search()` / `h.abort()` example. New example matches the shipped `dispatch` signature and includes an explicit "No `JoinHandle`, no tokio spawn" statement with the tui-fsm.md §3 citation so Task 4 implementers build against the right contract.

<!-- gh-id: 3106537650 -->
#### ↳ cmk ([2026-04-19 08:52 UTC](https://github.com/cmk/riffgrep/pull/21#discussion_r3106537650))

Fixed in df2bd29 — plan transition rule now says `QueryChanged(q)` in Similarity mode sets `debounce_dirty=true` and the subsequent `DebounceTick` emits `FilterSimilarity`. Matches the shipped behavior and today's TUI debounce.

<!-- gh-id: 3106537693 -->
#### ↳ cmk ([2026-04-19 08:52 UTC](https://github.com/cmk/riffgrep/pull/21#discussion_r3106537693))

Fixed in df2bd29 — R6 now says `serde_json::to_string(state) → from_str` to match what the tests actually exercise. bincode was aspirational; serde_json is what the property and inline tests run.

<!-- gh-id: 3106537753 -->
#### ↳ cmk ([2026-04-19 08:52 UTC](https://github.com/cmk/riffgrep/pull/21#discussion_r3106537753))

Fixed in df2bd29 — `Input::QueryChanged` doc no longer claims an immediate `FilterSimilarity` emission. Both modes debounce through `DebounceTick` per the implementation; doc text now spells that out.

<!-- gh-id: 3106537835 -->
#### ↳ cmk ([2026-04-19 08:52 UTC](https://github.com/cmk/riffgrep/pull/21#discussion_r3106537835))

Fixed in df2bd29 — `Action::LoadSample` → `TypedAction::LoadSample(PathBuf)` with a link to the runner module. Aligned with the actual type shipped in `search_runner.rs`.
