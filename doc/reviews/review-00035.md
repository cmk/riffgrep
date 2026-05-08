# PR #35 — Salvage deferred FSM plan docs

## Summary

Salvage four plan docs from the dormant `doc/outstanding-plans`
branch onto `main`. The branch was 1 ahead / 50 behind, never had
a PR, and held the only copies of these four sprint plans for
future FSM work:

- `plan-2026-04-19-03.md` — **Plan 07**: Playback FSM, reverse-path
  unification + engine wiring.
- `plan-2026-04-19-04.md` — **Plan 09**: Search FSM, keybinding
  split + app integration (continuation of Plan 08).
- `plan-2026-04-19-05.md` — **Plan 10**: Input-Mode FSM
  (Normal ↔ Insert).
- `plan-2026-04-19-06.md` — **Plan 11**: Serialize/Deserialize
  retrofit on Markers + Playback FSMs.

Once this lands, `doc/outstanding-plans` can be deleted (its
remaining unique content was the four plan docs above plus three
files that have already been removed from `main` by the workflow
port and PR #34).

The plan number naming is preserved as-is — these are the original
filenames the deferred work was tracked under, and renumbering would
muddle reference from any in-flight discussion.

### What ships

- Four `doc/plans/plan-2026-04-19-*.md` files, byte-identical to the
  versions on `doc/outstanding-plans`.

### What does NOT ship

- No code changes, no test changes.
- The four plans remain *deferred* — this PR salvages the docs only.
  Implementation belongs in their own future plan branches.

### Test plan

- [x] `scripts/check_pii.sh` clean.
- [x] `scripts/check_layers.sh` clean (no Rust code touched).
- [ ] CI green (no behavior change expected).

<!-- gh-id: 3207913626 -->
### Copilot on [`doc/plans/plan-2026-04-19-06.md:44`](https://github.com/cmk/riffgrep/pull/35#discussion_r3207913626) (2026-05-08 10:01 UTC)

`marker_fsm::Input` variant count here appears outdated: current `src/engine/marker_fsm.rs` defines 20 `Input` variants (including selection/bank toggles, rep edits, marker display toggle, CSV import/export, and `LoadConfig`). Suggest updating this bullet to match the current API (or avoid stating an exact variant count) so the plan stays accurate as a reference for the serde-derive pass.


<!-- gh-id: 3207913663 -->
### Copilot on [`doc/plans/plan-2026-04-19-04.md:25`](https://github.com/cmk/riffgrep/pull/35#discussion_r3207913663) (2026-05-08 10:01 UTC)

nit: The cleanup note here says `#[allow(dead_code)]` attributes are present on `TypedAction`, but `TypedAction` in `src/engine/search_runner.rs` is already live and does not appear to be `#[allow(dead_code)]`-gated. Suggest removing `TypedAction` from this list (or rephrasing to only cover the items that actually carry the attribute today).


<!-- gh-id: 3207913687 -->
### Copilot on [`doc/plans/plan-2026-04-19-06.md:13`](https://github.com/cmk/riffgrep/pull/35#discussion_r3207913687) (2026-05-08 10:01 UTC)

This plan cites `stdio-core/doc/designs/tui-fsm.md` (e.g. §1). Elsewhere in this repo, the referenced design doc path is `stdio/doc/designs/tui-fsm.md` (see `doc/plans/plan-2026-04-19-01.md` and `doc/reviews/review-00021.md`). Please align the referenced path/repo name (or explicitly note that the design doc moved) to avoid sending readers to a non-existent path in this codebase.


<!-- gh-id: 3207913704 -->
### Copilot on [`doc/plans/plan-2026-04-19-05.md:16`](https://github.com/cmk/riffgrep/pull/35#discussion_r3207913704) (2026-05-08 10:01 UTC)

This plan cites `stdio-core/doc/designs/tui-fsm.md` (§4), but other plans/reviews in this repo reference `stdio/doc/designs/tui-fsm.md` as the design doc location. Please align the reference (or clarify the repository name change) so readers know where to find the doc.


<!-- gh-id: 3207913723 -->
### Copilot on [`doc/plans/plan-2026-04-19-05.md:61`](https://github.com/cmk/riffgrep/pull/35#discussion_r3207913723) (2026-05-08 10:01 UTC)

nit: In this snippet, `Input::QueryChanged(...)` is ambiguous in a codebase with multiple FSM `Input` enums (search/markers/playback/etc.). Consider qualifying it (e.g., `search_fsm::Input::QueryChanged`) or introducing an alias in the snippet so the example is unambiguous.


<!-- gh-id: 4251380562 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-05-08 10:01 UTC](https://github.com/cmk/riffgrep/pull/35#pullrequestreview-4251380562))

## Pull request overview

This PR restores (“salvages”) four previously unmerged FSM sprint plan documents (and a corresponding review record) from a dormant branch into `main`, preserving their original filenames and intent. This fits into the repository’s ongoing design/roadmap documentation under `doc/plans/` and `doc/reviews/`.

**Changes:**
- Add four deferred FSM plan documents to `doc/plans/` (Plans 07, 09, 10, 11).
- Add a new PR review record for PR #35 under `doc/reviews/`.

### Reviewed changes

Copilot reviewed 5 out of 5 changed files in this pull request and generated 6 comments.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| doc/reviews/review-00035.md | Adds the review record documenting the intent/scope of PR #35. |
| doc/plans/plan-2026-04-19-03.md | Adds Plan 07 (playback FSM wiring + reverse-path unification). |
| doc/plans/plan-2026-04-19-04.md | Adds Plan 09 (search FSM keybinding split + App integration). |
| doc/plans/plan-2026-04-19-05.md | Adds Plan 10 (input-mode FSM for Normal/Insert). |
| doc/plans/plan-2026-04-19-06.md | Adds Plan 11 (serde retrofit for markers + playback FSMs). |
</details>






<!-- gh-id: 3207913755 -->
### Copilot on [`doc/plans/plan-2026-04-19-03.md:42`](https://github.com/cmk/riffgrep/pull/35#discussion_r3207913755) (2026-05-08 10:01 UTC)

Several tasks in this plan appear to already be implemented on `main` (e.g., `PlaybackEngine` already owns an `fsm` and dispatches `FsmInput`, and segment playback already models `PlaySegment.reversed` with effective direction via XOR). Consider adding a clear status note at the top (shipped/partially shipped + where) or updating the task list so readers don’t treat already-landed work as pending.

<!-- gh-id: 3207940164 -->
#### ↳ cmk ([2026-05-08 10:06 UTC](https://github.com/cmk/riffgrep/pull/35#discussion_r3207940164))

Added a Status banner up-top noting the marker `Input` enum has grown beyond the variant count quoted, and softened the count claim in the body to direct future readers to verify against `src/engine/marker_fsm.rs` before deriving. Keeping the rest of the plan's text as the salvaged 2026-04-19 snapshot.

<!-- gh-id: 3207940370 -->
#### ↳ cmk ([2026-05-08 10:06 UTC](https://github.com/cmk/riffgrep/pull/35#discussion_r3207940370))

Removed `TypedAction` from the dead-code list and added a parenthetical noting it has since gained live callers in `src/engine/search_runner.rs` so the attribute is already gone. Banner above flags broader codebase drift.

<!-- gh-id: 3207940568 -->
#### ↳ cmk ([2026-05-08 10:06 UTC](https://github.com/cmk/riffgrep/pull/35#discussion_r3207940568))

Fixed: `stdio-core/doc/designs/tui-fsm.md` → `stdio/doc/designs/tui-fsm.md` (matches `plan-2026-04-19-01.md` and `review-00021.md`). Same fix applied to the parallel reference in plan-2026-04-19-05.md.

<!-- gh-id: 3207940841 -->
#### ↳ cmk ([2026-05-08 10:06 UTC](https://github.com/cmk/riffgrep/pull/35#discussion_r3207940841))

Fixed: `stdio-core/doc/designs/tui-fsm.md` → `stdio/doc/designs/tui-fsm.md`. Mirrored in plan-2026-04-19-06.md.

<!-- gh-id: 3207941043 -->
#### ↳ cmk ([2026-05-08 10:06 UTC](https://github.com/cmk/riffgrep/pull/35#discussion_r3207941043))

Qualified to `search_fsm::Input::QueryChanged` so the snippet is unambiguous in a multi-FSM codebase. Good catch.

<!-- gh-id: 3207941250 -->
#### ↳ cmk ([2026-05-08 10:06 UTC](https://github.com/cmk/riffgrep/pull/35#discussion_r3207941250))

Added a Status banner naming the two tasks that have already shipped: `PlaybackEngine` already owns its `fsm` and dispatches `FsmInput`, and `PlaySegment.reversed` already drives effective direction via XOR. Future readers see the call-out before treating any task as pending. Keeping the body of the plan as the original snapshot rather than rewriting it inline — this PR is a salvage, not a re-plan.
