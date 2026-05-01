# PR #30 — fix: Sync workflow guard fixes

<!-- gh-id: 3172771689 -->
### Copilot on [`scripts/safe_merge.sh:36`](https://github.com/cmk/riffgrep/pull/30#discussion_r3172771689) (2026-05-01 09:40 UTC)

The `gh`-CLI preflight runs before the `--help` short-circuit, so `scripts/safe_merge.sh --help` fails on machines without `gh` instead of printing usage. Consider moving the `command -v gh` check to after the help block (or allowing help to run without `gh`).

<!-- gh-id: 3172771698 -->
### Copilot on [`scripts/safe_merge.sh:108`](https://github.com/cmk/riffgrep/pull/30#discussion_r3172771698) (2026-05-01 09:40 UTC)

The failure message for resolving `headRefName` drops context: it doesn’t include any forwarded `-R/--repo` args, and when no selector is provided it prints an empty `gh pr view` invocation. Consider including the fully constructed command (or explicitly noting "current branch PR" and the repo) so failures are actionable.

<!-- gh-id: 4210733605 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-05-01 09:40 UTC](https://github.com/cmk/riffgrep/pull/30#pullrequestreview-4210733605))

## Pull request overview

Syncs workflow helper scripts/docs with the current template updates, tightening guardrails around expensive GitHub lookups and improving `safe_merge.sh`’s handling of `gh` repo forwarding.

**Changes:**
- Adds a `gh` CLI preflight, plus `-R/--repo` detection/forwarding, to `scripts/safe_merge.sh`.
- Makes `scripts/workflow_state.sh`’s no-arg `review_path.sh` fallback opt-in via `WORKFLOW_STATE_ALLOW_REVIEW_PATH_FALLBACK=1`.
- Restores `/watch-pr` counter guidance in the Claude command docs.

### Reviewed changes

Copilot reviewed 3 out of 3 changed files in this pull request and generated 3 comments.

| File | Description |
| ---- | ----------- |
| scripts/workflow_state.sh | Gates the potentially expensive no-arg review-file prediction behind an explicit env var. |
| scripts/safe_merge.sh | Adds `gh` preflight + parses/forwards repo selection flags when resolving PR head refs. |
| .claude/commands/watch-pr.md | Documents integer counter tracking used to decide when to commit/push during watch rounds. |





<!-- gh-id: 3172771708 -->
### Copilot on [`scripts/safe_merge.sh:84`](https://github.com/cmk/riffgrep/pull/30#discussion_r3172771708) (2026-05-01 09:40 UTC)

`repo_args` parsing sets `expect_repo_value=true` for `-R/--repo` but never validates that a value was actually provided (e.g., argv ends with `--repo`). With `set -u`, this won’t crash, but it will defer to a less clear `gh` error later. Consider checking `expect_repo_value` after the loop and exiting with a targeted usage error.

<!-- gh-id: 3172821563 -->
#### ↳ cmk ([2026-05-01 10:02 UTC](https://github.com/cmk/riffgrep/pull/30#discussion_r3172821563))

Fixed - the --help block now runs before the gh CLI preflight, so usage can be printed even when gh is absent from PATH.

<!-- gh-id: 3172821683 -->
#### ↳ cmk ([2026-05-01 10:02 UTC](https://github.com/cmk/riffgrep/pull/30#discussion_r3172821683))

Fixed - the failure path now builds and reports the fully constructed gh pr view command, including any forwarded repo-selection args.

<!-- gh-id: 3172821691 -->
#### ↳ cmk ([2026-05-01 10:02 UTC](https://github.com/cmk/riffgrep/pull/30#discussion_r3172821691))

Fixed - repo-flag parsing now exits with a targeted missing-value error when -R/--repo has no value, including when the next token is another flag.
