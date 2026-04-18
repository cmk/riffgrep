# PR #11 — port-review-workflow (template-rust workflow import)

## Local review (2026-04-17)

**Branch:** port-review-workflow
**Commits:** 4 (origin/main..port-review-workflow)
**Reviewer:** Claude (sonnet, independent)

---

### 1. Python port correctness

**`_gh.resolve_repo`: error-handling semantics preserved.** Both scripts previously called `gh_repo()` only, with no PR pre-flight. `resolve_repo` is strictly more defensive — calls `gh_repo()` then pre-flights via `gh api repos/{repo}/pulls/{pr}`, surfacing mismatched cwd early. No regression.

**`gh_api` explicit pagination logic is sound.** Early-exit on `len(raw) < 100` plus fallback on `not raw` handles both the "partial last page" and "exact-multiple-of-100" cases without loss.

**Latent gap (pre-existing, not a regression).** `gh_api` returns a dict silently on page 1 when the endpoint is expected to be a list. GitHub can emit dict-shaped error bodies on some rate-limit responses; those would propagate to `collect_items` as no items rather than raising. The old `--paginate --slurp` code had the same gap. Not addressed here; would be worth a defensive `assert/log` in a follow-up.

**`submitted_at` guard** on reviews without a submission timestamp is a strict improvement over the old code's implicit `KeyError`.

### 2. Skill-vs-CLAUDE.md contradiction

Resolved in `ed72383`. Prior CLAUDE.md text allowed either "standalone `doc:` commit" or "folded into fix commit" for mirrored GH comments, contradicting the ported skills' "never standalone" rule. Replaced with the ride-with-fix-commit wording and explicit no-op-round handling.

### 3. Copilot prompt scope

`applyTo: "doc/**/*.md"` is the right scope — skills under `.claude/skills/` and source under `src/` stay under the default strict voice.

**One over-broad suppression** in the "what to skip" list: "comment-style nits in illustrative code snippets" is worded loosely enough to also silence findings on `my-crate::module` (invalid Rust module path) vs `my_crate::module` in callable positions, not just in inline `//` comments. Upstream template ownership — filing as a follow-up against template-rust rather than drifting the local copy.

### Recommendations

**Must fix before push:** none remaining — CLAUDE.md sync landed in `ed72383`.

**Follow-up (future work):**
1. `gh_api` defensive assert when a list endpoint returns a dict on page 1. Low urgency, not a regression.
2. Upstream template-rust: narrow the docs-review "skip" rule for illustrative code snippets to inline comments only, so invalid Rust module paths in callable positions still get flagged.

<!-- gh-id: 3104379407 -->
### Copilot on [`.github/instructions/docs-review.instructions.md:46`](https://github.com/cmk/riffgrep/pull/11#discussion_r3104379407) (2026-04-18 03:16 UTC)

This section uses `crates/` as an example of a "missing path prefix" pattern to ignore, but the repo doesn't have a `crates/` layout. Using a non-existent path here makes the guidance harder to apply; consider rewriting this example to a repo-relevant pattern (or remove the specific `crates/` reference).
```suggestion
  X and a pattern the reviewer would otherwise flag (broken
  cross-link, inconsistent formatting, stale terminology, etc.) also
  appears on lines the PR did *not* modify in file X, do not flag it.
  Those are concerns for a separate cleanup PR.
```

