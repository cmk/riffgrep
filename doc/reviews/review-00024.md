# PR #24 — Port template-rust workflow updates (2026-04-22)

<!-- gh-id: 3121975662 -->
### Copilot on [`scripts/extract_pr_body.sh:13`](https://github.com/cmk/riffgrep/pull/24#discussion_r3121975662) (2026-04-22 06:28 UTC)

The header comment says the extracted content is printed “verbatim”, but the implementation captures awk output via command substitution, which strips trailing newlines. This can subtly change the PR body (e.g., collapsing multiple trailing blank lines). Either adjust the wording to describe the newline normalization, or change the implementation to stream the section without command substitution if true verbatim output is required.
```suggestion
# next `## ` heading (exclusive), and written to stdout. Trailing
# newlines are normalized by the shell's command substitution.
```

<!-- gh-id: 3121975715 -->
### Copilot on [`CLAUDE.md:168`](https://github.com/cmk/riffgrep/pull/24#discussion_r3121975715) (2026-04-22 06:28 UTC)

The text implies `NNNNN` comes directly from `scripts/next_pr_number.sh`, but that script prints an unpadded integer (e.g. `17`) while review filenames are zero-padded to 5 digits (e.g. `review-00017.md`). As written, this can mislead someone into creating `review-17.md`, which `scripts/extract_pr_body.sh` (and other tooling) will not find. Consider explicitly spelling out the padding rule (e.g. `review-{N:05d}.md`) and using `N` for the numeric PR number throughout.
```suggestion
Step 7 of the TDD workflow creates `doc/reviews/review-{N:05d}.md` with
the sprint's PR description under a `## Summary` heading. `N` is the
numeric PR number from `scripts/next_pr_number.sh`, which queries the
repo's highest existing issue/PR number via `gh api` and adds one
(GitHub shares its numbering sequence between issues and PRs). The
filename always zero-pads `N` to 5 digits, so PR 17 uses
`doc/reviews/review-00017.md`. The `## Summary` section is the single
source of truth for the PR body: open the PR with
`gh pr create --body-file <(scripts/extract_pr_body.sh N)` so the
```

<!-- gh-id: 3121975740 -->
### Copilot on [`CLAUDE.md:174`](https://github.com/cmk/riffgrep/pull/24#discussion_r3121975740) (2026-04-22 06:28 UTC)

This section states the review file “is born with its final name and never needs to be renamed”, but `scripts/next_pr_number.sh` is only a prediction. If another issue/PR is opened between step 7 and `gh pr create`, the PR number can drift and the pre-created `review-<N>.md` will no longer match the actual PR number. Please either document the required constraint (open the PR immediately) or document the rename/re-sync procedure when the predicted number changes.
```suggestion
numbering sequence between issues and PRs), so it is only a prediction
until `gh pr create` returns the actual PR number. The `## Summary`
section is the single source of truth for the PR body: open the PR with
`gh pr create --body-file <(scripts/extract_pr_body.sh NNNNN)` so the
GitHub body is a direct copy of the file. Because the description is
committed *before* push, a PR that gets no review comments merges
without any extra round-trip — the body is already in history. After
`/sprint-review`, open the PR immediately so the predicted number does
not drift. If another issue or PR lands first and GitHub assigns a
different number, rename the review file to
`doc/reviews/review-<actual-pr-number>.md` before any follow-up edits
and use that actual number for subsequent commands such as
`scripts/extract_pr_body.sh`. `review-00000.md` is a protected
sentinel; real reviews start at `00001`.
```

<!-- gh-id: 4152489396 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-22 06:28 UTC](https://github.com/cmk/riffgrep/pull/24#pullrequestreview-4152489396))

## Pull request overview

Ports upstream `template-rust` workflow/documentation updates to keep the repo’s Tier-1 review workflow and PR-creation flow consistent with the “review-NNNNN.md is source of truth” model.

**Changes:**
- Adds `scripts/extract_pr_body.sh` to extract the PR body from `doc/reviews/review-NNNNN.md`’s `## Summary` section for `gh pr create --body-file`.
- Updates workflow docs (`CLAUDE.md`, `doc/workflow.md`, `.claude/commands/sprint-review.md`) to require a pre-existing review file with `## Summary`, and to split ff-merge instructions for branch vs worktree usage.

### Reviewed changes

Copilot reviewed 4 out of 4 changed files in this pull request and generated 6 comments.

| File | Description |
| ---- | ----------- |
| scripts/extract_pr_body.sh | New helper script to extract a PR body from the `## Summary` section of the per-PR review file. |
| doc/workflow.md | Updates lifecycle diagram text to reflect drafting PR body during plan finalization. |
| CLAUDE.md | Updates Tier-1 workflow and TDD step 7/9 instructions to incorporate pre-push PR description + worktree merge guidance. |
| .claude/commands/sprint-review.md | Updates `/sprint-review` procedure to verify `review-NNNNN.md` + `## Summary` prerequisite and adjusts PR-open instructions. |





<!-- gh-id: 3121975758 -->
### Copilot on [`CLAUDE.md:283`](https://github.com/cmk/riffgrep/pull/24#discussion_r3121975758) (2026-04-22 06:28 UTC)

