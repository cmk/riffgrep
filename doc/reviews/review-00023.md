# PR #23 — Sync template-rust workflow updates

<!-- gh-id: 3116142527 -->
### Copilot on [`CLAUDE.md:288`](https://github.com/cmk/riffgrep/pull/23#discussion_r3116142527) (2026-04-21 08:40 UTC)

The pre-commit hook description says CI mirrors the `cargo fmt --check` step as `continue-on-error`, but `.github/workflows/ci.yml` currently runs `cargo fmt --all -- --check` as a normal (failing) step. Either adjust CI to be warn-only, or update this doc to reflect that CI will fail on formatting diffs even though the local hook is warn-only.
```suggestion
   files need formatting but does not block the commit. CI still runs
   the formatting check as a normal failing step, so formatting diffs
   will fail CI even though the local hook only warns. Run
   `cargo fmt --all` to fix.
```

<!-- gh-id: 3116142576 -->
### Copilot on [`CLAUDE.md:204`](https://github.com/cmk/riffgrep/pull/23#discussion_r3116142576) (2026-04-21 08:40 UTC)

This workflow guidance references `.claude/settings.local.json` as the source of a “deny list” for force-pushes, but that file isn’t present in the repository (only `.claude/settings.json` exists). If the restriction is enforced elsewhere (e.g., branch protection or an agent policy), please name that mechanism here; otherwise add the referenced config (or remove the reference) so readers aren’t sent to a non-existent file.
```suggestion
`.claude/settings.json`'s deny list). `/reply-reviews` enforces
```

<!-- gh-id: 3116142624 -->
### Copilot on [`doc/workflow.md:55`](https://github.com/cmk/riffgrep/pull/23#discussion_r3116142624) (2026-04-21 08:40 UTC)

The text says the loop “auto-quit after 5 consecutive quiet ticks”, but the diagram/legend uses `count > 5` / “5→10m, >5→quit”, which implies quitting on the 6th quiet tick if `count` starts at 0. Please align the prose with the threshold (or adjust the threshold to match the stated 5-tick quit).
```suggestion
backoff and auto-quit on the 6th consecutive quiet tick. Counter state
```

