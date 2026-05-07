# PR #32 - Port template workflow contract

## Summary

Ports the current `template-rust` workflow contract into `riffgrep`.

- Renames only workflow entrypoints to the current `pr_*`, `git_*`, and underscore script names, leaving product search/indexing scripts untouched.
- Adds a pre-push hook for the expensive Rust test/clippy gate while keeping the commit-time hook cheap.
- Adds Python workflow-state tests and runs them in the existing Python CI job before the product pytest suite.

Verification:

- `bash -n scripts/*.sh .githooks/pre-commit .githooks/pre-push`
- `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest`
- `scripts/pr_report.py path 1`
- `scripts/workflow_state.sh`
- `scripts/check_pii.sh`
- `cargo fmt --all -- --check`
- `git diff --cached --check`

## Local review (2026-05-07)

**Branch:** plan/2026-05-07-01
**Commits:** 3 (origin/main..plan/2026-05-07-01)
**Reviewer:** Codex (`codex review --base origin/main`)

---

The workflow port leaves authoritative command documentation pointing at removed commands/files, and the Claude review command no longer loads the existing calibration examples. These issues would break or degrade the repo's review FSM even though the script renames mostly landed.

Full review comments:

- [P2] Keep local-review calibration path valid — .claude/commands/pr-review.md:94-94
  On this repo the calibration file is still `doc/reviews/review-calibration.md` (there is no `doc/reviews/calibration.md`), so `/pr-review` will always skip the few-shot examples after this rename. That removes the repository-specific review calibration from the Tier 1 workflow; either keep the old path here or rename the file in the same change.

- [P2] Update the Tier 2 pull command name — AGENTS.md:263-265
  The command files are renamed to `.claude/commands/pr-report.md`, but this Tier 2 section still tells agents to run `/pull-reviews <N>` for the `items_pulled` step and repeats that stale primitive below. In a checkout with this patch, `/pull-reviews` no longer exists, so agents following the authoritative AGENTS.md will fail before mirroring GitHub review activity; update the remaining mentions to `/pr-report <N>`.

- [P3] Remove the nonexistent layer-check hook reference — .claude/commands/pr-review.md:303-304
  These playbooks now say the pre-commit hook runs `scripts/check_layers.sh`, but riffgrep does not have that script and `.githooks/pre-commit` only invokes fmt plus `scripts/check_pii.sh`. When a reviewer follows `/pr-review`/`/pr-reply`/`/pr-watch`, the failure-recovery guidance points at a check that cannot run in this repo; drop the layer-check reference or add the script.
