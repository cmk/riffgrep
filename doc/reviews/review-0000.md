# PR #0000 — port-review-workflow (template-rust workflow import)

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
