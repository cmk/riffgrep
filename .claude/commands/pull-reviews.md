---
description: Fetch GitHub PR review bodies and inline comments for a PR and append new ones chronologically to doc/reviews/review-NNNNN.md. Lower-level primitive — normally invoked by /reply-reviews, but usable standalone to refresh the doc before final push.
argument-hint: <pr-number>
---

# Pull Reviews — Fetch GitHub Comments to Local File

Fetch review comments from a GitHub PR and append new ones chronologically
to `doc/reviews/review-NNNNN.md`. The heavy lifting lives in
`scripts/pull_reviews.py`; this command is a thin wrapper around it.

**In the normal review-round workflow, `/reply-reviews` invokes this
internally** after posting replies, so you rarely run it by hand.
Standalone use is for: (a) refreshing `review-NNNNN.md` right before the
final pre-merge push to capture any trailing reviewer comments; or
(b) catching up after a round where someone else pushed review activity
to the PR.

---

## Step 1: Parse the PR number

The user provides a PR number as an argument (e.g., `/pull-reviews 17`).
Argument: `$ARGUMENTS`

If omitted, check if the current branch has an open PR:

```
gh pr view --json number --jq .number
```

If no PR is found, ask the user for the number.

## Step 2: Run the script

```
scripts/pull_reviews.py <N>
```

The script handles everything: fetches reviews and inline comments by
iterating `?per_page=100&page=N` explicitly against the GitHub API
(chosen over `gh api --paginate --slurp` because `--slurp` requires
gh >= 2.47), merges them chronologically, de-dupes via set membership
on `<!-- gh-id: -->` markers, hyperlinks headers to GitHub permalinks,
absolute-ifies relative links in bodies, and creates `review-NNNNN.md`
with a `# PR #N — <title>` header if it doesn't exist.

The script is idempotent: any item whose `gh-id` is already present in
the file is skipped. Note: it is **not** safe to assume a single
"high-water mark" — GitHub draws review IDs and inline-comment IDs from
different sequences, so max-id across both would silently drop later
items from the lower-numbered sequence. Set membership avoids this.

## Step 3: Let the file ride with the next fix commit

The review file is not committed on its own. It rides with the **next
review round's fix commit** — the commit that addresses the comments
you just pulled.

The `/reply-reviews` command enforces this: it posts replies, runs
`pull_reviews.py`, and `git commit --amend`s the mutated doc into the
pre-push fix commit, so one push delivers everything in order.

If you invoked `/pull-reviews` standalone (no paired reply round), the
file stays modified-but-uncommitted on disk. A later `/reply-reviews`
will pick it up and fold it in. Do **not** open a standalone
`doc: update review-NNNNN.md` commit just to land it — that forces a CI
round-trip for no code change and was the ordering bug the unified
`/reply-reviews` was designed to eliminate.

If there were no new items (script reported `no new items`), there is
nothing staged and nothing to do.

## Step 4: Report

Print a one-paragraph summary: how many new items were appended and
the path to the review file. The script's own stdout line —
`PR #N: appended K items -> <path>` — already gives you both, so you
can pipe it through verbatim if that's easier.

---

## Format contract (for reference / debugging)

The script writes three block shapes. If you're editing the file by hand
or extending the script, preserve these:

Top-level review body:

```markdown
<!-- gh-id: {id} -->
### {user} — {state} ([{YYYY-MM-DD HH:MM UTC}]({html_url}))

{body}
```

Inline comment (new thread). The `:line` suffix is omitted when the
GitHub API returns `line: null` (outdated diff comments or file-level
comments), so both `{path}` and `{path}:{line}` variants are valid:

```markdown
<!-- gh-id: {id} -->
### {user} on [`{path}`]({html_url}) ({YYYY-MM-DD HH:MM UTC})
### {user} on [`{path}:{line}`]({html_url}) ({YYYY-MM-DD HH:MM UTC})

{body}
```

Reply (has `in_reply_to_id`):

```markdown
<!-- gh-id: {id} -->
#### ↳ {user} ([{YYYY-MM-DD HH:MM UTC}]({html_url}))

{body}
```

## Notes

- **The script does not auto-commit.** The file rides with the next
  round's fix commit (see Step 3); don't land a standalone `doc:`
  commit just to attach the audit trail.
- **Idempotent** via set membership on `<!-- gh-id: -->` markers (not
  max-id, which would be unsound across review/comment sequences).
  Safe to re-run.
- **Chronological, not grouped.** Items are appended in posted order
  (by `created_at` / `submitted_at`). Replies are only indicated by
  `↳` formatting based on `in_reply_to_id`; they are not guaranteed
  to be adjacent to their parent — other comments posted in between
  will interleave.
