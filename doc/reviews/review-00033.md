# PR #33 — Inline workflow-helper unittest into the cargo job

## Summary

Move the `python -m unittest` step out of the dedicated `Python tests`
job and into the existing `Check & Test` job, between `cargo test` and
`cargo fmt`. Renames `python` to `python3` while there.

After PR #32 ported the template's workflow scripts and tests, this
repo was the only ported sibling whose workflow-helper unittest run
sat in a separate job. The dedicated `python` job is still
load-bearing — it gates the embedding / CLAP / FAISS pytest suite,
which actually needs `pip install pytest numpy hypothesis`. The
workflow-helper unittest run does not, so it doesn't earn its own
job.

### Why now

Cross-repo consistency. Every other ported repo (`agogo`, `mcp-live`,
`stdio`, `stdio-core`, `adat`) puts `python3 -m unittest` inline in
the cargo job. With this PR, riffgrep matches.

### What ships

- `.github/workflows/ci.yml` — net 6-line diff, 3 added in `test`
  job, 3 removed from `python` job.
- `doc/plans/plan-2026-05-08-01.md` — sprint plan with the move
  rationale.

### What does NOT ship

- The `python` job's pytest suite (embeddings) is unchanged.
- No script renames or behavior changes elsewhere.

### Test plan

- [x] `python3 -m unittest` discovery on a fresh local clone runs
      `tests/test_workflow_state.py` (2 tests, ~1.6s).
- [x] `git diff --stat` shows only `.github/workflows/ci.yml` (and
      this PR's plan/review docs).
- [ ] CI's `Check & Test` job picks up the new step and stays green.
- [ ] CI's `Python tests` job remains green with its embedding
      pytest suite intact.
