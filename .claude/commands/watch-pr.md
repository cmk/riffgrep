---
description: Poll a PR for new review comments; auto-address the trivially-clear ones, run the full /reply-reviews flow, and stop before push. Designed for /loop-driven automation (e.g., `/loop 10m /watch-pr 17`). Never pushes. Refuses if a prior round's fix commit is still unpushed.
argument-hint: <pr-number>
---

# /watch-pr — Automated review-round driver (no push)

One tick of the polling loop: check for new reviewer activity, handle
the clear-cut items, leave everything else for the user, stop before
push.

Target PR: `$ARGUMENTS`

**Never push.** The user reviews the fix commit and pushes manually.
This is deliberate — auto-fixing is medium-risk (a misread nit can
ship wrong code); auto-pushing would remove the one remaining
safety net.

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

### 0d. No pending push?

```
git fetch --quiet origin "$branch" || true
unpushed=$(git log "origin/$branch..HEAD" --oneline)
```

If `unpushed` is non-empty: a previous round's fix commit is waiting
on the user to push. Exit with one line: `holding: fix commit <sha>
pending push`. Do not poll, do not fix, do not reply. The next loop
tick will check again.

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
- **push-back** — suggestion conflicts with repo conventions (CLAUDE.md),
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

## Step 3: Apply the auto-fix items

For each **auto-fix** item:

1. Read the surrounding code so you understand the context, not just
   the line.
2. Make the edit. Stay strictly within the scope of the comment — no
   adjacent cleanup, no "while I'm here" changes.
3. If multiple auto-fix items touch the same file, batch the edits
   before running tests (faster feedback).

Once all auto-fix edits are staged:

```
git add <edited files>
git commit -m "fix: Address review feedback on PR #<N>"
```

The pre-commit hook runs `cargo fmt --check`, `scripts/check-pii.sh`,
`cargo test --workspace`, and `cargo clippy --all-targets -- -D
warnings`. If it fails:

- Read the failure. If it's a single fix causing the failure, revert
  that specific edit and reclassify the corresponding thread as
  **push-back** with an explanation of why the suggestion breaks the
  build.
- Retry the commit.
- If the commit fails twice: abort the round. Revert all uncommitted
  edits (`git checkout -- <files>`), log what went wrong, exit. The
  user will investigate on the next loop tick.

## Step 4: Complete the round (inline /reply-reviews)

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

# Fold the doc update into the fix commit
git add doc/reviews/review-NNNNN.md
git commit --amend --no-edit
```

The amend is safe because Step 0d confirmed the fix commit is unpushed.

If Step 3 made no commit (all items were push-back / defer / ask) the
reply-post still happens, but there's no commit to amend. In that case
leave `review-NNNNN.md` modified-but-uncommitted — the next round's fix
commit will fold it in (matching `/pull-reviews`'s standalone-use
contract).

## Step 5: Report

Print a structured summary, ≤ 15 lines:

```
watch-pr PR #<N> — round complete
  auto-fixed:  <count>   (e.g., "unused import, typo in doc")
  pushed-back: <count>   (e.g., "proposed rename conflicts with crate boundary")
  deferred:    <count>
  needs you:   <count>   ← these stay open; read them
    - path:line — one-line summary
    - path:line — one-line summary
  fix commit:  <sha>     (or: "no commit — all push-back/defer")
  next step:   review the commit, then `git push`
```

Do not push. Do not start another round synchronously.

## Step 6: Schedule the next tick (or quit)

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
ticks that exited at Step 0d (holding), Step 1 (no new activity), or
Step 4 with nothing posted. Any tick that addressed activity resets
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
