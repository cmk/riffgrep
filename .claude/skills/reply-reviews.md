---
name: reply-reviews
description: >
  After addressing GitHub PR review findings with a fix commit, post
  short replies to each unresolved comment thread on GitHub. Use when
  the user says "/reply-reviews <N>", "reply to the review comments",
  or after landing a fix commit that addresses Copilot/reviewer
  feedback on a PR.
---

# Reply Reviews — Post Replies to GitHub Review Threads

Post short, direct replies to PR review comments that have been
addressed locally but not yet answered on GitHub. This closes the loop
for the reviewer and leaves a paper trail linking each finding to the
commit that addressed it.

Intended to run **after** a fix commit that addresses the feedback has
been pushed — the replies cite the fix commit's SHA.

---

## Step 1: Determine the PR

User provides a PR number (e.g. `/reply-reviews 5`). If omitted, use
the current branch's open PR:

```
gh pr view --json number --jq .number
```

Abort if no PR is found.

## Step 2: Identify unreplied threads

Read `doc/reviews/review-NNNN.md`. A thread is a top-level inline
comment (a `### {user} on [\`{path}:{line}\`](...)` header) plus any
`#### ↳ {user}` replies beneath it, terminating at the next `### ` or
end of file.

A thread is **unreplied** if no `↳` reply in it is authored by a
non-bot account (i.e., not `Copilot`, not `copilot-pull-request-reviewer[bot]`,
not `claude[bot]`). Top-level review-body sections (`### {user} — {state}`,
no `on [\`path:line\`]`) can usually be skipped — they don't take
threaded replies via the review-comment API.

If the file is stale (you know there's been GH activity since the last
`/pull-reviews`), run `scripts/pull_reviews.py <N>` first so you're
replying against current state.

## Step 3: Compose replies

For each unreplied thread, compose a short reply (1–3 sentences) that
does one of:

- **Acknowledge a fix**: cite the fix commit SHA and briefly name what
  changed. Example: `Fixed in 3bea723 — switched to set-membership
  de-dup per your suggestion.`
- **Accept as follow-up**: explain why it's deferred and where it's
  tracked. Example: `Good catch, deferred — tracked as a follow-up in
  PR description. Not blocking this change.`
- **Push back with reasoning**: if the suggestion is wrong or
  inapplicable, say why in one sentence without hedging.

Guidance:

- Cite the commit SHA (7 chars) when a fix exists. `git log
  --oneline origin/main..HEAD` is your source.
- Don't thank the reviewer. Don't apologize. Don't repeat the comment
  back.
- Don't post a reply that just says "done" — name what was done.
- If multiple comments got fixed by the same commit, each reply should
  still cite it individually (threads are independent on GitHub).

## Step 4: Post via the script

For each composed reply:

```
scripts/reply_review.py <PR> <in_reply_to_id> "<body>"
```

`<in_reply_to_id>` is the `gh-id` from the top-level comment's
`<!-- gh-id: NNNNN -->` marker in the review file. The script posts
via `gh api` and prints the new reply's id and URL.

Pass `-` as the body arg to read from stdin for long or multi-line
replies:

```
scripts/reply_review.py 5 3098547699 - <<'EOF'
Fixed in 3bea723 — ...
EOF
```

## Step 5: Mirror locally (optional)

After posting, run `scripts/pull_reviews.py <N>` to append your replies
to the local review file. Set-membership de-dup ensures only the new
replies get added.

## Step 6: Report

Print a summary: how many threads replied to, and the path to the
review file. One paragraph max.

## Notes

- **Bots don't count as human replies.** A Copilot `↳ Copilot` reply
  under its own comment doesn't satisfy "replied". Only a human-authored
  reply closes the thread.
- **One reply per thread, not per comment.** If a thread already has
  your reply buried three deep, don't post another.
- **Don't commit the updated review file in the same push as the
  replies.** The reply round-trip through `/pull-reviews` is a separate
  operation from landing the fix.
