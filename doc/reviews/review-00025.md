# PR #25 — Sync review-file workflow from template-rust

## Summary

Port two pieces of review-workflow infrastructure that had landed in
`template-rust` but never made it here: the `review_path.sh` helper and
the `review-calibration.md` few-shot examples.

**`scripts/review_path.sh`** becomes the single source of truth for the
`doc/reviews/review-NNNNN.md` naming convention. Before this change,
every caller that needed the path had to pair `scripts/next_pr_number.sh`
with its own `printf '%05d'` and compose the filename by hand. That left
three footguns in play:

- **Forgotten padding.** An agent skimming CLAUDE.md could create
  `review-17.md` instead of `review-00017.md`, which `extract_pr_body.sh`
  would then fail to find.
- **Bash's octal trap.** `printf '%05d' 08` errors with "invalid octal
  number"; any ad-hoc call site that didn't wrap input in `$((10#$n))`
  would break once PR numbers crossed `08`/`09`.
- **Drift between prose and scripts.** The naming convention was stated
  in CLAUDE.md, restated in `.claude/commands/sprint-review.md`, and
  re-implemented in every caller — three places to keep in sync.

Callers retrofitted:

- `.claude/commands/sprint-review.md` — replaces the "resolve N + printf"
  block with `scripts/review_path.sh "$(gh pr view --json number --jq .number)"`
  (PR-exists case) or bare `scripts/review_path.sh` (pre-push case).
- `CLAUDE.md` — three prose spots updated (Tier 1 description, drift
  paragraph, TDD step 7) to reference `review_path.sh` instead of
  documenting the padding rule inline.

`extract_pr_body.sh` and `pull_reviews.py` continue to compose inline
— they each receive `N` as an argument and pad it once at a single site
(Python's `:05d` has no octal hazard), so routing through a shell
helper would add a subshell with no safety benefit. This matches
template-rust.

**`doc/reviews/review-calibration.md`** is a verbatim copy of
template-rust's file: 8 real-world review comments demonstrating the
"cite the contract, show the violation, name the consequence" style.
`/sprint-review` already checks for this file and splices it into the
reviewer agent's prompt as few-shot examples when present, falling back
to its built-in guidance when absent. The file is purely additive — no
behavior change on absence — but with it the local reviewer agent now
gets the same calibration as template-rust's.

## Test plan

- [x] `scripts/review_path.sh 17` → `doc/reviews/review-00017.md`
- [x] `scripts/review_path.sh 00017` → `doc/reviews/review-00017.md`
- [x] `scripts/review_path.sh 8` → `doc/reviews/review-00008.md` (octal-trap defused)
- [x] `scripts/review_path.sh` (no arg) → predicts next PR number via `next_pr_number.sh`
- [ ] Next `/sprint-review` invocation picks up `review-calibration.md` and includes it in the reviewer prompt
