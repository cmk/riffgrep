---
name: pull-reviews
description: >
  Fetch GitHub review comments for a PR and append them to the local
  review file. Use when the user says "/pull-reviews <N>", "fetch reviews
  for PR N", or "pull down the GH comments".
---

# Pull Reviews — Fetch GitHub Comments to Local File

Fetch review comments from a GitHub PR and append new ones chronologically
to `doc/reviews/review-NNNN.md`. The heavy lifting lives in
`scripts/pull_reviews.py`; this skill is a thin wrapper around it.

---

## Step 1: Parse the PR number

The user provides a PR number as an argument (e.g., `/pull-reviews 17`).
If omitted, check if the current branch has an open PR:

```
gh pr view --json number --jq .number
```

If no PR is found, ask the user for the number.

## Step 2: Run the script

```
scripts/pull_reviews.py <N>
```

The script handles everything: fetches reviews and inline comments via
`gh api --paginate` (so PRs with >30 items aren't truncated), merges
them chronologically, de-dupes via set membership on `<!-- gh-id: -->`
markers, hyperlinks headers to GitHub permalinks, absolute-ifies
relative links in bodies, and creates `review-NNNN.md` with a
`# PR #N — <title>` header if it doesn't exist.

The script is idempotent: any item whose `gh-id` is already present in
the file is skipped. Note: it is **not** safe to assume a single
"high-water mark" — GitHub draws review IDs and inline-comment IDs from
different sequences, so max-id across both would silently drop later
items from the lower-numbered sequence. Set membership avoids this.

## Step 3: Report

Print a one-paragraph summary: how many new comments appended, from
which reviewers, and the path to the review file. Pipe through the
script's stdout if that's easier.

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

Inline comment (new thread):

```markdown
<!-- gh-id: {id} -->
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

- **Do not commit the review file.** The user decides when to commit.
- **Idempotent** via set membership on `<!-- gh-id: -->` markers (not
  max-id, which would be unsound across review/comment sequences).
  Safe to re-run.
- **Chronological, not grouped.** Items are appended in posted order
  (by `created_at` / `submitted_at`). Replies are only indicated by
  `↳` formatting based on `in_reply_to_id`; they are not guaranteed
  to be adjacent to their parent — other comments posted in between
  will interleave.
