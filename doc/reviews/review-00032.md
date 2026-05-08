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

<!-- gh-id: 4249320641 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-05-08 03:37 UTC](https://github.com/cmk/riffgrep/pull/32#pullrequestreview-4249320641))

## Pull request overview

Ports the current `template-rust` workflow contract into `riffgrep` by renaming workflow entrypoints, moving expensive Rust gates to pre-push, and adding Python unit tests for workflow-state behavior.

**Changes:**
- Replaces legacy workflow helpers (`review_path.sh`, `extract_pr_body.sh`, etc.) with `pr_report.py` subcommands and updated `pr_*`/`git_*` entrypoints.
- Adds a `pre-push` hook for `cargo test`/`cargo clippy`, while keeping `pre-commit` and Claude pre-tool hooks cheap.
- Introduces `unittest` coverage for `workflow_state.sh` and runs it in the existing Python CI job.

### Reviewed changes

Copilot reviewed 25 out of 26 changed files in this pull request and generated 1 comment.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| tests/test_workflow_state.py | Adds unittest coverage for `workflow_state.sh` behavior in minimal temp git repos. |
| tests/__init__.py | Marks `tests/` as a package for Python test discovery. |
| scripts/workflow_state.sh | Updates review-file resolution (via `pr_report.py` + local inference) and tweaks state classification. |
| scripts/review_path.sh | Removes legacy review-path helper (superseded by `pr_report.py path`). |
| scripts/pr_review.sh | Updates local-review driver to use `pr_report.py`, adds gh preflight, and commits the appended local review. |
| scripts/pr_request.sh | Renames/extends “next PR number” helper with improved gh error capture. |
| scripts/pr_report.py | Expands review ingestion script into a multi-subcommand CLI (`path`, `body`, `reviews`). |
| scripts/pr_reply.py | Renames reply tool and updates shared helper import. |
| scripts/github_client.py | Renames shared gh helper module and adjusts repo-resolution behavior/docs. |
| scripts/git_squash.sh | Renames autosquash wrapper script entrypoint/documentation. |
| scripts/git_merge.sh | Renames safe-merge wrapper references and adds a dirty-working-tree merge refusal. |
| scripts/extract_pr_body.sh | Removes legacy PR-body extractor (superseded by `pr_report.py body`). |
| scripts/check_pii.sh | Fixes self-exclusion path to match the renamed script filename. |
| doc/workflow.md | Updates workflow diagrams and prose to the new command/script names and `.pr-watch` state dir. |
| doc/reviews/review-00032.md | Adds the PR’s review record document. |
| doc/plans/plan-2026-05-07-01.md | Adds the sprint plan documenting the port tasks and verification steps. |
| AGENTS.md | Updates canonical workflow documentation for the new entrypoints, hooks layering, and commands. |
| .gitignore | Renames the watcher state directory from `/.watch-pr/` to `/.pr-watch/`. |
| .github/workflows/ci.yml | Runs `python -m unittest` before the existing pytest suite in the Python CI job. |
| .githooks/pre-push | Adds a pre-push gate for `cargo test` and `cargo clippy`. |
| .githooks/pre-commit | Makes pre-commit checks cheap (fmt + PII scan only) with clearer step logging. |
| .claude/settings.json | Aligns Claude’s commit-time hook with the cheap pre-commit checks. |
| .claude/commands/pr-watch.md | Updates the watcher command playbook to the new names and push-failure marker scheme. |
| .claude/commands/pr-review.md | Updates Tier-1 local review playbook to `pr_*`/`git_*` tooling and new hook split. |
| .claude/commands/pr-report.md | Updates Tier-2 “mirror review items” playbook to `pr_report.py reviews`. |
| .claude/commands/pr-reply.md | Updates Tier-2 reply-round playbook to `pr_reply.py` + `pr_report.py reviews`. |
</details>



<details>
<summary>Comments suppressed due to low confidence (3)</summary>

**scripts/pr_request.sh:33**
* The EXIT trap runs `rm -f` on `repo_err`/`api_err` even when they are still empty strings (e.g., when a repo is passed explicitly). This can emit noisy `rm: cannot remove '': ...` diagnostics on script exit. Consider guarding the removals (only `rm` when the variable is non-empty) or using a trap helper that checks `-n` before `rm -f --`.
**scripts/pr_report.py:160**
* `gh_api()` returns a dict for non-list JSON bodies, but downstream `collect_items()` iterates `gh_api(...)` assuming it is a list of dicts. If GitHub returns an error/wrapped response (rate limit, auth) that parses as a dict, this will crash with a confusing exception. Treat non-list responses as a hard error with a clear diagnostic (as the previous behavior did), or make `collect_items()` explicitly handle the dict case and exit cleanly.
**scripts/github_client.py:52**
* The `resolve_repo()` docstring says it verifies the PR exists in the chosen repo, but when `repo_override` is provided the function returns immediately without any preflight. Either update the docstring to match the behavior, or keep the PR-existence preflight even for `--repo` to preserve the advertised guarantee and produce earlier, more actionable errors on typos.
</details>



<!-- gh-id: 3206107135 -->
### Copilot on [`tests/test_workflow_state.py:26`](https://github.com/cmk/riffgrep/pull/32#discussion_r3206107135) (2026-05-08 03:37 UTC)

`run_state()` unconditionally removes `WORKFLOW_REVIEW_FILE` and `WORKFLOW_STATE_ALLOW_REVIEW_PATH_FALLBACK` from the environment, but `test_clean_pushed_branch_without_gh...` later sets both via `mock.patch.dict(os.environ, ...)`. As written, those env overrides never reach the script, so the patching doesn't test what it appears to. Either drop the env patch in the test, or change `run_state()` to accept explicit env overrides / stop stripping these vars when the test intends to exercise them.

<!-- gh-id: 3206118242 -->
#### ↳ cmk ([2026-05-08 03:41 UTC](https://github.com/cmk/riffgrep/pull/32#discussion_r3206118242))

Fixed: removed the ineffective environment patch from the workflow-state test, so the test now directly reflects run_state()'s default environment-clearing behavior.