<!-- gh-id: 4133373747 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-18 03:16 UTC](https://github.com/cmk/riffgrep/pull/11#pullrequestreview-4133373747))

## Pull request overview

Ports the repository’s PR review-workflow tooling and documentation conventions from `template-rust`, including shared GitHub CLI helpers, more compatible pagination logic, and updated “review file rides with fix commits” guidance.

**Changes:**
- Add `scripts/_gh.py` and refactor `pull_reviews.py` / `reply_review.py` to use shared repo-resolution + improved error handling.
- Replace `gh api --paginate --slurp` usage with explicit `?per_page=100&page=N` pagination in `scripts/pull_reviews.py`.
- Sync workflow documentation across `.claude/skills/*`, `CLAUDE.md`, and add a scoped Copilot docs review instruction file.

### Reviewed changes

Copilot reviewed 9 out of 9 changed files in this pull request and generated 5 comments.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| scripts/reply_review.py | Switches to shared repo resolution and improves subprocess error handling. |
| scripts/pull_reviews.py | Implements explicit pagination and tightens error handling / guards while collecting review items. |
| scripts/_gh.py | New shared helper for resolving repo + optional PR existence preflight. |
| doc/reviews/review-0000.md | New local sprint-review record for this PR (currently placeholder numbered). |
| CLAUDE.md | Updates two-tier review workflow guidance to avoid standalone audit-trail commits. |
| .github/instructions/docs-review.instructions.md | Adds Copilot instructions for doc-only reviews under `doc/**/*.md`. |
| .claude/skills/sprint-review.md | Fixes calibration file path reference. |
| .claude/skills/reply-reviews.md | Updates reply/mirroring workflow to “ride with fix commit” pattern and handles missing `line`. |
| .claude/skills/pull-reviews.md | Updates skill docs to match explicit pagination and new commit/mirroring rhythm. |
</details>






---

💡 <a href="https://github.com/cmk/riffgrep/new/main?filename=.github/instructions/*.instructions.md" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Add Copilot custom instructions</a> for smarter, more guided reviews. <a href="https://docs.github.com/en/copilot/customizing-copilot/adding-repository-custom-instructions-for-github-copilot" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Learn how to get started</a>.

<!-- gh-id: 3104379421 -->
### Copilot on [`scripts/pull_reviews.py:70`](https://github.com/cmk/riffgrep/pull/11#discussion_r3104379421) (2026-04-18 03:16 UTC)

`gh_api()` explicitly returns a dict when the endpoint returns non-list JSON (e.g., rate-limit/error bodies), but callers in `collect_items()` iterate the result and assume each item is a dict with `.get()` / `[]`. If `gh_api()` returns a dict, the current code will crash with a confusing error (iterating dict keys / AttributeError). Consider treating a dict response from these list endpoints as a hard error (print details + exit) or splitting into separate helpers for list vs scalar endpoints.

<!-- gh-id: 3104379429 -->
### Copilot on [`scripts/_gh.py:52`](https://github.com/cmk/riffgrep/pull/11#discussion_r3104379429) (2026-04-18 03:16 UTC)

`resolve_repo()` skips the PR-exists preflight when `--repo` is provided (`repo_override` short-circuits). That means a typoed `--repo` falls through to later endpoints and produces a less actionable error, and it’s slightly at odds with the PR description’s “PR-exists pre-flight” framing. Consider still doing the `gh api repos/{repo}/pulls/{pr}` check even when `--repo` is passed (just against the overridden repo).

<!-- gh-id: 3104379438 -->
### Copilot on [`scripts/pull_reviews.py:76`](https://github.com/cmk/riffgrep/pull/11#discussion_r3104379438) (2026-04-18 03:16 UTC)

The new explicit pagination / error-handling paths in `gh_api()` and the PR preflight behavior are important logic, but there are no unit tests covering them. Since the repo already has a `scripts/tests/` pytest suite, consider adding small tests that mock `subprocess.check_output` / `subprocess.run` to verify: multi-page fetch, exact-100 boundary behavior, and that non-list JSON responses are handled as intended.

<!-- gh-id: 3104379442 -->
### Copilot on [`.github/instructions/docs-review.instructions.md:11`](https://github.com/cmk/riffgrep/pull/11#discussion_r3104379442) (2026-04-18 03:16 UTC)

This instruction file claims "Source code under `crates/`" is reviewed under the default voice, but this repo has no `crates/` directory (it uses `src/`/`scripts/`). This is a factual mismatch that can confuse tooling/users; please update the path(s) to match the actual repo layout (or make the statement path-agnostic).
```suggestion
They narrow Copilot's review voice for prose content. Source code is
reviewed under the default (strict) voice; nothing here relaxes
that.
```
