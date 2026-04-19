# PR #0000 — placeholder

This file is a deliberate sentinel, not a real review.

GitHub numbers pull requests starting at **1**, so `review-0000.md` will
never correspond to an actual PR. It exists to anchor the numbering
scheme and pre-empt two recurring agent mistakes:

1. **Misreading `0000` as "the next available slot".** When
   `sprint-review` runs before a PR has been opened, the skill falls back
   to `0000` as a placeholder and expects the agent to rename the file
   to `review-NNNN.md` once the PR number is known. Without this
   sentinel on disk, agents see no `review-0000.md`, assume the
   placeholder convention is unused, and invent their own (e.g.
   `review-draft.md`, `review-pending.md`).

2. **Off-by-one numbering on the first real review.** The first PR in a
   repo is `#1`, not `#0`. An agent skimming the directory for "the
   highest existing review" and adding one would pick `0001` correctly;
   one skimming for "the next slot after the last placeholder" might
   pick `0000` and collide with this file. Keeping `review-0000.md`
   present makes the convention explicit: `0000` is reserved, real
   reviews start at `0001`.

Do not delete this file. Do not write review content into it.