Step 7’s instructions again treat `NNNNN` as the direct output of `scripts/next_pr_number.sh`, but the repo convention is 5-digit zero-padded review filenames (`review-{N:05d}.md`). To avoid creating an incorrectly named file (and later failures in `/sprint-review` / `extract_pr_body.sh`), please clarify how to map the numeric PR number `N` to the padded filename, ideally with an explicit example (e.g. `N=17 -> review-00017.md`).
```suggestion
   - Run `scripts/next_pr_number.sh` to get the numeric PR number `N`,
     then create `doc/reviews/review-{N:05d}.md` (for example, `N=17`
     -> `doc/reviews/review-00017.md`). File header is
     `# PR #<N> — <title>` followed by a `## Summary` section
     containing the PR body. This section is consumed verbatim by
     `gh pr create --body-file <(scripts/extract_pr_body.sh N)`,
```

<!-- gh-id: 3121975777 -->
### Copilot on [`.claude/commands/sprint-review.md:78`](https://github.com/cmk/riffgrep/pull/24#discussion_r3121975777) (2026-04-22 06:28 UTC)

The prerequisite check instructs resolving `NNNNN` via `gh pr view`/`scripts/next_pr_number.sh` and then looking for `doc/reviews/review-NNNNN.md`, but it never clarifies that the directory uses 5-digit zero-padding (`review-{N:05d}.md`) while the commands return an unpadded integer. Please clarify the padding rule (with an example) to prevent false “file missing” aborts caused by looking for `review-17.md` instead of `review-00017.md`.
```suggestion

The review filename uses the PR number zero-padded to 5 digits:
`review-{N:05d}.md`. For example, PR `17` maps to
`doc/reviews/review-00017.md`, not `doc/reviews/review-17.md`.
```

<!-- gh-id: 3121975798 -->
### Copilot on [`.claude/commands/sprint-review.md:83`](https://github.com/cmk/riffgrep/pull/24#discussion_r3121975798) (2026-04-22 06:28 UTC)

With the new “step 7 creates the file” prerequisite, the predicted PR number can still drift if someone opens another issue/PR before push. If that happens, `scripts/next_pr_number.sh` will point at a different `review-<N>.md` than the one created in step 7, and `/sprint-review` will abort even though a valid review file exists. Consider documenting (or encoding in the procedure here) how to detect/handle this drift (e.g., prefer the already-existing newest `doc/reviews/review-*.md` on the branch, or instruct renaming the file to the updated padded number before proceeding).

<!-- gh-id: 3122030048 -->
#### ↳ cmk ([2026-04-22 06:42 UTC](https://github.com/cmk/riffgrep/pull/24#discussion_r3122030048))

Fixed in 06e3ee8 via `printf '%05d' "$((10#$1))"`. The header comment was also tightened to note that command substitution normalizes trailing newlines (so "verbatim" was overclaiming).

<!-- gh-id: 3122030195 -->
#### ↳ cmk ([2026-04-22 06:42 UTC](https://github.com/cmk/riffgrep/pull/24#discussion_r3122030195))

Fixed in 06e3ee8. The docs now spell out the zero-padding rule: `next_pr_number.sh` prints `N` unpadded; `review-NNNNN.md` is `N` zero-padded to 5 digits. The extractor pads internally (so callers can pass `17` or `00017`), but creators in step 7 have to pad themselves. Also clarified in `/sprint-review`'s prerequisite check.

<!-- gh-id: 3122030336 -->
#### ↳ cmk ([2026-04-22 06:42 UTC](https://github.com/cmk/riffgrep/pull/24#discussion_r3122030336))

Good catch — the "born with final name" claim was aspirational since `/sprint-review` re-predicts on each run. 06e3ee8 restores the drift-rename paragraph from template-rust so the workflow is honest about the edge case.

<!-- gh-id: 3122030531 -->
#### ↳ cmk ([2026-04-22 06:42 UTC](https://github.com/cmk/riffgrep/pull/24#discussion_r3122030531))

Fixed in 06e3ee8. The docs now spell out the zero-padding rule: `next_pr_number.sh` prints `N` unpadded; `review-NNNNN.md` is `N` zero-padded to 5 digits. The extractor pads internally (so callers can pass `17` or `00017`), but creators in step 7 have to pad themselves. Also clarified in `/sprint-review`'s prerequisite check.

<!-- gh-id: 3122030680 -->
#### ↳ cmk ([2026-04-22 06:42 UTC](https://github.com/cmk/riffgrep/pull/24#discussion_r3122030680))

Fixed in 06e3ee8. The docs now spell out the zero-padding rule: `next_pr_number.sh` prints `N` unpadded; `review-NNNNN.md` is `N` zero-padded to 5 digits. The extractor pads internally (so callers can pass `17` or `00017`), but creators in step 7 have to pad themselves. Also clarified in `/sprint-review`'s prerequisite check.

<!-- gh-id: 3122030874 -->
#### ↳ cmk ([2026-04-22 06:42 UTC](https://github.com/cmk/riffgrep/pull/24#discussion_r3122030874))

Went with rename-on-drift in 06e3ee8 rather than locate-by-branch (which would change `/sprint-review`'s behavior). The rename keeps `/sprint-review`'s re-prediction pointed at the right file. If drift becomes a real pain, switching to locate-by-branch is a cleaner follow-up.
