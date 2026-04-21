---
description: Finalize a PR review round before push. Posts replies to each unresolved thread, mirrors the replies back into review-NNNNN.md via pull_reviews.py, and folds the mutated doc into the round's fix commit via git commit --amend. One push delivers the whole round. Refuses to run if the fix commit was already pushed (which would force an extra commit or force-push).
argument-hint: <pr-number>
---

# /reply-reviews — Finalize a Review Round (Before Push)

Posts short replies to each unresolved PR review thread, mirrors them
back into `doc/reviews/review-NNNNN.md`, and folds the resulting doc
update into the round's **unpushed** fix commit. Stops before `git push`
— the user runs that explicitly as the last step, so one push delivers
code + replies + review doc.

This ordering — fix → reply → mirror → amend → push — is deliberate.
Pushing before the mirror strands the reply-mirror in the working tree
and forces either a wasted `doc:` commit (extra CI round-trip) or a
force-push (disallowed by `.claude/settings.local.json`'s deny list).

Target PR: `$ARGUMENTS`

---

## Step 0: Preconditions (bail before posting anything)

Posting replies is destructive — they hit GitHub immediately and can't
be un-posted. Validate upfront; do not proceed past this step if any
check fails.

### 0a. Determine the PR number

If `$ARGUMENTS` is non-empty, use it. Otherwise fall back to the
current branch's open PR:

```
gh pr view --json number --jq .number
```

Abort if neither yields a number.

### 0b. Fix commit must be unpushed

The whole premise of this command is amending the fix commit with the
review-doc update, which is only safe while the commit is local-only.
Check:

```
branch=$(git branch --show-current)
git fetch --quiet origin "$branch" || true
git log "origin/$branch..HEAD" --oneline
```

- If there are **unpushed commits**: proceed. The last one is assumed
  to be the fix commit for this round. (If it's not — e.g., you made
  an unrelated commit after the fix — stop and tell the user to reorder
  first.)
- If there are **no unpushed commits** and there are unreplied threads:
  **refuse**. The fix commit was already pushed, which is the workflow
  bug this command prevents. Print:
  > Fix commit has already been pushed for PR #N. To avoid an extra
  > `doc:` commit or a force-push, the intended ordering is: make the
  > fix commit → run `/reply-reviews` → `git push`. Recovery options:
  > (a) post replies manually via `scripts/reply_review.py` and accept
  > a later round will pick up the mirror; (b) make a new fix commit
  > that bundles something worth committing plus the mirror, then run
  > `/reply-reviews` again.

  Do not post replies. Do not force-push.

### 0c. Working tree sanity

Run `git status`. If any path other than `doc/reviews/review-NNNNN.md`
has uncommitted changes, stop and ask the user to commit or stash
first. The amend in Step 6 is surgical — it shouldn't sweep up
unrelated dirty files.

## Step 1: Refresh the review doc before deciding what to reply to

Even though we'll run `pull_reviews.py` again in Step 5 to mirror our
own replies, run it **first** so the "unreplied threads" analysis is
against the latest GitHub state:

```
scripts/pull_reviews.py <N>
```

If the script reports new items, read them — a reviewer may have
posted push-back or new findings since the last round, which changes
which threads you should reply to and what the replies should say.

## Step 2: Identify unreplied threads

Read `doc/reviews/review-NNNNN.md`. A **thread** is a top-level inline
comment header plus any replies underneath it, terminating at the next
`### ` or end of file. Thread headers look like:

- `### {user} on [\`{path}\`](...)` (file-level or outdated-diff comments
  where the GitHub API returned `line: null`)
- `### {user} on [\`{path}:{line}\`](...)` (normal inline comments)

A thread is **unreplied** if no `↳` reply in it is authored by a non-bot
account — i.e., not `Copilot`, not `copilot-pull-request-reviewer[bot]`,
not `claude[bot]`. A bot replying to its own comment does not close the
thread.

Skip top-level review-body sections (`### {user} — {state}` with no `on
[\`path\`]`) — they don't take threaded replies via the review-comment
API.

## Step 3: Compose replies

For each unreplied thread, write 1–3 sentences that do exactly one of:

- **Acknowledge a fix**: name what changed. Example: `Fixed — switched
  to set-membership de-dup per your suggestion.` Citing the fix commit
  SHA is optional; naming the change is what matters.
- **Accept as follow-up**: explain the deferral and where it's tracked.
  Example: `Deferred — tracked as a follow-up in the PR description.
  Not blocking this change.`
- **Push back with reasoning**: in one sentence, no hedging. Example:
  `The existing behavior is intentional: callers downstream depend on
  the current return shape.`

Rules:
- Don't thank the reviewer. Don't apologize. Don't repeat the comment.
- Don't post a reply that just says "done" — name what was done.
- One reply per thread, not per comment. If a thread already has a
  human-authored reply buried, don't post another.

## Step 4: Post the replies

For each composed reply:

```
scripts/reply_review.py <PR> <in_reply_to_id> "<body>"
```

`<in_reply_to_id>` is the `gh-id` from the top-level comment's
`<!-- gh-id: NNNNN -->` marker in the review file. Pass `-` as the
body to read from stdin for long or multi-line replies:

```
scripts/reply_review.py 5 3098547699 - <<'EOF'
Fixed — ...
EOF
```

## Step 5: Mirror the replies back into the review doc

```
scripts/pull_reviews.py <N>
```

The script appends the replies you just posted (plus anything else new)
to `review-NNNNN.md` via set-membership de-dup — safe to re-run.

## Step 6: Fold the doc update into the round's fix commit

```
git add doc/reviews/review-NNNNN.md
git commit --amend --no-edit
```

This works because Step 0b confirmed the fix commit is unpushed. No
force-push. If the amend is a no-op (Step 5 appended nothing new —
unlikely but possible if replies were trivially duplicated), just skip
the amend.

## Step 7: Report and hand off to the user

Print a one-paragraph summary:

- Number of threads replied to
- Whether the fix commit was amended (yes → mirror folded in; no → no
  new doc content to fold)
- **Next step for the user:** `git push` (or `git push -u origin
  <branch>` if this is the first push for the branch)

Do **not** push. The user runs the push explicitly as the last step.

---

## No-op round (all push-back, no code fix)

If every thread got push-back and no code changed this round, there is
**no fix commit** to amend into. In that case:

- Step 0b sees no unpushed commits and unreplied threads → refuses,
  per the preconditions. This is intentional: without a fix commit,
  the only ways to land the reply-mirror are a standalone `doc:`
  commit (wasteful) or leaving it uncommitted until the next round.
  The latter is fine — a later round's fix commit will fold in all
  pending doc deltas via its own `/reply-reviews`.

- Recovery: post the push-back replies manually via
  `scripts/reply_review.py` (the GitHub threads remain the canonical
  record), and let the next substantive round's `/reply-reviews` catch
  up the mirror.

## Notes

- **Bots don't count as human replies.** A Copilot `↳ Copilot` reply
  under its own comment doesn't satisfy "replied". Only a human-
  authored reply closes the thread.
- **One reply per thread, not per comment.** If a thread already has
  a human reply, don't post another.
- **The review file rides with the fix commit.** Never as a standalone
  `doc: update review-NNNNN.md` commit — that was the ordering bug this
  command eliminates.
