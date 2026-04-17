---
name: pull-reviews
description: >
  Fetch GitHub review comments for a PR and append them to the local
  review file. Use when the user says "/pull-reviews <N>", "fetch reviews
  for PR N", or "pull down the GH comments".
---

# Pull Reviews — Fetch GitHub Comments to Local File

Fetch review comments from a GitHub PR and append new ones chronologically
to the local review file `doc/reviews/review-NNNN.md`.

---

## Step 1: Parse the PR number

The user provides a PR number as an argument (e.g., `/pull-reviews 17`).
If omitted, check if the current branch has an open PR:

```
gh pr view --json number --jq .number
```

If no PR is found, ask the user for the number.

## Step 2: Find the high-water mark

Read `doc/reviews/review-NNNN.md` if it exists. Scan for all
`<!-- gh-id: NNNNN -->` markers and take the maximum. This is the
high-water mark — only items with `id` greater than this value are new.

If the file doesn't exist or has no markers, all items are new.

## Step 3: Fetch comments

Fetch all review comments for the PR, sorted by creation time:

```
gh api repos/{owner}/{repo}/pulls/{N}/comments \
  --jq 'sort_by(.created_at) | .[] | {id, user: .user.login, path, line, body, created_at, in_reply_to_id}'
```

Also fetch top-level review bodies (Copilot summaries, human approvals):

```
gh api repos/{owner}/{repo}/pulls/{N}/reviews \
  --jq 'sort_by(.submitted_at) | .[] | select(.body != "") | {id, user: .user.login, state, body, submitted_at}'
```

Filter both lists to only entries with `id` greater than the high-water
mark.

## Step 4: Format and append

Append new comments **chronologically** (by `created_at` / `submitted_at`)
to the review file. Each comment is a self-contained block:

For a top-level review body:

```markdown
<!-- gh-id: {id} -->
### {user} — {state} ({YYYY-MM-DD HH:MM UTC})

{body}
```

For an inline comment (new thread):

```markdown
<!-- gh-id: {id} -->
### {user} on `{path}:{line}` ({YYYY-MM-DD HH:MM UTC})

{body}
```

For a reply (has `in_reply_to_id`):

```markdown
<!-- gh-id: {id} -->
#### ↳ {user} ({YYYY-MM-DD HH:MM UTC})

{body}
```

If the file is new, add a top-level header first:

```markdown
# PR #{N} — {PR title}
```

Fetch the PR title via:

```
gh pr view {N} --json title --jq .title
```

## Step 5: Report

Print a one-paragraph summary: how many new comments appended, from
which reviewers, and the path to the review file.

## Notes

- **Do not commit the review file.** The user decides when to commit.
- **Idempotent.** The high-water mark (`gh-id` HTML comments) ensures
  running twice never duplicates. The agent's only job is: find the
  largest `gh-id` already in the file, append anything newer.
- **Chronological, not grouped.** Comments appear in the order they were
  posted on GitHub. This preserves the conversational flow — a reply
  appears right after the comment it responds to (since GitHub assigns
  monotonically increasing ids within a PR). Use `in_reply_to_id` only
  to decide the `↳ reply` formatting, not to reorder.
