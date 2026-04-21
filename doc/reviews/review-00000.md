# PR #00000 — placeholder

This file is a deliberate sentinel, not a real review. **Do not delete
it. Do not write review content into it.**

GitHub numbers pull requests starting at **1**, so `review-00000.md`
will never correspond to an actual PR. The file exists to reserve the
`00000` slot and pre-empt two recurring agent mistakes:

1. **Inventing a placeholder filename.** `/sprint-review` names review
   files `review-NNNNN.md` from the start, computing `NNNNN` via
   `scripts/next_pr_number.sh` before the PR is opened (the script
   queries the repo's highest issue/PR number via `gh api` and adds
   one). Without a visible `review-NNNNN.md` in the directory, an
   agent that doesn't find the script might invent an ad-hoc name
   like `review-draft.md` or `review-pending.md` instead of running
   it. This file exists so the convention is visibly anchored.

2. **Off-by-one numbering on the first real review.** The first PR in
   a repo is `#1`, not `#0`. An agent skimming the directory for the
   highest-numbered file and adding one would pick `00001` correctly;
   one skimming for "the next slot" and assuming `00000` is unused
   would collide with this file. Keeping `review-00000.md` present
   makes the convention explicit: `00000` is reserved, real reviews
   start at `00001`.
