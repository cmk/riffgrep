# Workflow State Diagrams

Visual reference for the review-round and `/watch-pr` workflows defined
in `../AGENTS.md`. The prose specs there are authoritative; these
diagrams exist to make the state transitions easier to eyeball when
debugging an unexpected situation — a stuck fix commit, a loop that
won't quit, an ordering question about when to run which command.

GitHub renders mermaid in markdown natively, so both diagrams below
show up as real graphs on the PR page.

## Review round lifecycle

One sprint from `main` through merge, covering Tier 1 (local) review,
Tier 2 (GitHub) rounds, and the fix-edits → reply → mirror → commit →
push motion that `/reply-reviews` enforces.

```mermaid
stateDiagram-v2
    [*] --> main_clean
    main_clean --> on_branch: git worktree add / git switch -c
    on_branch --> plan_committed: write plan + `plan:` commit
    plan_committed --> impl_green: TDD loop (tests + feat/fix commits)
    impl_green --> plan_finalized: append Deferred + Review, draft PR body
    plan_finalized --> local_reviewed: /sprint-review or scripts/local_review.sh
    local_reviewed --> impl_green: must-fix items surfaced
    local_reviewed --> pushed: clean, git push
    pushed --> gh_review: CI runs + reviewers post
    gh_review --> items_pulled: /pull-reviews (scripts/pull_reviews.py)
    items_pulled --> round_unpushed: edit working tree + /reply-reviews (post + mirror + atomic commit)
    round_unpushed --> gh_review: git push (mandatory before merge)
    gh_review --> merged: no more items, scripts/safe_merge.sh (after push check)
    merged --> [*]
```

**Legend:**
- `round_unpushed` is the load-bearing state — one atomic commit
  containing both the code fix and the mirrored reply doc, sitting
  unpushed on the local branch. `/reply-reviews` produces it in one
  flow: refresh via `scripts/pull_reviews.py` → identify unreplied
  threads → post replies via `scripts/reply_review.py` → refresh
  again to mirror via `scripts/pull_reviews.py` → `git add -A &&
  git commit`. There is no `--amend` step and no prior fix commit
  to amend onto; replies and code arrive together by construction.
- The `gh_review → items_pulled → round_unpushed → gh_review` cycle
  runs once per review round. The transition out of `round_unpushed`
  is `git push` — that's the only way to advance to mergeability.
- **Never merge from `round_unpushed`.** There is no
  `round_unpushed → merged` edge in the FSM — only `gh_review →
  merged`. `gh pr merge` is GitHub-side and doesn't see local state,
  so a merge with an unpushed round silently drops the local commit.
  Use `scripts/safe_merge.sh <pr-args>` instead of `gh pr merge` —
  it refuses to invoke the merge while the local branch is ahead of
  origin. (Equivalent local check: `git log origin/<branch>..HEAD
  --oneline` must be empty.)
- `local_reviewed → impl_green` is the must-fix loop-back. The fix
  commits stay on the same branch; re-append any new Deferred/Review
  notes, then re-run the local review transition (`/sprint-review` for
  Claude Code, `scripts/local_review.sh` for Codex/shell) against the
  new tip.
- `plan_finalized` sits deliberately *before* `local_reviewed`: the
  reviewer reads the plan as context and should see its final form,
  including what was intentionally cut and why. It's also when
  `doc/reviews/review-NNNNN.md` is created with the PR body under
  `## Summary`. Committing the description pre-push is what lets a
  silent PR merge without an extra round-trip — `gh pr create`
  feeds GitHub a direct copy via `scripts/extract_pr_body.sh`.

**Recovery: stranded round commit after merge from `round_unpushed`.**

If a merge happened while the round was at `round_unpushed` (i.e.
`safe_merge.sh` was bypassed and `gh pr merge` was used directly) and
the local commit got stranded, the round-2 work isn't lost — it's
sitting on the local feature branch's tip. Don't open a tiny
standalone PR for it; per repo convention, fold the stranded commit
into the next plan branch's first commit:

```
# On the next plan branch, after the plan: commit:
git cherry-pick <stranded-sha>
# Squash into the first feat/fix commit you make on this branch,
# OR keep as a separate `fix:` commit if the change stands alone.
```

The previously-posted GitHub replies remain accurate (they reference
the right SHAs at the time of posting). The next PR's review file
should reference the prior PR's `gh-id` URLs in a `### History`
section so the chain isn't orphaned.

**Recovery: partial reply-post failure mid-`/reply-reviews`.**

If `scripts/reply_review.py` fails partway through the post loop
(network, rate limit, auth), `/reply-reviews` aborts before the
mirror+commit step. Some replies are on GitHub, some aren't; the
working tree still has the uncommitted code edits but no mirrored
doc changes. Recovery is a re-run of `/reply-reviews`:

1. Step 1's `pull_reviews.py` mirrors the already-posted replies into
   the doc.
2. Step 2's "unreplied threads" filter skips threads with mirrored
   replies — so we only post the missing ones.
3. The rest of the run completes normally.

`reply_review.py` is **not** idempotent server-side — calling it
twice with the same `in_reply_to_id` posts twice. Idempotency comes
from the "skip already-replied threads" filter, which depends on the
mirror happening *before* the post loop. Don't bypass Step 1.

## `/watch-pr` dynamic-mode loop

The `/loop /watch-pr <N>` self-pacing loop, with its 5/5/5/10/10-minute
backoff and auto-quit on the 6th consecutive quiet tick (after the
5-slot backoff is exhausted). Counter state lives in
`.watch-pr/pr-<N>.count` (gitignored).

```mermaid
stateDiagram-v2
    [*] --> scheduled
    scheduled --> tick: ScheduleWakeup fires
    tick --> push_failed: unpushed commit (prior tick's push errored)
    tick --> polling: clean, ready to poll
    push_failed --> reschedule_quiet: count+1
    polling --> no_activity: no new items
    polling --> triaging: new items
    no_activity --> reschedule_quiet: count+1
    triaging --> auto_fixing: auto-fix items present
    triaging --> replying: no auto-fix, replies only
    auto_fixing --> replying
    replying --> pushing: round commit ready
    pushing --> reschedule_active: push OK, count=0
    pushing --> reschedule_quiet: push failed (network / non-ff)
    reschedule_quiet --> quitting: count > 5
    reschedule_quiet --> scheduled: count <= 5, delay per backoff
    reschedule_active --> scheduled: 5m delay
    quitting --> [*]: rm counter, no ScheduleWakeup
```

**Legend:**
- `push_failed` is the recovery state when a previous tick's
  `git push` errored (network, non-fast-forward). The next tick
  surfaces the unpushed commit to the user without polling further.
  It counts as a quiet tick — successive failures still trigger
  backoff and quit.
- `reschedule_quiet` reads the backoff table: count 1→5m, 2→5m, 3→5m,
  4→10m, 5→10m, >5 → quit (total silence budget ≈ 35 minutes).
- Any `reschedule_active` edge resets the counter to 0, so a burst of
  review activity mid-backoff restores the 5-minute cadence.
- `quitting` terminates the dynamic loop: the state file is removed
  and `ScheduleWakeup` is deliberately *not* called. A fixed-interval
  loop (`/loop 5m /watch-pr <N>`) has no backoff and no quit — it
  runs until the user kills it.

## When a diagram disagrees with AGENTS.md

AGENTS.md wins. These are derived views; re-draw them when the
workflow prose changes. A lagging diagram is worse than no diagram.
