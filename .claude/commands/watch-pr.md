---
description: Poll a PR for new review comments; auto-address the trivially-clear ones, run the full /reply-reviews flow, then push the round commit. Designed for /loop-driven automation (e.g., `/loop 10m /watch-pr 17`). Never merges — the user merges manually, which is the real safety gate.
argument-hint: <pr-number>
---

# /watch-pr — Automated review-round driver

One tick of the polling loop: check for new reviewer activity, handle
the clear-cut items, leave everything else for the user, push the
round commit, stop before merge.

Target PR: `$ARGUMENTS`

**Never merge.** The user reviews each PR before merging, and that's
the real safety gate. Pushing the round commit so CI re-runs and the
PR shows the latest replies is a normal step in the loop — not a
risk worth gating on, since the user can always revert or push more
fixes before deciding to merge.

Designed for `/loop 10m /watch-pr <N>` (or similar cadence). Each tick
runs one round or exits quickly with a heartbeat.

---

## Step 0: Preconditions (fail fast, no side effects)

### 0a. Determine the PR number

Use `$ARGUMENTS` if non-empty. Otherwise:
```
gh pr view --json number --jq .number
```
Abort if neither yields a number.

### 0b. On the right branch?

```
branch=$(git branch --show-current)
remote_branch=$(gh pr view <N> --json headRefName --jq .headRefName)
[ "$branch" = "$remote_branch" ] || abort
```

### 0c. Working tree clean?

`git status --porcelain` must be empty. If dirty, exit with a message —
a prior run may have left something mid-flight, or the user is working.
Do **not** touch it.

### 0d. Recover from a previous push failure

```
git fetch --quiet origin "$branch" || true
unpushed=$(git log "origin/$branch..HEAD" --oneline)
```

If `unpushed` is non-empty, a previous tick's `git push` (Step 5)
failed and the round commit is stranded locally. Try one push to
recover:

```
git push origin "$branch"
```

If the push succeeds, continue to Step 1 (the round is now
`gh_review`; subsequent activity is what we're polling for). If it
fails again, exit with one line: `paused at round_unpushed: push
failed (<error>)`. Do not poll, do not fix, do not reply — surface
the issue and let the user investigate. The next loop tick will
retry.

## Step 1: Poll for new activity

```
scripts/pull_reviews.py <N>
```

- If output is `PR #N: no new items (...)` → exit with a one-line
  heartbeat (`watch-pr: no new activity on PR #N`). Round is over.
- If output is `PR #N: appended K items -> <path>` → continue.

## Step 2: Triage the new items

Read the newly appended block in `doc/reviews/review-NNNNN.md`. For each
new top-level thread, classify it into exactly one bucket:

- **auto-fix** — reviewer's intent is unambiguous AND the change is
  local (one file, under ~20 lines), non-destructive (no API removal,
  no file deletion), and does not require cross-module reasoning.
  GitHub suggested-change blocks, Copilot-style inline fixes, clear
  "should be X" with X spelled out, typos, missing imports, doc
  corrections.
- **push-back** — suggestion conflicts with repo conventions (AGENTS.md),
  plan constraints, or is factually wrong. You can explain the
  disagreement in one sentence.
- **defer** — valid concern but out of scope for this PR (tracked as
  a follow-up).
- **ask** — architecture decision, subjective design call, or anything
  where you'd hesitate. Do **not** auto-fix. Do **not** auto-reply.
  Leave the thread untouched on GitHub; flag it in the final report.

**When in doubt, classify as ask.** A miscategorized auto-fix ships
wrong code; a miscategorized ask only delays a round by one loop tick
until the user resolves it.

Track two counters for the commit step:

```
auto_fix_count=0
reply_count=0

# Increment as integers while classifying threads:
# auto-fix thread:
auto_fix_count=$((auto_fix_count + 1))
reply_count=$((reply_count + 1))

# push-back or defer thread:
reply_count=$((reply_count + 1))
```

## Step 3: Apply the auto-fix items (no commit yet)

For each **auto-fix** item:

1. Read the surrounding code so you understand the context, not just
   the line.
2. Make the edit. Stay strictly within the scope of the comment — no
   adjacent cleanup, no "while I'm here" changes.
3. If multiple auto-fix items touch the same file, batch the edits
   before running tests (faster feedback).

**Do not commit yet.** The whole round (code fix + replies + mirrored
doc) is committed once in Step 4 as a single atomic commit. This
matches the FSM in `doc/workflow.md`: `items_pulled → round_unpushed`
is a single transition; there is no intermediate `fix_unpushed` state.

If no auto-fix items apply (all push-back / defer / ask), the working
tree stays clean and Step 4's commit will be `doc:`-prefixed (mirror
only).

