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
