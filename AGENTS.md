# AGENTS.md

`AGENTS.md` is the shared instruction file for Codex, Claude Code, and
other coding agents. `CLAUDE.md` is a compatibility symlink back to this
file. Claude Code-specific commands and settings remain under `.claude/`.

## What this repo is

**riffgrep** is a high-performance Rust CLI for searching and editing
WAV sample library metadata. It targets ripgrep-level speed (1–5
seconds) across 1.2M WAV files (~4TB), replacing SoundMiner with a
native command-line workflow.

## Architecture

### Dual-mode search

Two interchangeable data sources behind the same `SampleSource` trait:

- **SQLite mode** (default): FTS5 Trigram index for instant (<10ms)
  search.
- **Databaseless mode** (`--no-db`): `ignore::WalkParallel` with JIT
  header reads.

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
- `doc/reviews/` — Local + GitHub code review artifacts
- `doc/misc/SOUNDMINER_SCHEMA_ANALYSIS.md` — Reverse-engineered SM schema

Bumping MSRV requires updating two places together: `rust-version` in
`Cargo.toml` and the channel in `rust-toolchain.toml`.

## Library conventions

- **No unsafe code**: `unsafe_code = "forbid"` in Cargo.toml lints.
- **anyhow is allowed here.** riffgrep is a binary crate and is the
  canonical wrapper exempted from the workspace-wide `anyhow` ban (see
  template-rust's dependency policy). Internal library modules should
  still prefer `thiserror` for typed errors; `anyhow::Result` is the
  CLI boundary.
- **Test fixtures are gitignored**; a fresh checkout passes
  `cargo test` with zero setup. Fixture-dependent tests must `return`
  cleanly when the fixture is absent — **do not** `#[ignore]` them and
  do not panic.
- **Property-based testing is mandatory** for any module that parses,
  encodes, or transforms data (`proptest` dev-dep):
  - Strategies are functions returning `impl Strategy`, not `Arbitrary`
    derive. Use `prop_oneof!` with frequency weights to bias toward
    boundaries.
  - **Generator domain = the input type's full domain.** Boundaries
    (`MAX`, `MIN`, `0`, NaN, ±∞ where relevant) go in explicit `Just(_)`
    arms with elevated frequency. Bounding the generator to keep
    arithmetic "safe" is an anti-pattern — it fakes coverage by hiding
    the wrap region.
  - **Test the test before pushing.** Revert the fix in a dirty
    worktree and re-run; if the proptest still passes, it's
    decorative.
  - Sprint-blocking properties go in the plan's **Verification** table
    before any code is written. Temporary `#[ignore]` requires a
    Review-section reason and re-enable plan.
- **Prefer Rust's modern module layout** for new modules and refactors
  where feasible — parent module one level up, no `mod.rs`. This repo
  still has some `mod.rs` files, so their presence alone is not a bug.
  If you introduce, retain, or depend on `mod.rs` in a new change,
  document why.

  ```
  src/
  ├── main.rs
  ├── network.rs      <-- Defines 'network' module
  └── network/
      └── server.rs   <-- Submodule of 'network'
  ```

## Repository conventions

### Parallel work

At the start of each conversation, ask: "Are any other agent instances
working in this repo right now?" If yes, a worktree is **mandatory** —
two agents in the same worktree stall on cargo's `target/` lock.
Naming: `../<repo>.plan-YYYY-MM-DD-NN` + branch
`plan/YYYY-MM-DD-NN` (TDD step 1).

Verify worktrees aren't sharing `target/` (would happen if
`CARGO_TARGET_DIR` is set or `~/.cargo/config.toml` overrides
`build.target-dir`):
`cargo metadata --format-version 1 --no-deps | jq -r .target_directory`
in each — different paths = safe.

### The gardener rule

Weeds are weeds, regardless of who planted them. Whenever you spot a
violation of any rule above — stale comment, mis-bounded proptest
generator, lying `expect()` string, undocumented `#[ignore]`,
new-but-undocumented `mod.rs` — flag it even if you didn't write it.

