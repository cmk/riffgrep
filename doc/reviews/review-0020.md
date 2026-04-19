# PR #20 — Playback FSM (Plan 06)

## Local review (2026-04-19)

**Branch:** sprint/playback-fsm
**Reviewer:** code-reviewer agent (pre-push local review)

## Summary

The FSM module (`src/engine/playback_fsm.rs`) is well-structured. All
14 inputs are handled exhaustively, transition semantics match both the
plan spec and `PlaybackEngine::restart_program`'s early-return
convention, output generation is correct for the four transport-mutation
inputs, and `PlaybackFsm::consume` silently discards the `Err` arm from
the infallible `StateMachineImpl` correctly. 24 inline unit tests cover
every branch explicitly (baseline transitions, Stop-clears-pending,
Q7 no-op-from-Stopped, program-end + loop interactions, and the
round-2 ConsumeRestart-guard regressions). The `prop_state_machine!` harness for Q2 and
the baseline `SyncedSutTest` are wired correctly — `SharedInvariants::assert_all`
catches any SUT/reference drift from the first step, giving full
coverage of all 14 inputs across arbitrary sequences. Build, clippy,
fmt, and all 9 integration tests pass.

One must-fix from the agent review was a trivially weak Q5 (no prefix,
dead generators) — fixed in-sprint before push: Q5 now runs a prefix
drawn from `transitions_no_stop_or_program_end` before the
Seek+ConsumeSeek injection, and the stale broad `#[allow(dead_code)]`
annotations were narrowed to targeted allowances on `Input`,
`MixerCommand`, and `PlaybackFsm` (tracked to Plan 07 engine wiring).
F2 (`ProgramEnded` from `Paused` without loop) also addressed in-sprint
via a unit test. F1 (ConsumeRestart precondition) was initially deferred
but the round-2 review round resolved it in-tree — see below.

## Must-fix

None (M1 resolved in-sprint).

## Follow-ups

None open. (F1 above was resolved in the round-2 GitHub review: the
`ConsumeRestart` arm now guards on `pending_restart && transport !=
Stopped` and silently no-ops otherwise, with two new unit tests
covering the spurious-dispatch cases. F2 shipped inline in the FSM
tests.)

## Commit hygiene

Commits on the branch (Tier 1 + three GitHub review rounds): FSM
scaffold, property suite, status-doc update, the review-round fix for
M1+F2, a round-2 fix for ProgramEnded clearing / ConsumeRestart guard /
TestConfig::max_steps replacement / CI fmt repair, and a round-3
doc-sync pass. All carry conventional commit prefixes (`feat:`,
`test:`, `doc:`, `fix:`). Each passes `cargo test` per the pre-commit
hook. No merge commits, linear history, no issues.

## Build gates

- `cargo build` — clean
- `cargo clippy --all-targets -- -D warnings` — clean
- `cargo test` — all pass (842 lib/bin + 9 playback_fsm integration +
  other binaries)
- `cargo fmt --check` — clean
- `cargo test --release --test playback_fsm` — well under the 60 s
  budget

## Recommendation

**Ship.** M1 resolved, F2 resolved. F1 is a legitimate correctness note
for Plan 07's engine wiring (the sprint where `ConsumeRestart` actually
gets dispatched by live code).

<!-- gh-id: 3106429612 -->
### Copilot on [`src/engine/playback_fsm.rs:204`](https://github.com/cmk/riffgrep/pull/20#discussion_r3106429612) (2026-04-19 07:15 UTC)

`ProgramEnded` without loop transitions to `Stopped` but leaves `pending_restart` unchanged. If `pending_restart` was already `true` (e.g., user hit Restart near the end), this can recreate the same latent-footgun Q7 is trying to prevent (a queued restart while stopped that could unexpectedly fire on a later Play). Consider explicitly clearing `pending_restart` when `ProgramEnded` causes `transport = Stopped`, and add a unit/property assertion covering `pending_restart = true` entering this branch.

<!-- gh-id: 3106429620 -->
### Copilot on [`src/engine/playback_fsm.rs:212`](https://github.com/cmk/riffgrep/pull/20#discussion_r3106429620) (2026-04-19 07:15 UTC)