## Step 4: Post replies, mirror, and commit the round atomically

For each **auto-fix**, **push-back**, and **defer** thread from Step 2,
compose a 1–3 sentence reply per the rules in
`.claude/commands/reply-reviews.md` (acknowledge / push back / defer).
Skip **ask** threads entirely — they stay unreplied on GitHub.

Then:

```
# Post replies
for each thread: scripts/reply_review.py <N> <in_reply_to_id> "<body>"

# Mirror replies back into the review doc
scripts/pull_reviews.py <N>

# Single atomic commit: code edits (if any) + mirrored replies.
# If every new item was `ask`, do not stage or keep the pull-only
# review-doc delta from Step 1; restore doc/reviews/ so the next tick
# still starts from a clean tree.
if [ "$auto_fix_count" -eq 0 ] && [ "$reply_count" -eq 0 ]; then
    git restore --source=HEAD --worktree -- doc/reviews
else
    git add -A
    if git diff --cached --quiet; then
        # Nothing staged — branch stays at gh_review. Step 5's
        # "no commit" report branch fires.
        :
    else
        # Pick prefix based on staged content:
        #   fix:  any staged change outside doc/reviews/*.md
        #   doc:  only doc/reviews/<file>.md changed (replies-only round)
        if git diff --cached --name-only | grep -Eqv '^doc/reviews/.*\.md$'; then
            commit_prefix=fix
        else
            commit_prefix=doc
        fi
        git commit -m "$commit_prefix: Address review feedback on PR #<N>"
    fi
fi
```

The pre-commit hook runs `cargo fmt --check`, `scripts/check-pii.sh`,
`cargo test --workspace`, and `cargo clippy --all-targets -- -D
warnings`. If it fails:

- Read the failure. If a specific auto-fix caused the breakage, revert
  that one edit and reclassify the corresponding thread as
  **push-back** with an explanation. Retry `git commit`.