- **Flag** in the plan's `## Review` section: `file:line — rule —
  consequence`.
- **Fix** if local, single-file, no API change, no scope expansion —
  ask the user before merging.
- **Defer** otherwise — name the cleanup specifically enough for the
  next plan branch to pick up.

CI gates (fmt, clippy, gitleaks, `check_pii.sh`) catch their own
drift. The gardener rule covers what lives *below* the gate: prose,
doc links, decorative tests, mis-bounded generators, stale section
headers, panic strings that contradict preconditions, deferred
Verification-table properties.

### Session notes

Session notes live in `doc/notes/note-YYYY-MM-DD-nn.md` (the `nn`
counter resets to 01 each day). `doc/notes/` is gitignored and holds
the user's personal notes. Agents may read it for context but must not
write to it unless explicitly asked. When the user says "print to
notes", append to the current day's notes file (create if absent).

### Git hooks

Hooks are activated by `git config core.hooksPath .githooks`. Bypass
(`--no-verify`) only when explicitly authorized; CI re-runs the same
gates plus a `gitleaks` history scan as defense-in-depth.

- **Each pushed commit must be green.** `pre-push` runs `cargo test` +
  `cargo clippy --all-targets -- -D warnings`. Intra-branch commits
  can be transiently red — pre-push is the gate, CI is the source of
  truth for `origin/main`. `pre-commit` runs the cheap chain on every
  commit: `cargo fmt --check`, `scripts/check_pii.sh` (absolute
  user-home paths, private-key headers, common API-token shapes;
  allow-list in `.pii-allow`). There's also an agent `PreToolUse`
  layer in `.claude/settings.json` that catches PII drift on
  agent-invoked `git commit*` — but use separate `git add` and
  `git commit` calls, since chained `add && commit` sees an empty
  pre-add diff and slips through.
- **CI-repair commits are fixups.** `git commit --fixup=<sha>`, then
  `scripts/git_squash.sh` before push (a thin wrapper over
  `GIT_SEQUENCE_EDITOR=: git rebase -i --autosquash origin/main`).
  Review-round commits stay standalone so the audit trail survives.
- **No merge commits.** Always rebase onto main — history must be
  linear.

### Sprint workflow

The sprint workflow is a finite state machine, not a menu. The full
review-round lifecycle and `/pr-watch` loop are diagrammed in
`doc/workflow.md` (the prose here is authoritative if the two
disagree). Identify the current state before committing, running
local review, pushing, replying, or merging — take only the documented
transition. Use `scripts/workflow_state.sh` when the state isn't
obvious.

```
main_clean → on_branch → plan_committed → impl_green → plan_finalized
  → local_reviewed → pushed → gh_review → items_pulled → round_unpushed
  → gh_review → merged
```

Workflow-sensitive actions go through repo scripts/commands:

- Local review: `/pr-review` (Claude Code) or `scripts/pr_review.sh`.
  `/review` is post-push help, **not** the canonical pre-push transition.
- PR body: `scripts/pr_report.py path` / `body`.
- GitHub review ingestion: `scripts/pr_report.py reviews`.
- Replies: `/pr-reply` (wraps `scripts/pr_reply.py` + `pr_report.py reviews`).
- Merge: `scripts/git_merge.sh`, **not** `gh pr merge`.

When a `gh`-backed command errors (auth prompt, network, missing
permission), surface the error — **don't silently fall back** to git
plumbing or MCP tools. They almost always do the wrong thing for
GitHub-side state.

### Test-driven development (TDD) workflow

A plan at `doc/plans/plan-YYYY-MM-DD-NN.md` maps to branch
`plan/YYYY-MM-DD-NN` and (optionally) worktree
`../<repo>.plan-YYYY-MM-DD-NN`. One slug, three places.

1. **Pick the filename.** `ls doc/plans/plan-YYYY-MM-DD-*.md` to find
   the next unused `NN`. No writes yet — main stays clean.
2. **Worktree or branch?** Worktree if another agent is active, else
   user's call. `git worktree add ../<repo>.plan-YYYY-MM-DD-NN -b
   plan/YYYY-MM-DD-NN` or `git switch -c plan/YYYY-MM-DD-NN`.
3. **Write the plan.** The Verification table lists property tests
   that must pass to ship. Commit as `plan: <one-line goal>`.
4. Write proptest properties + test skeletons that compile but fail.
5. Implement until green.
6. Commit on the branch when green.
7. **Finalize sprint docs** in one commit: append Deferred/Review
   sections to the plan; create the review file at
   `$(scripts/pr_report.py path)` with `# PR #<N> — <title>` +
   `## Summary` (the PR body, written for a human reviewer — not a
   ship-report). `review-00000.md` is a protected sentinel; real
   reviews start at `00001`. Commit as `doc: Finalize plan NN and PR
   description`. **Must precede local review.**
