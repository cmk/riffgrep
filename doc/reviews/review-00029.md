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

<!-- gh-id: 3172515535 -->
### Copilot on [`.claude/commands/watch-pr.md:145`](https://github.com/cmk/riffgrep/pull/29#discussion_r3172515535) (2026-05-01 07:50 UTC)

The all-`ask` path intentionally avoids staging/committing the pull-only `doc/reviews` delta, but Step 1 already modified the review file via `scripts/pull_reviews.py`. As written, this leaves the working tree dirty while Step 0c requires a clean tree on entry, which can cause the next tick to immediately abort/stall. Either (a) restore/stash the pull-only review-doc changes when skipping the commit, or (b) relax the precondition to allow a dirty tree when the only changes are in `doc/reviews/` and explicitly document that exception.
```suggestion
# If every new item was `ask`, do not stage or keep the pull-only
# review-doc delta from Step 1; restore doc/reviews/ so the next tick
# still starts from a clean tree.
if [ "$auto_fix_count" -eq 0 ] && [ "$reply_count" -eq 0 ]; then
    git restore --source=HEAD --worktree -- doc/reviews
```

<!-- gh-id: 3172515557 -->
### Copilot on [`AGENTS.md:152`](https://github.com/cmk/riffgrep/pull/29#discussion_r3172515557) (2026-05-01 07:50 UTC)

This section states that the repo uses Rust’s modern module layout (no `mod.rs`), but earlier in this same file the documented source layout includes `src/engine/mod.rs` and `src/ui/mod.rs`, and the repo currently does use those files. Please clarify the convention (e.g., “prefer modern layout for new top-level modules where feasible” or document why `mod.rs` is still used here) to avoid giving agents contradictory guidance.
```suggestion
- **Prefer Rust's modern module layout for new modules and refactors where
  feasible.** In that layout, the parent module lives one level up and
  there is no `mod.rs` file. This repo still has some existing `mod.rs`
  files, so their presence alone is not a bug. If you introduce, retain,
  or depend on `mod.rs` in a new change, **you must document why**. The
  modern layout looks like this:
```

<!-- gh-id: 4210471302 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-05-01 07:50 UTC](https://github.com/cmk/riffgrep/pull/29#pullrequestreview-4210471302))

## Pull request overview

Ports the multi-agent workflow scaffolding from `template-rust` into `riffgrep`, making `AGENTS.md` the canonical agent instruction source and adding supporting scripts/hooks to enforce the review-round FSM and local quality gates.

**Changes:**
- Adds FSM workflow utilities (`scripts/workflow_state.sh`, `scripts/safe_merge.sh`, `scripts/local_review.sh`) and updates workflow documentation/Claude command docs to match the atomic “one commit per review round” flow.
- Adds/enables a git pre-commit hook layer and tightens the Claude Code pre-tool hook to match the blocking gate.
- Fixes the SQLite integration test so `sqlite_count_mode` actually exercises SQLite mode by using the raw command helper.

### Reviewed changes

Copilot reviewed 17 out of 18 changed files in this pull request and generated 4 comments.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| tests/integration.rs | Fixes `sqlite_count_mode` to use `riffgrep_raw()` so it doesn’t implicitly force `--no-db`. |
| scripts/workflow_state.sh | New script to report best-effort FSM state from local repo conditions. |
| scripts/safe_merge.sh | New guard wrapper around `gh pr merge` to prevent merging with unpushed round commits. |
| scripts/next_pr_number.sh | Updates comments to reflect that predicted PR numbers can drift and require renames. |
| scripts/local_review.sh | New Codex/shell implementation of the `plan_finalized → local_reviewed` transition. |
| scripts/extract_pr_body.sh | Updates docs/behavior notes around extracting PR body from `## Summary` in review files. |
| scripts/check-pii.sh | Makes `.pii-allow` comment/blank-line filtering POSIX-safe via `[[:space:]]`. |
| doc/workflow.md | Updates mermaid diagrams + recovery notes to match the atomic review-round flow and new scripts. |
| doc/reviews/review-00029.md | Adds the PR’s local review artifact capturing findings and resolutions. |
| AGENTS.md | Introduces shared canonical agent instructions, repo conventions, and FSM workflow spec. |
| .githooks/pre-commit | Adds blocking git-side pre-commit gate (fmt, PII scan, tests, clippy). |
| .claude/settings.json | Aligns Claude Code PreToolUse hook to be blocking (matches pre-commit gate). |
| .claude/commands/watch-pr.md | Updates `/watch-pr` semantics to push round commits and reflect new FSM states. |
| .claude/commands/sprint-review.md | Updates `/sprint-review` to reflect FSM transition framing and adds auto-fix triage/reporting. |
| .claude/commands/reply-reviews.md | Updates `/reply-reviews` to produce one atomic round commit (no amend) and documents recovery paths. |
| .claude/commands/pull-reviews.md | Updates guidance to reflect “round commit” atomicity rather than “fix commit + amend”. |
</details>






<!-- gh-id: 3172515567 -->
### Copilot on [`scripts/local_review.sh:61`](https://github.com/cmk/riffgrep/pull/29#discussion_r3172515567) (2026-05-01 07:50 UTC)

`plan_context` and `calibration_context` are computed but never used. This looks like leftover scaffolding from a custom-prompt approach and makes it unclear what context Codex is actually reviewing with. Either remove these variables entirely, or wire them into the Codex invocation once the CLI supports it again.
```suggestion

```

<!-- gh-id: 3172515582 -->
### Copilot on [`scripts/safe_merge.sh:91`](https://github.com/cmk/riffgrep/pull/29#discussion_r3172515582) (2026-05-01 07:50 UTC)

The unpushed-commit guard only checks for a local branch named exactly `head_ref` (`refs/heads/$head_ref`). If the PR branch is checked out locally under a different name (e.g., a local branch that tracks `origin/$head_ref`), this script will treat it as “no local branch” and skip the ahead-of-origin check, potentially allowing a merge that strands unpushed commits. Consider resolving the local ref via the branch(es) whose upstream is `origin/$head_ref` (or checking HEAD when it points at that upstream), rather than assuming the local branch name matches.

<!-- gh-id: 3172593181 -->
#### ↳ cmk ([2026-05-01 08:24 UTC](https://github.com/cmk/riffgrep/pull/29#discussion_r3172593181))

Fixed — AGENTS.md now says to prefer modern module layout for new modules/refactors and explicitly notes that existing mod.rs files in riffgrep are not bugs by themselves.

<!-- gh-id: 3172593194 -->
#### ↳ cmk ([2026-05-01 08:24 UTC](https://github.com/cmk/riffgrep/pull/29#discussion_r3172593194))

Fixed — removed the unused plan_context and calibration_context setup from local_review.sh now that Codex uses the supported base-review invocation.

<!-- gh-id: 3172593291 -->
#### ↳ cmk ([2026-05-01 08:24 UTC](https://github.com/cmk/riffgrep/pull/29#discussion_r3172593291))

Fixed — watch-pr.md now restores doc/reviews after all-ask ticks, so pull-only review doc deltas do not leave the next tick with a dirty tree.

<!-- gh-id: 3172593301 -->
#### ↳ cmk ([2026-05-01 08:24 UTC](https://github.com/cmk/riffgrep/pull/29#discussion_r3172593301))

Fixed — safe_merge.sh now checks both a same-named local branch and any local branches whose upstream is origin/<head_ref>, so differently named tracking branches cannot bypass the unpushed-commit guard.
