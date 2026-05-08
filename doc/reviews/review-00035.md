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