8. Run `/pr-review` (or `scripts/pr_review.sh`).
9. Open the PR:
   `gh pr create --body-file <(scripts/pr_report.py body N)`.
10. Rebase + land: `git fetch origin && git rebase origin/main`, then
    `git merge --ff-only`. (Worktree case: main is checked out in
    the *primary* worktree, so run the merge from there.)
11. `git worktree remove ...` (if used), then
    `git branch -d plan/YYYY-MM-DD-NN`.

### Code review

#### Tier 1 — Local (pre-push)

Before pushing, run `/pr-review` (or `scripts/pr_review.sh`). It
examines `git diff origin/main...HEAD`, appends a
`## Local review (YYYY-MM-DD)` section, and aborts if the review file
or `## Summary` is missing. The path comes from
`scripts/pr_report.py path` — no argument, it predicts the next PR
number (via `scripts/pr_request.sh`, which queries the highest
existing issue/PR and adds one) and emits the zero-padded filename.
If another issue or PR is opened between step 7 and pushing, re-run
`scripts/pr_report.py path` and `mv` the old file to the new path.

#### Tier 2 — GitHub (post-push)

CI runs build, clippy, test, and fmt checks. Auto-review agents and/or
Copilot review the PR.

1. `/pr-report <N>` fetches comments and **appends** them to
   `review-NNNNN.md` (idempotent via `<!-- gh-id: NNNNN -->` markers).
2. Address findings as **uncommitted edits** in the working tree.
3. `/pr-reply <N>` posts replies, mirrors them into the doc, and
   makes ONE atomic commit (code + replies + doc).
4. `git push` once.

**Do not pre-commit the fix** — `/pr-reply` expects to start from
`gh_review` (local at-or-behind origin) and produce the round commit
itself. **Do not merge from `round_unpushed`** — `gh pr merge` is
GitHub-side and silently drops local commits. Use
`scripts/git_merge.sh`, which refuses if the branch is ahead of origin.
If a merge already dropped a round commit, cherry-pick the stranded
SHA into the next plan branch's first commit.

`/pr-report <N>` remains available standalone to refresh the doc right
before the final pre-merge push; its output rides with the next round
commit, never as a standalone `doc:` commit.

#### Automated poll loop (optional)

`/loop 10m /pr-watch <N>` runs the round cycle on a timer. Each tick
either heartbeats, auto-fixes trivially-clear items + runs the
`/pr-reply` flow + pushes, or pauses on push failure. Auto-fix scope:
one file, < 20 lines, no API removal, no cross-module reasoning —
anything ambiguous is surfaced as **needs you**. The loop **never
merges** — that's the user's manual gate. See `doc/workflow.md` →
*`/pr-watch` dynamic-mode loop* for the per-tick state diagram.

### Commit style

Conventional commits, present-tense imperative subject. Accepted
prefixes: `plan`, `feat`, `fix`, `fmt`, `doc`, `test`, `task`, `debt`.
Scopes allowed (`doc(skills):`, `fix(scripts):`).

- `plan:` lands a new plan doc — always the first commit on a
  `plan/YYYY-MM-DD-NN` branch.
- `feat:` and the rest cover the implementation that follows.

```
plan: BEXT writer round-trip, sprint goals and verification table
feat: Add surgical BEXT writer
fix: Handle truncated RIFF INFO chunks
test: Add round-trip property tests for bext
doc: Finalize plan 14 and PR description
task: Bump lofty to 0.22
debt: Remove dead handshake branch
```

Keep subjects under 72 characters. Use the body for non-obvious
decisions.

## Sprint plan format

```markdown
# Plan NN — Title

## Goal
One sentence.

## Dependency Graph
T1 → T2, T3 → T4, ...

## Tasks
T1, T2, ... — each with: problem/motivation, solution/approach,
types or API surface.

## Verification

### Properties (must pass)
| Property | Module | Invariant |
|----------|--------|-----------|
| `bext_round_trips` | `engine::bext` | parse then write recovers original bytes |

### Spot checks
Unit test names + specific assertions.

### Build gates
- cargo build, test, clippy --all-targets — all clean
- End-to-end scenario description

## Deferred
What was intentionally left out, why.

## Review
- Any `#[ignore]`d properties — which, why, re-enablement plan
- Design deviations from the plan
- **Drift caught** (file:line — rule — fixed-here / deferred). See
  the gardener rule.
- Recommendations
```
