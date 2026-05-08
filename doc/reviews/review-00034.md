# PR #34 — Delete dormant Claude auto-review workflow

## Summary

Delete `.github/workflows/claude-code-review.yml`. The workflow has
been gated by `if: false` since it was added in PR #2 (2026-04-15)
and has not run a single non-skipped job. The disabling comment said
it would be re-enabled once `CLAUDE_CODE_OAUTH_TOKEN` was set, but
that secret has been set since 2026-04-16 — the day after the
workflow was added. Three weeks of dormant skipped runs.

Copilot reviews every PR already; the auto-Claude path is redundant.

### What ships

- Deletion of `.github/workflows/claude-code-review.yml`.
- Plan + review docs.

### What does NOT ship

- `claude.yml` (the mention-triggered `@claude` bot) is untouched
  per user direction. It is still listed as a workflow and still
  responds to `@claude` mentions in issues / PR review comments.
- No code, script, or test changes.

### Test plan

- [x] `git rm` cleanly removes the file; no other workflow files
      reference it.
- [ ] CI runs on this PR no longer include a `Claude Code Review`
      check.
- [ ] `Check & Test` and `Python tests` jobs stay green.