- If the commit fails twice: abort the round. Replies are already on
  GitHub (Step 4's post loop is destructive); leave the working tree
  dirty so the next loop tick exits at Step 0c and surfaces the issue
  to the user. Do not `git checkout --` — that would discard the
  fixes the replies reference.

If `reply_review.py` fails partway through the post loop (network /
rate limit), abort before mirror+commit. Re-running this step is
clean: the next tick's Step 1 mirrors the already-posted replies, the
"unreplied threads" filter skips them, and the missing posts go
through. (Same as `/reply-reviews`'s Recovery section.)

## Step 5: Push the round commit

If Step 4 produced a commit (the working tree had staged changes —
either code edits or the mirrored reply doc), push it now:

```
git push origin "$branch"
```

This advances the branch to `gh_review` so CI re-runs against the
latest state and the reviewer (Copilot, the user) sees the replies
attached to the right tip. The user is still the merge gate — they
review the PR as a whole before invoking `gh pr merge` /
`scripts/safe_merge.sh`. A round commit reaching origin without
their merge is the normal mid-PR state, not a risk to gate on.

If Step 4 produced no commit (no auto-fix items AND no replies
posted — every item was `ask`), skip this step. There is nothing to
push. The branch stays at `gh_review` (its original state).

If `git push` fails (network, auth, non-fast-forward because someone
else pushed), exit with the error and leave the round commit local.
The next tick's Step 0d will see the unpushed commit and surface the
state to the user.

**Never merge.** `gh pr merge` is a manual user step.

## Step 6: Report

Print a structured summary, ≤ 15 lines. The heading names the FSM
state from `doc/workflow.md` so the read-out reflects what's actually
true on the wire.

If Step 5 pushed a commit:

```
watch-pr PR #<N> — round complete at gh_review (commit pushed)
  auto-fixed:  <count>   (e.g., "unused import, typo in doc")
  pushed-back: <count>   (e.g., "proposed rename conflicts with crate boundary")
  deferred:    <count>
  needs you:   <count>   ← these stay open; read them
    - path:line — one-line summary
    - path:line — one-line summary
  round commit: <sha>    (pushed — fix: or doc:)
  next step:   wait for CI + Copilot re-review, then merge with
               `gh pr merge` / `scripts/safe_merge.sh` when ready.
```

If Step 4 produced no commit (no auto-fix items AND no replies
posted — all items were `ask`):

```
watch-pr PR #<N> — round complete at gh_review (no commit needed)
  auto-fixed:  0
  pushed-back: 0
  deferred:    0
  needs you:   <count>
    - path:line — one-line summary
  round commit: none — all items classified as `ask`, no replies posted
  next step:   PR is mergeable when reviewers stop posting.
```

If Step 5's `git push` failed:

```
watch-pr PR #<N> — paused at round_unpushed (push failed)
  auto-fixed:  <count>
  ...
  round commit: <sha>    (LOCAL ONLY — push failed: <error>)
  next step:   investigate the push failure, then `git push` manually.
```

Do not start another round synchronously.

## Step 7: Schedule the next tick (or quit)

This command is designed for `/loop /watch-pr <N>` **without an
interval** — dynamic/self-paced mode — so the schedule below actually
takes effect. If invoked with a fixed `/loop 10m …`, the runtime
overrides the schedule; see "Fixed vs. dynamic" below.

### Backoff schedule

```
BACKOFF_MINUTES=(5 5 5 10 10)   # quit on the 6th quiet tick (after the 5 slots are exhausted)
```

State is a single integer in `.watch-pr/pr-<N>.count` (gitignored,
created on first use). It counts consecutive **unproductive** ticks —
ticks that exited at Step 0d (push_failed, after recovery push also
failed), Step 1 (no new activity), or Step 4 with nothing posted.
Any tick that addressed activity (and pushed successfully) resets
the counter to 0.

### Decision

Read the counter (default 0 if the file doesn't exist):

```
count=$(cat .watch-pr/pr-<N>.count 2>/dev/null || echo 0)
```

Classify this tick's outcome:
- **activity** — Step 2 triage saw new items, any bucket (even if all
  were ask/push-back): `new=0`
- **unproductive** — early-exit at Step 0d, or Step 1's "no new items":
  `new=$((count + 1))`

Then decide the next action:

```
BACKOFF_MINUTES=(5 5 5 10 10)
N_SLOTS=${#BACKOFF_MINUTES[@]}     # 5

if [ "$new" -gt "$N_SLOTS" ]; then
  # Out of schedule. End the loop.
  rm -f ".watch-pr/pr-<N>.count"
  echo "loop ending: $((N_SLOTS + 1)) consecutive quiet ticks (backoff exhausted after $(( 5+5+5+10+10 ))min)"
  # DO NOT call ScheduleWakeup.
else
  if [ "$new" -eq 0 ]; then
    delay_min=${BACKOFF_MINUTES[0]}      # post-activity: back to 5m
  else
    delay_min=${BACKOFF_MINUTES[$((new - 1))]}
  fi
  mkdir -p .watch-pr
  echo "$new" > ".watch-pr/pr-<N>.count"
  # Call ScheduleWakeup:
  #   delaySeconds = delay_min * 60
  #   prompt       = "/watch-pr <N>"
  #   reason       = concise status, e.g.:
  #                  "PR #<N> quiet <new>/<N_SLOTS>, next in <delay_min>min"
  #                  or "PR #<N> addressed <K> comments, next in 5min"
fi
```

Then emit the `ScheduleWakeup` tool call (if not quitting).

### Resulting cadence

Assuming no activity from the moment of `/loop /watch-pr <N>`:

| Tick | Elapsed | Outcome     | Next wait |
|------|---------|-------------|-----------|
| 0    | 0m      | quiet       | 5m        |
| 1    | 5m      | quiet       | 5m        |
| 2    | 10m     | quiet       | 5m        |
| 3    | 15m     | quiet       | 10m       |
| 4    | 25m     | quiet       | 10m       |
| 5    | 35m     | quiet       | **quit**  |

Any activity tick resets the counter to 0, so a burst mid-backoff
restores the 5-minute cadence. The 35-minute ceiling is a cap on
"total silence before I go home"; it does not cap how long the loop
can run if reviewers are active.

### Fixed vs. dynamic

- `/loop /watch-pr <N>` — **dynamic**, backoff+quit as above.
  Recommended for Copilot-on-push rounds where silence means "review
  is done, go home."
- `/loop 5m /watch-pr <N>` — fixed 5m, no backoff, no auto-quit.
  Use if you want the loop to run indefinitely until you kill it
  (e.g., waiting on a slow human reviewer).
- `/loop 10m /watch-pr <N>` — fixed 10m. Same as above, lazier cadence.
