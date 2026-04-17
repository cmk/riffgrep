# PR #7 — Require review-NNNN.md to ride along with its PR

<!-- gh-id: 4127127945 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-17 08:09 UTC](https://github.com/cmk/riffgrep/pull/7#pullrequestreview-4127127945))

## Pull request overview

Codifies that `doc/reviews/review-NNNN.md` must be committed on the same PR branch it documents, preventing review files from being orphaned after merge.

**Changes:**
- Add an explicit “review file rides along with the PR” rule to Tier-2 guidance in `CLAUDE.md`.
- Update `/pull-reviews` skill to require committing the review file when new items are appended.
- Update `/reply-reviews` skill to require committing the review file after mirroring replies locally.

### Reviewed changes

Copilot reviewed 3 out of 3 changed files in this pull request and generated 1 comment.

| File | Description |
| ---- | ----------- |
| `CLAUDE.md` | Adds the core rule that `review-NNNN.md` must be committed on the PR branch and refreshed before final push. |
| `.claude/skills/reply-reviews.md` | Requires committing the updated review file after pulling replies back into `review-NNNN.md`. |
| `.claude/skills/pull-reviews.md` | Adds an explicit commit step when `/pull-reviews` appends new review items. |





---

💡 <a href="https://github.com/cmk/riffgrep/new/main?filename=.github/instructions/*.instructions.md" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Add Copilot custom instructions</a> for smarter, more guided reviews. <a href="https://docs.github.com/en/copilot/customizing-copilot/adding-repository-custom-instructions-for-github-copilot" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Learn how to get started</a>.

<!-- gh-id: 3098771853 -->
### Copilot on [`.claude/skills/pull-reviews.md:53`](https://github.com/cmk/riffgrep/pull/7#discussion_r3098771853) (2026-04-17 08:09 UTC)

Step 3 instructs committing the updated review file, but the later "Notes" section still says "Do not commit the review file". This is internally contradictory guidance; update or remove the Notes bullet (or rephrase it to clarify that the skill/script doesn’t auto-commit, but the user should commit when the file changes).
