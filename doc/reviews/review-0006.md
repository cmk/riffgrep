# PR #6 — Add fixup/autosquash convention for CI-repair commits

<!-- gh-id: 3098739303 -->
### Copilot on [`.claude/skills/sprint-review.md:31`](https://github.com/cmk/riffgrep/pull/6#discussion_r3098739303) (2026-04-17 08:02 UTC)

The grep pattern uses `\+` in a basic regex, which isn’t reliably portable across grep implementations. Use `grep -E '^[0-9a-f]+ fixup!'` (and consider disabling git color, e.g. `git -c color.ui=never log ...`, so the match isn’t broken by user config).
```suggestion
git -c color.ui=never log --oneline origin/main..HEAD | grep -E '^[0-9a-f]+ fixup!' || true
```

<!-- gh-id: 3098739329 -->
### Copilot on [`scripts/autosquash.sh:22`](https://github.com/cmk/riffgrep/pull/6#discussion_r3098739329) (2026-04-17 08:02 UTC)

`base` is configurable, but the script always runs `git fetch ... origin`. If a caller passes a different upstream (e.g. `upstream/main`), the fetch won’t update that ref and the rebase may use a stale base. Consider either (a) fetching the remote inferred from `base` (when it looks like `<remote>/<branch>`), or (b) dropping the `base` argument and hard-coding `origin/main` consistently, or (c) fetching the specific ref you’re rebasing onto (e.g. `git fetch --quiet <remote> <branch>`).
```suggestion
if [[ "$base" == */* ]]; then
  remote="${base%%/*}"
  branch="${base#*/}"
  git fetch --quiet "$remote" "$branch"
fi
```

<!-- gh-id: 4127094124 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-17 08:02 UTC](https://github.com/cmk/riffgrep/pull/6#pullrequestreview-4127094124))

## Pull request overview

Codifies a workflow convention to keep `main`’s linear history free of “CI repair” commits by requiring CI-repair commits to be `--fixup`s and providing a helper to autosquash them before review/push.

**Changes:**
- Add `scripts/autosquash.sh` to perform a non-interactive `rebase -i --autosquash` with a dirty-tree guard.
- Document the “CI-repair commits must be fixups” convention in `CLAUDE.md`.
- Update `/sprint-review` guidance to detect pending fixups and run autosquash before proceeding.

### Reviewed changes

Copilot reviewed 3 out of 3 changed files in this pull request and generated 3 comments.

| File | Description |
| ---- | ----------- |
| scripts/autosquash.sh | Adds a helper script to autosquash `fixup!` commits onto `origin/main` (or a provided base). |
| CLAUDE.md | Documents the fixup/autosquash convention for CI-repair commits. |
| .claude/skills/sprint-review.md | Adds a Step 0 pre-review check for fixups and instructs running the autosquash script. |





---

💡 <a href="https://github.com/cmk/riffgrep/new/main?filename=.github/instructions/*.instructions.md" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Add Copilot custom instructions</a> for smarter, more guided reviews. <a href="https://docs.github.com/en/copilot/customizing-copilot/adding-repository-custom-instructions-for-github-copilot" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Learn how to get started</a>.

<!-- gh-id: 3098739345 -->
### Copilot on [`scripts/autosquash.sh:20`](https://github.com/cmk/riffgrep/pull/6#discussion_r3098739345) (2026-04-17 08:02 UTC)

The dirty-tree check relies on `git status`, but if the script is run outside a git worktree (or in a repo without an `origin` remote) it will fail with git’s default error output under `set -e`, which is less actionable than a targeted message. Consider adding an early `git rev-parse --is-inside-work-tree` check (and optionally validating that the expected remote/ref exists) to emit a clear `error:` message before proceeding.

<!-- gh-id: 3098772464 -->
#### ↳ cmk ([2026-04-17 08:09 UTC](https://github.com/cmk/riffgrep/pull/6#discussion_r3098772464))

Fixed in 1a31a1b — switched to `grep -E '^[0-9a-f]+ fixup!'` (portable ERE) and prefixed with `git -c color.ui=never` so user color config can't break the match.

<!-- gh-id: 3098772684 -->
#### ↳ cmk ([2026-04-17 08:09 UTC](https://github.com/cmk/riffgrep/pull/6#discussion_r3098772684))

Fixed in 1a31a1b — when `base` matches `<remote>/<branch>`, the script now parses out the remote and fetches that specific ref. Callers passing `upstream/main` or similar get a fresh base.

<!-- gh-id: 3098772880 -->
#### ↳ cmk ([2026-04-17 08:09 UTC](https://github.com/cmk/riffgrep/pull/6#discussion_r3098772880))

Fixed in 1a31a1b — added `git rev-parse --is-inside-work-tree` as the very first check, so running the script outside a worktree produces a one-line `error: not inside a git worktree` instead of git's default stderr under `set -e`.
