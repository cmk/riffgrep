# AGENTS.md

`AGENTS.md` is the shared instruction file for Codex, Claude Code, and
other coding agents. `CLAUDE.md` is a compatibility symlink back to this
file. Claude Code-specific commands and settings remain under `.claude/`.

## What this repo is

**riffgrep** is a high-performance Rust CLI tool for searching and editing
WAV sample library metadata. It targets ripgrep-level speed (1-5 seconds)
across 1.2M WAV files (~4TB). It replaces SoundMiner with a native
command-line workflow.

## Parallel work

At the start of each conversation, ask the user:
"Are any other agent instances working in this repo right now?"

If yes, a worktree is **mandatory** — see the TDD workflow's Step 1
for the naming convention (`../<repo>.plan-YYYY-MM-DD-NN` + branch
`plan/YYYY-MM-DD-NN`).

Never run two agent instances in the same worktree. Cargo takes a
file lock on `target/` during each build, so concurrent builds stall
behind each other ("Blocking waiting for file lock"). Separate
worktrees each get their own `target/` and sidestep the lock —
**unless** `CARGO_TARGET_DIR` is exported in your shell or
`~/.cargo/config.toml` sets `[build] target-dir`, either of which
forces every worktree to share one directory and reintroduces the
lock. Verify with `cargo metadata --format-version 1 --no-deps | jq
-r .target_directory` in two worktrees — different paths = safe.

## Workflow Is a State Machine

The TDD and review workflow is a finite state machine, not a menu of
roughly-equivalent steps. Before committing, running local review,
pushing, replying to review comments, or merging, agents must identify
the current state and take only the documented transition out of it.
Use `scripts/workflow_state.sh` as a read-only state check when the
state is not obvious.

The intended path is:

```
main_clean
  -> on_branch
  -> plan_committed
  -> impl_green
  -> plan_finalized
  -> local_reviewed
  -> pushed
  -> gh_review
  -> items_pulled
  -> round_unpushed
  -> gh_review
  -> merged
```

Do not skip, reorder, or replace a transition with an ad hoc command
that merely looks equivalent. Use the repo scripts and commands for
workflow-sensitive actions:

- Local review: Claude Code uses `/sprint-review`; Codex and shell
  users use `scripts/local_review.sh`. Claude Code's built-in
  `/review [PR]` is optional post-push review help, not the canonical
  pre-push transition.
- PR body pathing: `scripts/review_path.sh` and
  `scripts/extract_pr_body.sh`.
- GitHub review ingestion: `scripts/pull_reviews.py`.
- Review replies: `/reply-reviews` or the underlying
  `scripts/reply_review.py` + `scripts/pull_reviews.py` flow.
- Merge: `scripts/safe_merge.sh`, not raw `gh pr merge`.

## Architecture

### Dual-mode search

Two interchangeable data sources behind the same `SampleSource` trait:
- **SQLite mode** (default): FTS5 Trigram index for instant (<10ms) search.
- **Databaseless mode** (`--no-db`): `ignore::WalkParallel` with JIT header reads.

### Source layout

```
src/
├── main.rs              # Entry point, bpaf CLI parsing
├── lib.rs               # Crate root, module declarations
├── engine/
│   ├── mod.rs           # UnifiedMetadata, SampleSource trait, read_metadata
│   ├── bext.rs          # Surgical BEXT parser/writer, packed schema
│   ├── id3.rs           # ID3v2 tag reading via lofty, merge into metadata
│   ├── riff_info.rs     # RIFF INFO chunk parsing
│   ├── wav.rs           # WAV format handling, audio source
│   ├── source.rs        # AudioRegistry, format dispatch
│   ├── marks.rs         # Marker/cue point serialization
│   ├── sqlite.rs        # FTS5 Trigram search, batch indexing
│   ├── filesystem.rs    # Databaseless ignore walker
│   ├── workflow.rs      # Lua workflow engine, scripted transforms
│   ├── playback.rs      # Audio playback via rodio/symphonia
│   ├── similarity.rs    # Embedding-based similarity search
│   ├── cli.rs           # CLI argument definitions
│   └── config.rs        # Configuration handling
├── ui/
│   ├── mod.rs           # TUI event loop
│   ├── widgets.rs       # Braille waveform, result list
│   ├── search.rs        # Search state management
│   ├── actions.rs       # User action dispatch
│   └── theme.rs         # Theme definitions
└── util.rs              # Logging, path normalization
```