<!-- gh-id: 4146284776 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-21 08:40 UTC](https://github.com/cmk/riffgrep/pull/23#pullrequestreview-4146284776))

## Pull request overview

Ports the workflow/review infrastructure from `template-rust` into riffgrep to standardize the repo’s automation and documentation around review rounds, pre-commit gates, and PR-numbered review artifacts.

**Changes:**
- Adds workflow helper scripts (`check-pii.sh`, `next_pr_number.sh`) and updates the pre-commit hook to run fmt (warn-only), PII scan, tests, and clippy `-D warnings`.
- Migrates Claude workflow docs from `.claude/skills/` to `.claude/commands/` and adds `/watch-pr` + updates `/reply-reviews` behavior.
- Widens review-file naming from 4 to 5 digits (adds `review-00000.md` sentinel and updates scripts/docs to write `review-NNNNN.md`).

### Reviewed changes

Copilot reviewed 15 out of 34 changed files in this pull request and generated 4 comments.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| scripts/tests/test_codebook_rust_compat.py | Updates follow-up reference to 5-digit review filename. |
| scripts/pull_reviews.py | Writes review output to `review-{pr:05d}.md` and updates help/docstring accordingly. |
| scripts/next_pr_number.sh | New helper to predict the next PR/issue number via `gh api`. |
| scripts/check-pii.sh | New staged-diff PII/secret scanner with allow-list support. |
| doc/workflow.md | New mermaid state diagrams for review rounds and `/watch-pr` loop. |
| doc/reviews/review-00022.md | Renamed/backfilled review artifact at 5-digit padding. |
| doc/reviews/review-00020.md | Renamed/backfilled review artifact at 5-digit padding. |
| doc/reviews/review-00018.md | Renamed/backfilled review artifact at 5-digit padding. |
| doc/reviews/review-00017.md | Renamed/backfilled review artifact at 5-digit padding. |
| doc/reviews/review-00016.md | Renamed/backfilled review artifact at 5-digit padding. |
| doc/reviews/review-00014.md | Renamed/backfilled review artifact at 5-digit padding. |
| doc/reviews/review-00013.md | Renamed/backfilled review artifact at 5-digit padding. |
| doc/reviews/review-00012.md | Renamed/backfilled review artifact at 5-digit padding. |
| doc/reviews/review-00011.md | Renamed/backfilled review artifact at 5-digit padding. |
| doc/reviews/review-00007.md | Renamed/backfilled review artifact at 5-digit padding. |
| doc/reviews/review-00006.md | Renamed/backfilled review artifact at 5-digit padding. |
| doc/reviews/review-00005.md | Renamed/backfilled review artifact at 5-digit padding. |
| doc/reviews/review-00004.md | Renamed/backfilled review artifact at 5-digit padding. |
| doc/reviews/review-00003.md | Renamed/backfilled review artifact at 5-digit padding. |
| doc/reviews/review-00002.md | Renamed/backfilled review artifact at 5-digit padding. |
| doc/reviews/review-00001.md | Renamed/backfilled review artifact at 5-digit padding. |
| doc/reviews/review-00000.md | Adds 5-digit sentinel review file reserving `00000`. |
| doc/reviews/review-0000.md | Removes prior 4-digit sentinel file. |
| doc/plans/plan-2026-04-18-01.md | Updates references to `review-00010.md` (5-digit). |
| CLAUDE.md | Updates workflow/TDD/review instructions for 5-digit review files, new commands, and pre-commit chain. |
| .gitignore | Ignores `.watch-pr/` state directory. |
| .claude/skills/reply-reviews.md | Removes legacy skills-based `/reply-reviews` doc. |
| .claude/settings.json | Updates pre-commit hook chain (fmt warn-only → PII → tests → clippy `-D warnings`). |
| .claude/commands/watch-pr.md | Adds `/watch-pr` command documentation. |
| .claude/commands/sprint-review.md | Updates `/sprint-review` command doc and 5-digit review file naming instructions. |
| .claude/commands/reply-reviews.md | Adds new `/reply-reviews` command doc (post → mirror → amend → push ordering). |
| .claude/commands/pull-reviews.md | Updates `/pull-reviews` command doc for 5-digit filenames and new workflow rhythm. |
</details>



<details>
<summary>Comments suppressed due to low confidence (2)</summary>

**.claude/commands/sprint-review.md:101**
* These calibration/examples appear to be leftover from another repository (e.g., referencing a `device = ["dep:tokio", ...]` feature gate and a `driver-motu` crate). Since those don’t exist in this repo, they’re misleading for reviewers using `/sprint-review`. Replace these examples with riffgrep-relevant ones (e.g., marker/playback FSM properties, embedding schema migrations, or workflow/BEXT writer invariants).
**.claude/commands/sprint-review.md:188**
* This section still refers to concepts that don’t exist in this repo (e.g., “driver crates”, `project_core::testing::fixture_or_skip!`, and TCP/NRPN examples). That makes the generated reviewer prompt inaccurate and can cause false positives/irrelevant feedback. Update these bullets/examples to match riffgrep’s actual code layout and fixture-gating pattern (the convention in CLAUDE.md is “return early when fixture absent”).
</details>



<!-- gh-id: 3116142674 -->
### Copilot on [`.claude/commands/reply-reviews.md:17`](https://github.com/cmk/riffgrep/pull/23#discussion_r3116142674) (2026-04-21 08:40 UTC)

This references `.claude/settings.local.json` as enforcing a force-push deny list, but that file isn’t present in the repo. Either add/track the referenced config (if it’s meant to be shared), or rephrase to the actual enforcement mechanism (e.g., branch protection) so users aren’t pointed at a missing file.
```suggestion
force-push that this workflow is designed to avoid.
```

<!-- gh-id: 3116239810 -->
#### ↳ cmk ([2026-04-21 08:57 UTC](https://github.com/cmk/riffgrep/pull/23#discussion_r3116239810))

Fixed in ccaceed — riffgrep's CI runs `cargo fmt --all -- --check` as a normal failing step, so the 'continue-on-error' claim was wrong. Reworded to: 'CI still runs `cargo fmt --all -- --check` as a normal, failing step, so formatting diffs will fail CI even though the local hook only warns.' That matches the actual ci.yml.

<!-- gh-id: 3116240031 -->
#### ↳ cmk ([2026-04-21 08:57 UTC](https://github.com/cmk/riffgrep/pull/23#discussion_r3116240031))

Fixed in ccaceed — `.claude/settings.local.json` is gitignored globally (`~/.config/git/ignore`) and never trackable in any repo, so naming it was a broken pointer. Rephrased to describe the ban as a workflow intent: 'a force-push that this workflow is designed to avoid'. Same fix at the matching site in `.claude/commands/reply-reviews.md`.

<!-- gh-id: 3116240244 -->
#### ↳ cmk ([2026-04-21 08:57 UTC](https://github.com/cmk/riffgrep/pull/23#discussion_r3116240244))

Fixed in ccaceed — reworded to 'auto-quit on the 6th consecutive quiet tick (after the 5-slot backoff is exhausted)'. The 5+5+5+10+10 cadence depends on all 5 slots firing, so kept the `count > 5` threshold and aligned the prose/message. Same fix in `.claude/commands/watch-pr.md` (comment + loop-ending `echo`).

<!-- gh-id: 3116240478 -->
#### ↳ cmk ([2026-04-21 08:57 UTC](https://github.com/cmk/riffgrep/pull/23#discussion_r3116240478))

Fixed in ccaceed — `.claude/settings.local.json` is gitignored globally (`~/.config/git/ignore`) so GitHub reviewers never see it. Rephrased to 'a force-push that this workflow is designed to avoid', matching the parallel fix in CLAUDE.md.
