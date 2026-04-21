# PR #5 — Unify local + GitHub review into per-PR files

<!-- gh-id: 3098547699 -->
### Copilot on [`scripts/pull_reviews.py:135`](https://github.com/cmk/riffgrep/pull/5#discussion_r3098547699) (2026-04-17 07:22 UTC)

The high-water mark logic uses `max(gh-id)` across both review IDs and inline comment IDs, but those IDs come from different resource types/ranges and aren’t comparable. Once a review with a larger ID has been written, any later inline comments with smaller IDs will be permanently skipped on subsequent runs. De-dupe should be based on “ID already present in file” (set membership) or separate high-water marks per item kind/resource (e.g., `review_id` vs `comment_id`).

<!-- gh-id: 3098547732 -->
### Copilot on [`scripts/pull_reviews.py:6`](https://github.com/cmk/riffgrep/pull/5#discussion_r3098547732) (2026-04-17 07:22 UTC)

The module docstring describes idempotence as “only writes items with an id greater than the existing max”. Given GitHub review IDs and inline comment IDs are not from a single comparable sequence, this guarantee is not correct and can cause missed items. Update the docstring to match the actual de-dupe approach (ideally: skip IDs already present, or track high-water separately per resource type).
```suggestion
Idempotent: uses embedded `<!-- gh-id: N -->` markers to avoid writing
items that are already recorded in the target file. This de-duplication
should not be interpreted as a single globally ordered high-water mark
across both review IDs and inline comment IDs.
```

<!-- gh-id: 3098547756 -->
### Copilot on [`CLAUDE.md:134`](https://github.com/cmk/riffgrep/pull/5#discussion_r3098547756) (2026-04-17 07:22 UTC)

This section documents idempotence as a single “high-water mark” over `<!-- gh-id: ... -->`. Since GitHub review IDs and inline comment IDs aren’t comparable across resource types, a single max ID can cause new inline comments to be skipped after a review with a higher ID is recorded. Consider documenting (and implementing) de-dupe as “skip any gh-id already present” or keeping separate markers/high-water marks per item kind.
```suggestion
— it records `<!-- gh-id: NNNNN -->` markers for appended GitHub items and
skips any item whose `gh-id` is already present, so running it repeatedly
only appends new comments. The result is one file per PR containing the
full local + GitHub review history in order.
```

<!-- gh-id: 3098547786 -->
### Copilot on [`.claude/skills/pull-reviews.md:42`](https://github.com/cmk/riffgrep/pull/5#discussion_r3098547786) (2026-04-17 07:22 UTC)

This description of idempotence relies on a single high-water mark (`items newer than the last gh-id`). GitHub review IDs and inline comment IDs are not from a single ordered sequence, so “max gh-id” can cause later inline comments (with smaller IDs) to be ignored. Suggest changing the contract to de-dupe by “IDs already present in the file” (set membership) or track separate high-water marks per resource type.

<!-- gh-id: 3098547813 -->
### Copilot on [`.claude/skills/sprint-review.md:85`](https://github.com/cmk/riffgrep/pull/5#discussion_r3098547813) (2026-04-17 07:22 UTC)

The calibration examples here reference crates/features that don’t exist in this repository (e.g., `driver-motu`, `driver-qu`, and a `device = ["dep:tokio", ...]` feature gate). This makes the “contract” guidance misleading for actual riffgrep reviews; consider replacing with repo-relevant examples (e.g., embeddings/schema migration/property-test examples) or removing the concrete crate names.
```suggestion
  does Y, quote both. "The plan says this change must include a schema
  migration and a regression test for the new field, but the diff updates the
  persisted model without any migration or test coverage."

- **Name the consequence.** Don't just say "this differs from the plan." Say
  what breaks: "Without the migration, existing indexes can become unreadable
  after deploy, and without the regression test this is likely to recur."
```