### Key documentation

- `doc/DESIGN.md` — Architectural spec, trait interfaces, SQLite schema
- `doc/PICKER_SCHEMA.md` — BEXT metadata byte-level format
- `doc/plans/` — Sprint plan documents
- `doc/reviews/` — Local code review artifacts
- `doc/misc/SOUNDMINER_SCHEMA_ANALYSIS.md` — Reverse-engineered SM schema

## Repository conventions

- **Each commit must leave the repo in a state where `cargo test` passes.**
  Do not commit a library module without the tests that cover it in the
  same commit. Never commit a red test suite.
- **No merge commits.** Always rebase onto main — never `git merge`. The
  history must be linear.
- **CI-repair commits must be fixups.** If a commit on this branch broke
  CI and the follow-up exists only to repair it, commit with
  `git commit --fixup=<broken-sha>` instead of a standalone `fix:`.
  Before pushing, run `scripts/autosquash.sh` (a thin wrapper over
  `GIT_SEQUENCE_EDITOR=: git rebase -i --autosquash origin/main`) so the
  fixups collapse into their targets. This keeps main's linear history
  free of commits that temporarily broke the build. Review-round commits
  (addressing reviewer feedback from an earlier push) remain standalone
  so the audit trail survives.
- **No unsafe code**: `unsafe_code = "forbid"` in Cargo.toml lints.
- **Test fixtures**: tests that depend on fixture files must return early
  when the fixture is absent — do not `#[ignore]` and do not panic.
- **Property-based testing is mandatory** for any module that parses,
  encodes, or transforms data. Use `proptest` (dev-dep).
  - Define strategies as functions returning `impl Strategy`, not
    `Arbitrary` derive. Use `prop_oneof!` with frequency weights to
    bias toward boundary values and edge cases.
  - Properties that must hold for a sprint to ship are defined **in
    the plan's Verification table** before any code is written.
  - If a property test blocks progress during implementation, you may
    `#[ignore]` it temporarily but **you must document it** in the
    plan's Review section with the reason and a plan to re-enable.
- **Use Rust's modern module layout.** If you have a specific reason why
  you cannot then again **you must document it**. The modern layout does
  not have a `mod.rs` file. The equivalent module sits one level up and
  is named after the module directory:

  ```
  src/
  ├── main.rs
  ├── network.rs      <-- Defines 'network' module
  └── network/
      └── server.rs   <-- Submodule of 'network'
  ```

### Session notes

Session notes live in `doc/notes/note-YYYY-MM-DD-nn.md`. The final field
`nn` is a counter that resets to 01 each day. `doc/notes/` is gitignored
and holds the user's personal notes for the project. Agents may read
from it for context but must not write to it unless explicitly asked.

When the user says "print to notes", append the requested content to the
current day's notes file. Create the file if it doesn't exist.

### Commit style

Conventional commits, present-tense imperative subject. Accepted prefixes:
`plan`, `feat`, `fix`, `fmt`, `doc`, `test`, `task`, `debt`. Scopes are
allowed (e.g. `doc(skills):`, `fix(scripts):`).

- `plan:` lands a new plan doc in `doc/plans/` — always the first
  commit on a `plan/YYYY-MM-DD-NN` branch.
- `feat:` and the rest cover the implementation that follows.

```
plan: Widget-format parser, sprint goals and verification table
feat: Add parser for widget format
fix: Handle timeout on reconnect
test: Add round-trip property tests for codec
doc: Append sprint completion report
task: Add serde to dependencies
debt: Remove dead handshake branch
```

