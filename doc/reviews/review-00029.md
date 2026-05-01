# PR #29 — Port multi-agent workflow

## Summary

Port the current multi-agent workflow from `template-rust` into `riffgrep`.

This makes `AGENTS.md` the canonical shared instruction file, leaves `CLAUDE.md` as a compatibility symlink, adds the FSM workflow scripts, and updates the Claude command docs to use the atomic review-round flow. It also adds the git pre-commit hook layer and enables the hook path in this checkout.

The new hook exposed a pre-existing SQLite integration test issue: `sqlite_count_mode` used the helper that always injects `--no-db`, so it was not actually exercising SQLite mode. That test now uses the raw command helper for both indexing and counting.

Validation:
- `bash -n` on workflow scripts and `.githooks/pre-commit`
- `git diff --check`
- `scripts/workflow_state.sh`
- `scripts/safe_merge.sh --help`
- `scripts/local_review.sh --check`
- commit hook: `cargo fmt --all -- --check`, `scripts/check-pii.sh`, `cargo test --workspace`, `cargo clippy --all-targets -- -D warnings`

## Local review (2026-05-01)

**Branch:** codex/agent-workflow-fsm
**Commits:** 2 (origin/main..codex/agent-workflow-fsm)
**Reviewer:** Codex (`codex review --base origin/main`)

---

The workflow additions contain guard/state-machine issues that can misclassify branches or bypass the intended safe-merge check in common invocation shapes. The `/watch-pr` instructions also contradict their all-ask no-op path and can push doc-only review pulls unexpectedly.

Full review comments:

- [P2] Parse the PR selector after leading flags — `scripts/safe_merge.sh:46`
  When callers pass merge flags before the PR selector, which `gh pr merge` accepts (for example `scripts/safe_merge.sh --rebase 17`), this guard treats the selector as absent and validates the current branch's PR instead. If invoked from another clean PR branch, the guard can pass and then forward the original args to merge PR 17 without checking PR 17's local branch for unpushed commits. Parse the first positional selector after flags, or reject flags-before-selector before invoking `gh pr merge`.

- [P2] Inspect the predicted review file before classifying — `scripts/workflow_state.sh:24`
  On the normal pre-push path there is no PR number yet, but TDD step 7 has already created the predicted `doc/reviews/review-NNNNN.md`. Because `review_file` is only set from an existing PR or `WORKFLOW_REVIEW_FILE`, `workflow_state.sh` reports those `plan_finalized`/`local_reviewed` branches as `impl_green`, which can send agents to the wrong FSM transition. Fall back to `scripts/review_path.sh` when no PR exists.

- [P2] Do not stage pull-only docs for all-ask ticks — `.claude/commands/watch-pr.md:141`
  In an all-`ask` `/watch-pr` tick, Step 1 has already appended the new review comments to `doc/reviews`, and this unconditional `git add -A` stages that pull-only doc delta. Step 4 therefore creates a doc commit and Step 5 pushes it, contradicting the later no-commit/no-push path for all-ask activity and causing an unnecessary CI/review round. Gate staging/commit on having posted replies or applied fixes, or explicitly defer the pull-only doc delta.

### Local review fixes

- Fixed `scripts/safe_merge.sh` to reject flags-before-selector invocations so the checked PR and forwarded merge target cannot diverge.
- Fixed `scripts/workflow_state.sh` to inspect the predicted review file before a PR exists, and switched its output from a here-doc to `printf` so read-only review sandboxes can run it.
- Fixed `.claude/commands/watch-pr.md` so all-ask ticks do not stage/push pull-only review-doc deltas.
- Fixed `scripts/local_review.sh` to use the supported `codex review --base origin/main` invocation for Codex CLI 0.125.