`ConsumeRestart` unconditionally snaps `transport` to `Playing` even if `pending_restart` is not set (or if the sink is paused/stopped). Because `ConsumeRestart` emits no `MixerCommand`, this can put the UI-observable state into `Playing` while the audio path remains silent/idle. Recommendation: gate this transition on `state.pending_restart` (and/or on a transport precondition), and/or add a debug assertion + documentation clarifying the required mixer-side preconditions so accidental/early dispatch can’t desync UI state from actual playback.
```suggestion
                debug_assert!(
                    state.pending_restart
                        && matches!(state.transport, Transport::Playing),
                    "ConsumeRestart requires a queued restart while already Playing"
                );
                if state.pending_restart && matches!(state.transport, Transport::Playing) {
                    next.pending_restart = false;
                    // Mixer has consumed the queued restart while audio is
                    // already flowing, so we can safely confirm the Playing
                    // transport state.
                    next.transport = Transport::Playing;
                }
```

<!-- gh-id: 3106429632 -->
### Copilot on [`tests/engine/playback_fsm/test.rs:67`](https://github.com/cmk/riffgrep/pull/20#discussion_r3106429632) (2026-04-19 07:15 UTC)

`TestConfig::max_steps` is documented as controlling the maximum transitions per case, but the `prop_state_machine!` invocations hardcode `sequential 1..32`, so `max_steps` currently has no effect. Either wire `max_steps` into the harness (e.g., by making the step range derive from a single constant used in both places), or remove the field/docs to avoid a misleading configuration knob.