Keep subjects under 72 characters. Use the body for non-obvious decisions.

## Code Review Workflow

`doc/workflow.md` has mermaid state diagrams for the review-round
lifecycle and the `/watch-pr` loop — useful when debugging an
unexpected situation (stuck fix commit, loop that won't quit). The
prose below is authoritative; the diagrams are derived views.

### Tier 1 — Local Review (pre-push)

The coding agent makes atomic commits as it works. Each commit must pass
`cargo test` and `cargo clippy` (enforced by the pre-commit hooks in
`.claude/settings.json` and `.githooks/pre-commit`). Commits can be as small as desired.

Step 7 of the TDD workflow creates the PR's review file with the
sprint's PR description under a `## Summary` heading. The path comes
from `scripts/review_path.sh` — no argument, it predicts the next PR
number (via `scripts/next_pr_number.sh`) and emits the zero-padded
filename, e.g. `doc/reviews/review-00017.md`. `next_pr_number.sh`
queries the repo's highest existing issue/PR number via `gh api` and
adds one (GitHub shares its numbering sequence between issues and
PRs). The `## Summary` section is the single source of truth for the
PR body: open the PR with
`gh pr create --body-file <(scripts/extract_pr_body.sh N)` so the
GitHub body is a direct copy of the file. Because the description is
committed *before* push, a PR that gets no review comments merges
without any extra round-trip — the body is already in history.
`review-00000.md` is a protected sentinel; real reviews start at
`00001`.

Before pushing, run the local review transition. Claude Code uses the
repo-specific `/sprint-review` command. Codex and shell users use
`scripts/local_review.sh`, which invokes `codex review --base
origin/main` with the repo conventions and calibration examples. Both
paths examine `git diff origin/main...HEAD` and the commit log, then
append findings as a `## Local review (YYYY-MM-DD)` section below the
summary. The local-review command aborts if the review file or its
`## Summary` section is missing — step 7 is a prerequisite. Claude
Code's built-in `/review [PR]` may be useful after a PR exists, but it
is not the canonical pre-push FSM transition.

If another issue or PR is opened between running step 7 and opening
this branch's PR, the predicted number can drift — re-run
`scripts/review_path.sh` before pushing and `mv` the old file to the
new path if needed. `/sprint-review` re-predicts on each run, so the
rename keeps it pointed at the same file.

If must-fix items exist, resolve them before pushing. If the review
is clean, push and open the PR with `--body-file` as above.

### Tier 2 — GitHub Review (post-push)

Once pushed, CI (`.github/workflows/ci.yml`) runs build, clippy, test, and
fmt checks. Claude Code Action and/or GitHub Copilot perform a second-round
review on the PR automatically.

After GitHub review activity, run `/pull-reviews <N>` to fetch the PR's
review bodies and inline comments and **append them chronologically to the
same `doc/reviews/review-NNNNN.md`** used by Tier 1. The command is
idempotent — it records `<!-- gh-id: NNNNN -->` markers for each appended
item and skips any id already present, so running it repeatedly only
appends new comments. The result is one file per PR containing the full
local + GitHub review history in order.

Once the findings are addressed as **uncommitted edits in the working
tree**, run `/reply-reviews <N>`. The command does the whole round
in order: posts replies to each unresolved thread, runs
`scripts/pull_reviews.py` to mirror the replies into `review-NNNNN.md`,
then makes ONE atomic commit containing both the code edits and the
mirrored doc. You then `git push` once — code + replies + review doc
land in a single round trip.

**Do not commit the fix yourself before running `/reply-reviews`.**
The command runs on the `gh_review → items_pulled → round_unpushed`
arrow per `doc/workflow.md` — it expects to start from `gh_review`
(local at-or-behind origin) and produce the round commit itself.
Pre-committing a fix would put the branch at an unpushed-state that
breaks the precondition; if you have a stranded pre-existing fix
commit, push it first, then re-run. `/reply-reviews` refuses to run
if the branch already has unpushed commits.

**Do not merge before pushing the round commit.** Per
`doc/workflow.md`'s state machine, the merge transition is
`gh_review → merged` — there is no edge from `round_unpushed → merged`.
Merging from `round_unpushed` (the state after `/reply-reviews`
makes its commit but before push) silently drops the local commit
because `gh pr merge` is GitHub-side and doesn't see local state.
Use `scripts/safe_merge.sh <pr-args>` instead of `gh pr merge` —
the wrapper refuses to invoke the merge while the local branch
is ahead of origin.

`/pull-reviews <N>` remains available as a lower-level primitive for
fetching comments without posting. Use it standalone only to refresh
the doc right before the final pre-merge push, to capture any trailing
reviewer comments; its output rides with the next round commit, never as
a standalone `doc:` commit.

The local review catches design issues and convention violations early.
The GitHub review catches anything that slipped through and validates in
the CI environment. Joining them into a single file per PR preserves the
conversational flow and keeps the review record in one place.

### PR Polling (optional)

For PRs where you don't want to manually ping "check the replies", pair
`/watch-pr <N>` with `/loop`:

```
/loop 10m /watch-pr 17
```

Each tick does one of: (a) heartbeat if no new activity, (b) one
finish-the-round cycle — auto-fix the trivially-clear items, push back
or defer the rest, run the `/reply-reviews` flow, **push the round
commit**, or (c) `paused at round_unpushed: push failed` if the push
itself errored (network, non-fast-forward).

Auto-fix is scoped tightly: only items where the reviewer's intent is
unambiguous and the change is local (one file, under ~20 lines, no
API removal, no cross-module reasoning). Anything involving judgment
is classified as **needs you** and surfaced in the round report with
`path:line` — those threads stay open on GitHub for you to resolve.

The command never **merges**. The merge is the user's safety gate:
each PR is reviewed manually before `gh pr merge` /
`scripts/safe_merge.sh`. Pushing the round commit advances the
branch to `gh_review` so CI re-runs and the reviewer sees replies
attached to the right tip — that's normal mid-PR motion, not a risk
worth gating on.

## Test-Driven Development Workflow

Every sprint follows this order. Naming is keyed to the plan filename:
a plan at `doc/plans/plan-YYYY-MM-DD-NN.md` maps to branch
`plan/YYYY-MM-DD-NN` and (if used) worktree `../<repo>.plan-YYYY-MM-DD-NN`.
One slug, three places.

1. **Pick the plan filename.** `ls doc/plans/plan-YYYY-MM-DD-*.md` to
   find the next unused `NN` for today's date (zero-padded, starts at
   `01`). No writes yet — main stays clean.
