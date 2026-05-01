---
description: Finalize a PR review round before push. Apply fix edits to the working tree first, then run this command — it posts replies, mirrors them back into review-NNNNN.md via pull_reviews.py, and produces ONE atomic commit containing both the code fix and the mirrored reply doc. One push delivers the whole round. Refuses to run if the branch already has unpushed commits (which would create a bisect-bisecting two-commit round).
argument-hint: <pr-number>
---

# /reply-reviews — Finalize a Review Round (Before Push)

Apply fix edits to the working tree, then run this command. It posts
replies to each unresolved PR review thread, mirrors them back into
`doc/reviews/review-NNNNN.md`, and produces **one atomic commit**
containing both the code fix and the mirrored reply doc. Stops before
`git push` — the user runs that explicitly as the last step, so one
push delivers code + replies + review doc.

This ordering — **fix-edits (uncommitted) → reply → mirror → atomic
commit → push** — is deliberate. The `replies_amended` state from the
older flow is gone: replies and code are bundled into a single commit
by construction, so the only state between "round started" and
"round pushed" is `round_unpushed`. There is no `--amend` step and no
opportunity for replies and fix to diverge.

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

### 0b. Branch must be at `gh_review` (no unpushed commits)

Per `doc/workflow.md`, this command runs on the
`gh_review → items_pulled → round_unpushed` arrow — starting from
`gh_review`, which means the local branch is at-or-behind origin.
A pre-existing unpushed commit means a previous round didn't push,
and bundling another round on top would produce a two-commit round
that's harder to bisect and doesn't fit the FSM.

```
branch=$(git branch --show-current)
git fetch --quiet origin "$branch" || true
unpushed=$(git log "origin/$branch..HEAD" --oneline)
```

- If `unpushed` is non-empty: **refuse**. Print:
  > Branch '$branch' has unpushed commits. Per doc/workflow.md, a new
  > review round starts from `gh_review` — push the existing commit
  > first (`git push`), then re-run `/reply-reviews`. If the existing
  > commit is itself a stale round-1, just push it.

  Do not post replies.

### 0c. Working tree sanity

Run `git status`. The working tree may be **dirty with the round's fix
edits** — that's expected and is exactly what this command bundles
into the round commit. No constraint on which paths can be dirty;
trust the user not to bundle unrelated changes. The pre-commit hook
(Step 6) will catch test/clippy/PII regressions.

If the working tree is **clean**, this is a no-op-code round (all
push-back / defer). Proceed normally — the round commit will be a
`doc:`-prefixed commit containing only the mirrored reply doc, which
is fine and does not break the FSM.

## Step 1: Refresh the review doc before deciding what to reply to

Even though we'll run `pull_reviews.py` again in Step 5 to mirror our
own replies, run it **first** so the "unreplied threads" analysis is
against the latest GitHub state and so partial-failure recovery
(see Recovery section below) works correctly:

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
  to set-membership de-dup per your suggestion.` Citing the eventual
  round commit SHA is fine but optional (you don't have it yet at this
  step); naming the change is what matters.
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

If any post fails (network, rate limit, auth): **abort before mirror
and commit**. The working tree still has the uncommitted code edits.
See the Recovery section for the re-run shape — it's clean because
Step 1 + Step 2's filter dedupes already-posted threads.

## Step 5: Mirror the replies back into the review doc

```
scripts/pull_reviews.py <N>
```

The script appends the replies you just posted (plus anything else new)
to `review-NNNNN.md` via set-membership de-dup — safe to re-run.

## Step 6: Atomic round commit

Stage everything in the working tree (code edits + mirrored doc) and
make ONE commit, but **only if there are staged changes** — an
all-`ask` round with no doc delta produces nothing to commit:

```
git add -A
if git diff --cached --quiet; then
    # Nothing staged — no replies posted (e.g., all `ask`) and no
    # doc delta from Step 5. Branch stays at gh_review. Step 7's
    # "no commit" report branch fires.
    :