<!-- gh-id: 4135609342 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-19 07:16 UTC](https://github.com/cmk/riffgrep/pull/20#pullrequestreview-4135609342))

## Pull request overview

> [!NOTE]
> Copilot was unable to run its full agentic suite in this review.

Introduces a formal playback finite state machine (FSM) to model UI-observable transport state plus loop/reverse flags and pending seek/restart intents, along with a property-based test suite validating Q1–Q8 behaviors.

**Changes:**
- Added `PlaybackFsm` / `PlaybackMachine` (`rust-fsm`) with `Input` events and `MixerCommand` outputs for transport-affecting actions.
- Added `proptest-state-machine` harness + generators implementing the Q-series properties and regression seed persistence.
- Exported the new engine module and registered a dedicated `playback_fsm` integration test target.

### Reviewed changes

Copilot reviewed 10 out of 10 changed files in this pull request and generated 3 comments.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| `src/engine/playback_fsm.rs` | New playback FSM implementation + inline unit tests for core behaviors |
| `src/engine/mod.rs` | Exposes `playback_fsm` module from `engine` |
| `tests/engine/playback_fsm/test.rs` | Property-suite entrypoint + proptest config wiring |
| `tests/engine/playback_fsm/prop.rs` | Reference model + Q1–Q8 properties and state-machine harnesses |
| `tests/engine/playback_fsm/generators.rs` | Input generators (including restricted prefix generator for Q5) |
| `tests/engine/playback_fsm/unit.rs` | Placeholder unit-test module for future Plan 07 regressions |
| `tests/proptest-regressions/playback_fsm/test.txt` | Saved proptest failure seeds to replay regressions |
| `Cargo.toml` | Registers `playback_fsm` as an integration test target |
| `doc/reviews/review-0020.md` | Captures local review notes, must-fix/follow-ups, and gates |
| `doc/plans/plan-2026-04-18-04.md` | Plan doc describing shipped scope and Plan 07 deferrals |
</details>






<!-- gh-id: 3106435751 -->
#### ↳ cmk ([2026-04-19 07:22 UTC](https://github.com/cmk/riffgrep/pull/20#discussion_r3106435751))

Fixed in 5553c9d — ProgramEnded (no-loop branch) now clears pending_restart alongside setting transport = Stopped. New unit test `program_ended_without_loop_clears_pending_restart` covers the (pending_restart=true, transport=Playing) entry case.

<!-- gh-id: 3106435774 -->
#### ↳ cmk ([2026-04-19 07:22 UTC](https://github.com/cmk/riffgrep/pull/20#discussion_r3106435774))

Fixed in 5553c9d — ConsumeRestart now guards on both `pending_restart == true` and `transport != Stopped`; dropped the `debug_assert` in favor of a silent no-op so the property suite can dispatch the input freely (the guard is the real invariant enforcement). Doc comment updated. Two new unit tests: `consume_restart_is_noop_when_no_pending` and `consume_restart_is_noop_when_stopped`.

<!-- gh-id: 3106435821 -->
#### ↳ cmk ([2026-04-19 07:22 UTC](https://github.com/cmk/riffgrep/pull/20#discussion_r3106435821))

Fixed in 5553c9d — dropped the unused `TestConfig::max_steps` field and introduced a `pub const MAX_STEPS: usize = 32` referenced by both the `prop_state_machine!` sequential ranges and the doc, so raising it now actually widens every harness's transitions-per-case in lockstep.

<!-- gh-id: 3106440607 -->
### Copilot on [`src/engine/playback_fsm.rs:11`](https://github.com/cmk/riffgrep/pull/20#discussion_r3106440607) (2026-04-19 07:27 UTC)

Module-level docs say the FSM is already the single source of truth and that `PlaybackEngine`/TUI dispatch through it, but this PR explicitly defers engine wiring (and the file itself uses `#[allow(dead_code)]`). Please reword this to reflect the current reality (e.g., “will become the source of truth once Plan 07 wires it into PlaybackEngine”), so readers aren’t misled about what codepaths are actually active today.
```suggestion
//! This FSM is intended to become the single source of truth for
//! UI-observable transitions once Plan 07 wires it into
//! [`PlaybackEngine`](crate::engine::playback::PlaybackEngine) and the TUI
//! action handlers. In the current PR, it models those transitions while
//! the existing atomics remain the active mixer-thread interface.
```

<!-- gh-id: 3106440611 -->
### Copilot on [`src/engine/playback_fsm.rs:73`](https://github.com/cmk/riffgrep/pull/20#discussion_r3106440611) (2026-04-19 07:27 UTC)

`Input::Play` is documented as “resuming from pause uses `Input::Resume`”, but the actual transition/output logic treats `Play` from `Paused` as a resume (and there’s an inline unit test asserting that behavior). Please update the doc comment to match the implemented semantics (or, if the intent is to forbid `Play` from `Paused`, adjust the transition/output + tests accordingly).
```suggestion
    /// program) is `Stop` followed by `Play`; when already `Paused`,
    /// `Play` also resumes playback. [`Input::Resume`] is the explicit
    /// paused-to-playing input.
```

<!-- gh-id: 3106440614 -->
### Copilot on [`doc/reviews/review-0020.md:30`](https://github.com/cmk/riffgrep/pull/20#discussion_r3106440614) (2026-04-19 07:27 UTC)

This summary says “the stale `#[allow(dead_code)]` annotations are gone”, but the shipped `src/engine/playback_fsm.rs` still contains narrow `#[allow(dead_code)]` on `Input`/`MixerCommand`/`PlaybackFsm` (as also noted in the plan). Please update this sentence so the review record matches what actually landed.
```suggestion
Seek+ConsumeSeek injection, and the stale broad `#[allow(dead_code)]`
annotations were narrowed to targeted allowances on `Input`,
`MixerCommand`, and `PlaybackFsm`. One follow-up (F2:
`ProgramEnded` from `Paused` without loop) also addressed in-sprint
via a unit test. F1 (ConsumeRestart precondition doc) deferred to
Plan 07 where engine wiring will make the invariant testable.
```

<!-- gh-id: 4135617719 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-19 07:27 UTC](https://github.com/cmk/riffgrep/pull/20#pullrequestreview-4135617719))

## Pull request overview

Copilot reviewed 10 out of 10 changed files in this pull request and generated 6 comments.





<!-- gh-id: 3106440619 -->
### Copilot on [`doc/plans/plan-2026-04-18-04.md:154`](https://github.com/cmk/riffgrep/pull/20#discussion_r3106440619) (2026-04-19 07:27 UTC)

The plan’s test-suite layout bullet says `test.rs` includes `TestConfig (cases, max_steps, env-var override)`, but the shipped `tests/engine/playback_fsm/test.rs` removes `max_steps` in favor of a `MAX_STEPS` constant used by the `prop_state_machine!` ranges. Please update this bullet to match the implementation.
```suggestion
- `test.rs` — entrypoint + `TestConfig` (cases, env-var override)
  and a `MAX_STEPS` constant used by the `prop_state_machine!`
  ranges. Default cases=64.
```

<!-- gh-id: 3106440624 -->
### Copilot on [`doc/plans/plan-2026-04-18-04.md:183`](https://github.com/cmk/riffgrep/pull/20#discussion_r3106440624) (2026-04-19 07:27 UTC)

This section still describes `TestConfig` as `struct TestConfig { cases, max_steps, verbose }` with `max_steps=32`, but the current code instead defines `pub const MAX_STEPS: usize = 32` and no `max_steps` field. Please update the plan text to document `MAX_STEPS` (and/or reintroduce `max_steps` if that configurability is desired).
```suggestion
`struct TestConfig { cases: u32, verbose: bool }`
in `tests/engine/playback_fsm/test.rs`, with
`pub const MAX_STEPS: usize = 32` defining the fixed step bound.
Default `cases=64`. Read env `RIFFGREP_PROPTEST_CASES` if set.
```

<!-- gh-id: 3106440626 -->
### Copilot on [`doc/reviews/review-0020.md:45`](https://github.com/cmk/riffgrep/pull/20#discussion_r3106440626) (2026-04-19 07:27 UTC)

The F1 follow-up states `Input::ConsumeRestart` “unconditionally sets transport=Playing” and that “the FSM does not enforce” a precondition, but the current `src/engine/playback_fsm.rs` adds a guard (`pending_restart && transport != Stopped`) and unit tests for spurious ConsumeRestart. Please update/remove this follow-up so the review record matches the final code in this PR.

<!-- gh-id: 3106447184 -->
#### ↳ cmk ([2026-04-19 07:34 UTC](https://github.com/cmk/riffgrep/pull/20#discussion_r3106447184))

Fixed in 9e17ed2 — module doc reworded to "intended to become the single source of truth once Plan 07 wires it into PlaybackEngine", with a note on the narrow allow(dead_code) attributes still in place until that wiring lands.

<!-- gh-id: 3106447219 -->
#### ↳ cmk ([2026-04-19 07:34 UTC](https://github.com/cmk/riffgrep/pull/20#discussion_r3106447219))

Fixed in 9e17ed2 — `Input::Play` doc now matches the transition + output arms: Play from Paused resumes playback and emits `MixerCommand::Resume`, and `Input::Resume` is the explicit Paused→Playing alias.

<!-- gh-id: 3106447260 -->
#### ↳ cmk ([2026-04-19 07:34 UTC](https://github.com/cmk/riffgrep/pull/20#discussion_r3106447260))

Fixed in 9e17ed2 — summary corrected to "narrowed to targeted allowances on Input, MixerCommand, PlaybackFsm" (those narrow attrs ride until Plan 07 consumes the types). Also dropped the now-stale F1 follow-up from the same file since the round-2 review resolved it in-tree.

<!-- gh-id: 3106447310 -->
#### ↳ cmk ([2026-04-19 07:34 UTC](https://github.com/cmk/riffgrep/pull/20#discussion_r3106447310))

Fixed in 9e17ed2 alongside the Task 6 section update.

<!-- gh-id: 3106447344 -->
#### ↳ cmk ([2026-04-19 07:34 UTC](https://github.com/cmk/riffgrep/pull/20#discussion_r3106447344))

Fixed in 9e17ed2 — Task 6 section now describes `struct TestConfig { cases, verbose }` plus `pub const MAX_STEPS: usize = 32` and notes the rationale (prop_state_machine! requires a literal/const range, so the step bound can't be a runtime struct field).

<!-- gh-id: 3106447390 -->
#### ↳ cmk ([2026-04-19 07:34 UTC](https://github.com/cmk/riffgrep/pull/20#discussion_r3106447390))

Fixed in 9e17ed2 — removed the stale F1 follow-up (and updated the summary that referenced it). Follow-ups section now reads "None open" since the round-2 fix shipped the guard + tests.

<!-- gh-id: 3106452038 -->
### Copilot on [`doc/plans/plan-2026-04-18-04.md:209`](https://github.com/cmk/riffgrep/pull/20#discussion_r3106452038) (2026-04-19 07:38 UTC)

In the Verification table, the Q2 invariant is written as `resume ∘ pause ≡ id`, but the plan (and the actual tests) define Q2 as `pause ∘ resume ≡ id` when transport is `Playing`. Updating this row would prevent a reader from implementing/checking the inverse order by mistake.
```suggestion
| Q2 Pause/Resume inverse   | `tests/engine/playback_fsm/prop.rs`   | `pause ∘ resume ≡ id` when playing                     |
```

<!-- gh-id: 4135626767 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-19 07:38 UTC](https://github.com/cmk/riffgrep/pull/20#pullrequestreview-4135626767))

## Pull request overview

Copilot reviewed 10 out of 10 changed files in this pull request and generated 3 comments.





<!-- gh-id: 3106452056 -->
### Copilot on [`src/engine/playback_fsm.rs:233`](https://github.com/cmk/riffgrep/pull/20#discussion_r3106452056) (2026-04-19 07:38 UTC)

`Input::ConsumeRestart` currently snaps `transport` to `Playing` whenever `pending_restart` is set and `transport != Stopped`, which includes `Paused`. In the current engine, pause uses `sink.pause()` (see `src/engine/playback.rs:599-621`), so the audio path is silent and the mixer isn’t producing frames while paused; a spurious `ConsumeRestart` during `Paused` would therefore desync UI state to `Playing` without any `MixerCommand::Resume`. Consider tightening the guard to only apply when `transport == Playing` (and update the doc comment accordingly), so `ConsumeRestart` can’t resurrect `Playing` while paused/silent.
```suggestion
                // `pending_restart` must be set and the transport must
                // already be Playing (the mixer is producing frames). A
                // spurious dispatch is a no-op rather than a panic so
                // property tests can exercise it freely; the guard
                // protects against the bug-class where ConsumeRestart
                // accidentally snaps transport to Playing while the
                // audio path is silent.
                if state.pending_restart && matches!(state.transport, Transport::Playing) {
```

<!-- gh-id: 3106452062 -->
### Copilot on [`tests/engine/playback_fsm/prop.rs:219`](https://github.com/cmk/riffgrep/pull/20#discussion_r3106452062) (2026-04-19 07:38 UTC)

The Q2 header comment says `Resume ∘ Pause ≡ id`, but the property and harness below are explicitly about `Pause` then `Resume` being a no-op when starting from `Playing`. Swapping the composition order in this comment would avoid misdocumenting the property being tested.
```suggestion
// Q2: Pause ∘ Resume ≡ id when transport = Playing
```

<!-- gh-id: 3106456852 -->
#### ↳ cmk ([2026-04-19 07:43 UTC](https://github.com/cmk/riffgrep/pull/20#discussion_r3106456852))

Fixed in 080270b — Q2 row now reads `pause ∘ resume ≡ id` when playing, matching the property and the prop.rs section header swap in the same commit.

<!-- gh-id: 3106456895 -->
#### ↳ cmk ([2026-04-19 07:43 UTC](https://github.com/cmk/riffgrep/pull/20#discussion_r3106456895))

Fixed in 080270b — tightened the guard to `pending_restart && transport == Playing`. Paused is now also a no-op (Paused + pending_restart stays queued for the eventual Resume). Also dropped the now-redundant `next.transport = Transport::Playing` since the guard enforces it. New unit test `consume_restart_is_noop_when_paused` covers the [Play, Pause, Restart, ConsumeRestart] sequence explicitly.

<!-- gh-id: 3106456914 -->
#### ↳ cmk ([2026-04-19 07:43 UTC](https://github.com/cmk/riffgrep/pull/20#discussion_r3106456914))

Fixed in 080270b — Q2 section header in prop.rs now reads `Pause ∘ Resume ≡ id` (matching the harness and the plan table row fixed alongside).