2. **Ask the user: worktree or branch?** Worktree is mandatory if
   another agent instance is active in this repo; otherwise it's the
   user's call. Then:
   - worktree: `git worktree add ../<repo>.plan-YYYY-MM-DD-NN -b plan/YYYY-MM-DD-NN`, `cd` into it.
   - branch: `git switch -c plan/YYYY-MM-DD-NN`.
3. **Write the plan** to `doc/plans/plan-YYYY-MM-DD-NN.md` on that branch.
   The plan's **Verification** table must list the property tests that
   must pass for the sprint to ship. Commit as `plan: <one-line goal>`
   — this is the sprint-opener, always the first commit on the branch.
4. Write proptest properties and test skeletons that compile but
   trivially fail. Properties come first — they define the contract.
5. Implement the module until all tests are green.
6. Commit on the branch, when green.
7. **Finalize the sprint docs.** In one commit:
   - Append Deferred and Review sections to the plan document. If any
     property tests were `#[ignore]`d during implementation, document
     the reason and the re-enablement plan here.
   - Create the review file at `$(scripts/review_path.sh)` (no
     argument predicts the next PR number and zero-pads the
     filename). Header is `# PR #<N> — <title>` followed by a
     `## Summary` section containing the PR body. This section is
     consumed verbatim by
     `gh pr create --body-file <(scripts/extract_pr_body.sh N)`, so
     write it as the PR description (what & why for a human
     reviewer) — not a ship-report.

   This must happen *before* the local review — the reviewer agent
   reads the plan as context and should see the final version, and
   `/sprint-review` aborts if the review file is missing its
   `## Summary`. Commit as `doc: Finalize plan NN and PR description`.