else
    # Pick the prefix that matches the staged content:
    #   fix:  any staged change outside doc/reviews/*.md
    #   doc:  only doc/reviews/<file>.md changed (replies-only round)
    if git diff --cached --name-only | grep -qv '^doc/reviews/.*\.md$'; then
        prefix=fix
    else
        prefix=doc
    fi
    git commit -m "$prefix: Address review feedback on PR #<N>"
fi
```

The pre-commit hook runs `cargo fmt --check`, `scripts/check-pii.sh`,
`cargo test --workspace`, and `cargo clippy --all-targets -- -D
warnings`. If it fails:

- Read the failure. Fix the code or revert the offending edit.
- Retry `git commit` — staging is preserved.
- Replies are already on GitHub (Step 4 succeeded). Don't try to
  un-post; just commit when ready.

There is no `--amend` step. The commit either succeeds (whole round
captured, state advances to `round_unpushed`) or:

- The pre-commit hook fails: working tree still dirty, replies on
  GitHub but no mirror committed yet — re-run `/reply-reviews` to
  redo Step 5+6.
- Nothing was staged: branch stays at `gh_review`. No state change.

## Step 7: Report and hand off to the user

Print a one-paragraph summary that **names the FSM state from
`doc/workflow.md`** so the read-out reflects what's actually true on
the wire. Two terminal shapes:

**If Step 6 made a commit:**

- Number of threads replied to
- Round commit SHA
- **State:** `round_unpushed` (mid-cycle, NOT mergeable). The merge
  transition starts from `gh_review`, which requires push.
- **Next step for the user:** `git push` (or `git push -u origin
  <branch>` if this is the first push for the branch). Then merge
  via `scripts/safe_merge.sh <pr-args>` rather than `gh pr merge` —
  the wrapper refuses to invoke the merge while the local branch is
  ahead of origin, which is the only protection against silently
  dropping an unpushed round.

**If Step 6 made no commit** (all-`ask` round or every reply was
already a duplicate of an existing one — no code delta, no doc
delta):

- Number of threads replied to (may be zero)
- "No commit — nothing staged after Step 5"
- **State:** `gh_review` (no change — branch is exactly where it
  started). Mergeable when reviewers stop posting.
- **Next step for the user:** read the review file for any `ask`
  threads that need their judgment.

Do **not** push. The user runs the push explicitly as the last step.

---

## Recovery: partial reply-post failure

If `scripts/reply_review.py` fails partway through Step 4, abort the
run before Step 5. Working tree has the code edits but no mirrored
replies. Some replies are on GitHub, some aren't. Re-run
`/reply-reviews`:

1. Step 1's `pull_reviews.py` mirrors the already-posted replies into
   the doc.
2. Step 2's "unreplied threads" filter skips threads that now have
   `↳ {user}` mirrored replies — only the unposted threads remain.
3. Steps 3–5 cover the missing posts and re-mirror.
4. Step 6 commits everything.

`reply_review.py` is **not** idempotent server-side — calling it twice
with the same `in_reply_to_id` posts twice. Idempotency comes from
the Step 1 mirror happening *before* Step 2's filter; don't bypass
Step 1 on retry.

## Recovery: pre-commit hook fails in Step 6

Replies are on GitHub. Working tree has code edits + mirrored doc,
all uncommitted. Fix the test/clippy/PII issue, then either:

- **`git commit` manually**: same staging, same message. The
  pre-commit hook re-runs.
- **Re-run `/reply-reviews`**: Step 0b sees no unpushed commits, Step
  1+2 see no unreplied threads (all already mirrored), Steps 3–5 are
  no-ops, Step 6 retries the commit.

Both paths converge on the same `round_unpushed` state.

## Notes

- **Bots don't count as human replies.** A Copilot `↳ Copilot` reply
  under its own comment doesn't satisfy "replied". Only a human-
  authored reply closes the thread.
- **One reply per thread, not per comment.** If a thread already has
  a human reply, don't post another.
- **The whole round is one commit.** Never two commits per round
  (a `fix:` then a `doc:`) — that defeats the atomicity guarantee
  this command exists to provide.