<!-- gh-id: 4126867447 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-17 07:22 UTC](https://github.com/cmk/riffgrep/pull/5#pullrequestreview-4126867447))

## Pull request overview

Unifies Tier 1 (local) and Tier 2 (GitHub) review artifacts into a single per-PR review file (`doc/reviews/review-NNNN.md`) and adds tooling/docs to automatically append GitHub review bodies + inline comments into that file.

**Changes:**
- Add `scripts/pull_reviews.py` + `/pull-reviews` skill to fetch PR reviews/inline comments and append them chronologically into `doc/reviews/review-NNNN.md`.
- Update review workflow documentation (`CLAUDE.md`, `/sprint-review`) to use PR-numbered review files.
- Backfill historical reviews into `doc/reviews/review-0001.md` … `review-0004.md` and remove the older dated review files.

### Reviewed changes

Copilot reviewed 10 out of 10 changed files in this pull request and generated 7 comments.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| `scripts/pull_reviews.py` | New script to fetch PR reviews/comments via `gh` and append to per-PR review files. |
| `CLAUDE.md` | Updates the two-tier review workflow to use `review-NNNN.md` and introduces `/pull-reviews`. |
| `.claude/skills/sprint-review.md` | Updates Tier 1 skill instructions/output format for PR-numbered review files and refreshes reviewer guidance. |
| `.claude/skills/pull-reviews.md` | New skill wrapper documentation for running `scripts/pull_reviews.py`. |
| `doc/reviews/review-0001.md` | New consolidated per-PR review file seeded with historical GitHub review content for PR #1. |
| `doc/reviews/review-0002.md` | New consolidated per-PR review file seeded with local + GitHub review content for PR #2. |
| `doc/reviews/review-0003.md` | New consolidated per-PR review file seeded with GitHub review content for PR #3. |
| `doc/reviews/review-0004.md` | New consolidated per-PR review file seeded with local + GitHub review content for PR #4. |
| `doc/reviews/review-2026-04-15-01.md` | Removed in favor of `doc/reviews/review-0002.md`. |
| `doc/reviews/review-2026-04-15-02.md` | Removed in favor of `doc/reviews/review-0004.md`. |
</details>






---

💡 <a href="https://github.com/cmk/riffgrep/new/main?filename=.github/instructions/*.instructions.md" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Add Copilot custom instructions</a> for smarter, more guided reviews. <a href="https://docs.github.com/en/copilot/customizing-copilot/adding-repository-custom-instructions-for-github-copilot" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Learn how to get started</a>.

<!-- gh-id: 3098547840 -->
### Copilot on [`.claude/skills/sprint-review.md:170`](https://github.com/cmk/riffgrep/pull/5#discussion_r3098547840) (2026-04-17 07:22 UTC)

This checklist item refers to `fixture_or_skip!` from `studio_core::testing`, but there’s no `studio_core` crate or `fixture_or_skip!` usage in this repo. As written it will send reviewers looking for a non-existent convention; consider updating this to the actual fixture-gating pattern used here (or removing the macro reference).
```suggestion
- Do fixture-gated tests skip cleanly when a fixture is absent (for
  example, by returning early), rather than panicking or using
  `#[ignore]`?
```

<!-- gh-id: 3098547859 -->
### Copilot on [`scripts/pull_reviews.py:26`](https://github.com/cmk/riffgrep/pull/5#discussion_r3098547859) (2026-04-17 07:22 UTC)

`gh api` calls aren’t paginated, so PRs with >30 reviews/comments will be truncated and the generated review file will silently miss older items. Consider adding pagination (e.g., `gh api --paginate --slurp ...` or an explicit `per_page=100` loop) for both `/reviews` and `/comments` endpoints.
```suggestion
    data = json.loads(
        subprocess.check_output(
            ["gh", "api", "--paginate", "--slurp", path],
            text=True,
        )
    )
    if not isinstance(data, list):
        return data
    if not data:
        return []
    if all(isinstance(page, list) for page in data):
        items: list = []
        for page in data:
            items.extend(page)
        return items
    if len(data) == 1 and isinstance(data[0], dict):
        return data[0]
    return data
```

<!-- gh-id: 3098643338 -->
### Copilot on [`.claude/skills/pull-reviews.md:96`](https://github.com/cmk/riffgrep/pull/5#discussion_r3098643338) (2026-04-17 07:42 UTC)

The note says a reply “follows its parent because GitHub assigns monotonically increasing ids within a PR”, but the actual script sorts by timestamps and replies can be interleaved with other comments/reviews depending on when they were posted. This is also not something the format contract should rely on. Suggest rewording to: items are appended in chronological order, and replies are merely formatted with `↳` based on `in_reply_to_id` (not grouped/adjacent).
```suggestion
- **Chronological, not grouped.** Items are appended in posted order.
  Replies are only indicated by `↳` formatting based on
  `in_reply_to_id`; they are not guaranteed to be grouped with or
  adjacent to their parent.
```

<!-- gh-id: 3098643391 -->
### Copilot on [`scripts/pull_reviews.py:31`](https://github.com/cmk/riffgrep/pull/5#discussion_r3098643391) (2026-04-17 07:42 UTC)

`gh_api` uses the PEP 604 union operator (`list | dict`) in the return annotation, which is a SyntaxError on Python < 3.10. If these scripts are expected to run on older system Pythons, switch to `typing.Union[...]` (or drop the annotation) or explicitly document that Python 3.10+ is required.
```suggestion
from typing import Union


def gh_api(path: str) -> Union[list, dict]:
```

<!-- gh-id: 3098643409 -->
### Copilot on [`scripts/pull_reviews.py:62`](https://github.com/cmk/riffgrep/pull/5#discussion_r3098643409) (2026-04-17 07:42 UTC)

`--repo` is accepted on the CLI and used for API calls, but `pr_title()` always runs `gh pr view` against the current repo. If the user passes `--repo owner/name` (or runs the script outside the target repo), the header line can be created with the wrong title or fail entirely. Pass the resolved `repo` into `pr_title()` and include `--repo` in the `gh pr view` invocation.
```suggestion
def pr_title(n: int, repo: str | None = None) -> str:
    cmd = ["gh", "pr", "view", str(n)]
    if repo is not None:
        cmd.extend(["--repo", repo])
    cmd.extend(["--json", "title", "--jq", ".title"])
    return subprocess.check_output(cmd, text=True).strip()
```

<!-- gh-id: 3098643431 -->
### Copilot on [`scripts/pull_reviews.py:73`](https://github.com/cmk/riffgrep/pull/5#discussion_r3098643431) (2026-04-17 07:42 UTC)

`existing_ids()` reads the review file with `path.read_text()` using the platform default encoding. Since GitHub review bodies can contain arbitrary Unicode, this can fail (or corrupt data) on non-UTF-8 locales (notably Windows). Use an explicit `encoding="utf-8"` for both `read_text()` and `write_text()`/`open()` writes to keep the script portable and deterministic.
```suggestion
    return {
        int(h)
        for h in re.findall(r"<!-- gh-id: (\d+) -->", path.read_text(encoding="utf-8"))
    }
```

<!-- gh-id: 3098643454 -->
### Copilot on [`scripts/reply_review.py:27`](https://github.com/cmk/riffgrep/pull/5#discussion_r3098643454) (2026-04-17 07:42 UTC)

`gh_repo()` uses `subprocess.check_output(...)` without handling `CalledProcessError`. If `gh` isn't installed/authenticated or the repo can't be resolved, the script will exit with a Python traceback instead of a clear one-line error. Consider catching `CalledProcessError` (and possibly `FileNotFoundError`) and printing a concise message to stderr before returning a non-zero exit code.
```suggestion
    try:
        return subprocess.check_output(
            ["gh", "repo", "view", "--json", "nameWithOwner", "--jq", ".nameWithOwner"],
            text=True,
            stderr=subprocess.PIPE,
        ).strip()
    except FileNotFoundError:
        print("error: `gh` CLI not found; install GitHub CLI and ensure it is on PATH", file=sys.stderr)
        raise SystemExit(1)
    except subprocess.CalledProcessError as exc:
        detail = (exc.stderr or "").strip()
        if detail:
            print(f"error: failed to determine repository: {detail}", file=sys.stderr)
        else:
            print("error: failed to determine repository via `gh repo view`", file=sys.stderr)
        raise SystemExit(1)
```

<!-- gh-id: 4126982850 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-17 07:42 UTC](https://github.com/cmk/riffgrep/pull/5#pullrequestreview-4126982850))

## Pull request overview

Copilot reviewed 12 out of 12 changed files in this pull request and generated 5 comments.





---

💡 <a href="https://github.com/cmk/riffgrep/new/main?filename=.github/instructions/*.instructions.md" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Add Copilot custom instructions</a> for smarter, more guided reviews. <a href="https://docs.github.com/en/copilot/customizing-copilot/adding-repository-custom-instructions-for-github-copilot" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Learn how to get started</a>.

<!-- gh-id: 3098670399 -->
#### ↳ cmk ([2026-04-17 07:48 UTC](https://github.com/cmk/riffgrep/pull/5#discussion_r3098670399))

Fixed in 3bea723 — `existing_ids()` now returns a set of all recorded gh-ids and we filter with `not in seen`, so items from either sequence are preserved.

<!-- gh-id: 3098670631 -->
#### ↳ cmk ([2026-04-17 07:48 UTC](https://github.com/cmk/riffgrep/pull/5#discussion_r3098670631))

Fixed in 3bea723 — docstring now describes set-membership on gh-id markers and explicitly calls out that max-id across reviews + comments would be unsound.

<!-- gh-id: 3098670778 -->
#### ↳ cmk ([2026-04-17 07:48 UTC](https://github.com/cmk/riffgrep/pull/5#discussion_r3098670778))

Fixed in 3bea723 — CLAUDE.md Tier-2 paragraph rephrased to describe the de-dup as "skips any id already present" rather than a high-water mark.

<!-- gh-id: 3098670931 -->
#### ↳ cmk ([2026-04-17 07:48 UTC](https://github.com/cmk/riffgrep/pull/5#discussion_r3098670931))

Fixed in 3bea723 — skill now says idempotency is via set membership on gh-ids, with a note that max-id would be unsound across the two sequences.

<!-- gh-id: 3098671370 -->
#### ↳ cmk ([2026-04-17 07:48 UTC](https://github.com/cmk/riffgrep/pull/5#discussion_r3098671370))

Fixed in 3bea723 — swapped the template-rust examples (driver-motu, feature gate) for riffgrep analogues (v1→v2 embedding migration, `load_all_embeddings` length guard).

<!-- gh-id: 3098671594 -->
#### ↳ cmk ([2026-04-17 07:48 UTC](https://github.com/cmk/riffgrep/pull/5#discussion_r3098671594))

Fixed in 3bea723 — dropped the `fixture_or_skip!`/`studio_core::testing` reference and replaced with plain guidance tied to CLAUDE.md's "fresh checkout must pass cargo test with zero setup" rule.

<!-- gh-id: 3098671705 -->
#### ↳ cmk ([2026-04-17 07:48 UTC](https://github.com/cmk/riffgrep/pull/5#discussion_r3098671705))

Fixed in 3bea723 — `gh_api()` now uses `--paginate --slurp` and flattens the resulting list-of-pages, so both /reviews and /comments fetch fully.

<!-- gh-id: 3098671841 -->
#### ↳ cmk ([2026-04-17 07:48 UTC](https://github.com/cmk/riffgrep/pull/5#discussion_r3098671841))

Fixed in 79e2481 — note now says items are appended by timestamp and replies can interleave with other comments; `↳` formatting is independent of ordering.

<!-- gh-id: 3098672512 -->
#### ↳ cmk ([2026-04-17 07:48 UTC](https://github.com/cmk/riffgrep/pull/5#discussion_r3098672512))

Declining — both scripts start with `from __future__ import annotations`, which makes all annotations lazy (evaluated as strings). `list | dict` in a return annotation is a string at runtime on any Python ≥3.7, so it doesn't raise on 3.9 or earlier. If you can point to an env where this actually fails, happy to revisit.

<!-- gh-id: 3098672646 -->
#### ↳ cmk ([2026-04-17 07:48 UTC](https://github.com/cmk/riffgrep/pull/5#discussion_r3098672646))

Fixed in 79e2481 — `pr_title()` now takes an optional `repo` arg and forwards `--repo` to `gh pr view` when it's set. `main()` passes the resolved repo through.

<!-- gh-id: 3098672773 -->
#### ↳ cmk ([2026-04-17 07:48 UTC](https://github.com/cmk/riffgrep/pull/5#discussion_r3098672773))

Fixed in 79e2481 — `encoding="utf-8"` pinned on `read_text()` in `existing_ids()`, the header `write_text()`, and the append `open()`.

<!-- gh-id: 3098672915 -->
#### ↳ cmk ([2026-04-17 07:48 UTC](https://github.com/cmk/riffgrep/pull/5#discussion_r3098672915))

Fixed in 79e2481 — `gh_repo()` in both scripts now catches `FileNotFoundError` (gh missing) and `CalledProcessError` (auth/repo failure), prints a one-line stderr message, and exits 1.