8. Run the local review transition before pushing:
   - Claude Code: `/sprint-review`
   - Codex/shell: `scripts/local_review.sh`
9. Rebase and land on main. First, on the feature branch:
   `git fetch origin && git rebase origin/main`. Then fast-forward main:
   - **Branch case**: `git checkout main && git merge --ff-only plan/YYYY-MM-DD-NN`.
   - **Worktree case**: main is already checked out in the *primary*
     worktree, so you can't `checkout main` here. `cd` back to the
     primary and run `git merge --ff-only plan/YYYY-MM-DD-NN` there.
     Step 10's `git worktree remove` then runs from the primary too.
10. Clean up: `git worktree remove ../<repo>.plan-YYYY-MM-DD-NN`
    (worktree case only), then `git branch -d plan/YYYY-MM-DD-NN`.

### Pre-Commit Hooks

Two complementary layers guard every commit:

**Layer 1 — Claude Code `PreToolUse`** (`.claude/settings.json`):
fires on agent-invoked Bash calls matching `git commit*`. Catches
issues during agent iteration without invoking git for real.
Limitation: `PreToolUse` runs *before* the matched Bash call's body
executes, so a chained command like `git add file && git commit -m
"..."` sees an empty pre-add staged diff at hook time and slips
through `check-pii.sh`. Use separate `git add` and `git commit`
calls to keep this layer effective.

**Layer 2 — Git `pre-commit`** (`.githooks/pre-commit`): fires at
git's standard hook point (after staging, before commit object
creation). Sees the actual staged content regardless of how the
commit was invoked — chained Bash, terminal, IDE, anything. This is
the unbypassable safety net.

Activate Layer 2 on a fresh clone:

```
git config core.hooksPath .githooks
```

Both layers run the same check chain, in order. Every step is
blocking — the chain short-circuits on the first failure and the
commit is aborted:

1. `cargo fmt --all -- --check` — fmt drift aborts the commit. Run
   `cargo fmt --all` to fix.
2. `scripts/check-pii.sh` — grep the staged diff for absolute
   user-home paths (`/Users/...` on macOS, `/home/...` on Linux),
   private-key headers, and common API-token shapes. Fail fast on
   any match. Allow-list exceptions go in `.pii-allow`.
3. `cargo test --workspace` — all tests must pass.
4. `cargo clippy --all-targets -- -D warnings` — matches CI.

This is the automated quality gate; the local review transition
(`/sprint-review` for Claude Code, `scripts/local_review.sh` for
Codex/shell) is the manual one. Bypass with `--no-verify` only when
explicitly authorized.

## Sprint Plan Format

```markdown
# Plan NN — Title

## Goal
One sentence.

## Dependency Graph
ASCII art showing task dependencies (T1 → T2, T3 → T4, etc.)

## Tasks
Each task is T1, T2, etc. Each task section includes:
- Problem or motivation
- Solution / implementation approach
- Types or API surface

## Verification

### Properties (must pass)
Table of proptest property names, the module they live in, and the
invariant they assert. These are the contract — if a property can't
be satisfied, the sprint isn't done.

| Property | Module | Invariant |
|----------|--------|-----------|
| `msg_round_trips` | `crate_foo::codec` | encode then decode recovers original |

### Spot checks
Table of unit test names + specific assertions.

### Build gates
- cargo build — no errors
- cargo test — all pass (no `#[ignore]` without Review documentation)
- cargo clippy --all-targets — no errors
- End-to-end scenario description

## Deferred
What was intentionally left out and why.

## Review
- Any `#[ignore]`d properties: which ones, why, re-enablement plan
- Design deviations from the plan
- Recommendations
```
