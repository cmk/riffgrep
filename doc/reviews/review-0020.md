# PR #0000 (pre-PR) — Playback FSM (Plan 06)

## Local review (2026-04-19)

**Branch:** sprint/playback-fsm
**Commits:** 3 (origin/main..HEAD) + 1 review-round fix
**Reviewer:** code-reviewer agent (pre-push local review)

## Summary

The FSM module (`src/engine/playback_fsm.rs`) is well-structured. All
14 inputs are handled exhaustively, transition semantics match both the
plan spec and `PlaybackEngine::restart_program`'s early-return
convention, output generation is correct for the four transport-mutation
inputs, and `PlaybackFsm::consume` silently discards the `Err` arm from
the infallible `StateMachineImpl` correctly. 21 inline unit tests cover
every branch explicitly. The `prop_state_machine!` harness for Q2 and
the baseline `SyncedSutTest` are wired correctly — `SharedInvariants::assert_all`
catches any SUT/reference drift from the first step, giving full
coverage of all 14 inputs across arbitrary sequences. Build, clippy,
fmt, and all 9 integration tests pass.

One must-fix from the agent review was a trivially weak Q5 (no prefix,
dead generators) — fixed in-sprint before push: Q5 now runs a prefix
drawn from `transitions_no_stop_or_program_end` before the
Seek+ConsumeSeek injection, and the stale `#[allow(dead_code)]`
annotations are gone. One follow-up (F2: `ProgramEnded` from `Paused`
without loop) also addressed in-sprint via a unit test. F1 (ConsumeRestart
precondition doc) deferred to Plan 07 where engine wiring will make
the invariant testable.

## Must-fix

None (M1 resolved in-sprint).

## Follow-ups

**F1 — `ConsumeRestart` from `Paused` snaps transport to `Playing`
without a precondition guard**

`Input::ConsumeRestart` unconditionally sets `transport =
Transport::Playing`. A `Restart` dispatched while `Paused` queues
`pending_restart = true` without changing transport. If the mixer
dispatched `ConsumeRestart` before the sink resumes — which the current
operational contract forbids but the FSM does not enforce — the FSM
state would snap to `Playing` while audio stays silent. No test covers
`[Play, Pause, Restart, ConsumeRestart]`; Plan 07's engine wiring should
either add a transport guard or document at the `ConsumeRestart` arm
that it is a mixer-internal signal requiring an active sink.

## Commit hygiene

Four commits on the branch: FSM scaffold, property suite, status-doc
update, and this review-round fix for M1+F2. All carry conventional
commit prefixes (`feat:`, `test:`, `doc:`, `fix:`). Commit messages are
informative and each passes `cargo test` per the pre-commit hook. No
merge commits, linear history, no issues.

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
