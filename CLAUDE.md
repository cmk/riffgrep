# CLAUDE.md

## What this repo is

**riffgrep** is a high-performance Rust CLI tool for searching and editing
WAV sample library metadata. It targets ripgrep-level speed (1-5 seconds)
across 1.2M WAV files (~4TB). It replaces SoundMiner with a native
command-line workflow.

## Parallel work

At the start of each conversation, ask the user:
"Are any other Claude instances working in this repo right now?"

If yes (or if the user says "work in a worktree"), create a git worktree
before making any changes:

```zsh
git worktree add ../riffgrep-<task> -b <branch>
```

Never run two Claude instances in the same worktree — cargo target-dir
locks and fd contention will break one or both sessions.

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
  Never commit a red test suite. A pre-commit hook enforces this.
- **No merge commits.** Always rebase onto main — never `git merge`. The
  history must be linear.
- **No unsafe code**: `unsafe_code = "forbid"` in Cargo.toml lints.
- **Property-based testing is mandatory** for any module that parses,
  encodes, or transforms data. Use `proptest` (dev-dep).
- **Test fixtures**: tests that depend on fixture files must return early
  when the fixture is absent — do not `#[ignore]` and do not panic.

### Session notes

Session notes live in `doc/notes/note-YYYY-MM-DD-nn.md`. The final field
`nn` is a counter that resets to 01 each day.

When the user says "print to notes", append the requested content to the
current day's notes file. Create the file if it doesn't exist.

### Commit style

Conventional commits, present-tense imperative subject:

```
feat: Add parser for widget format
fix: Handle timeout on reconnect
test: Add round-trip property tests for codec
doc: Append sprint completion report
task: Add serde to dependencies
```

Keep subjects under 72 characters. Use the body for non-obvious decisions.

## Two-tier review workflow

### Tier 1 — Local review (pre-push)

The coding agent makes atomic commits as it works. Each commit must pass
`cargo test` and `cargo clippy` (enforced by the pre-commit hook in
`.claude/settings.json`). Commits can be as small as desired.

Before pushing to GitHub, run `/sprint-review`. This spawns an independent
reviewer agent that examines `git diff main...HEAD` and the commit log. The
reviewer flags must-fix issues and follow-ups. The review is saved to
`doc/reviews/review-YYYY-MM-DD-nn.md`.

If must-fix items exist, resolve them before pushing. If the review is
clean, push and create a PR.

### Tier 2 — GitHub review (post-push)

Once pushed, CI (`.github/workflows/ci.yml`) runs build, clippy, test, and
fmt checks. Claude Code Action and/or GitHub Copilot perform a second-round
review on the PR automatically.

The local review catches design issues and convention violations early.
The GitHub review catches anything that slipped through and validates in
the CI environment.

## TDD workflow

1. Write the plan to `doc/plans/plan-YYYY-MM-DD-nn.md` before touching source.
2. Create a branch for the sprint: `git checkout -b sprint/<name>`
3. Write tests first — property tests define the contract.
4. Implement until all tests are green.
5. Commit atomically as you go (hook enforces test + clippy).
6. Run `/sprint-review` (Tier 1) before pushing.
7. Push and create PR (Tier 2 runs automatically).
8. Rebase onto main after approval, fast-forward merge.

## Sprint plan format

```markdown
# Plan NN — Title

## Goal
One sentence.

## Tasks
Each task section includes problem, solution, and API surface.

## Verification

### Properties (must pass)
| Property | Module | Invariant |
|----------|--------|-----------|

### Spot checks
Unit test names + specific assertions.

### Build gates
- cargo build — no errors
- cargo test — all pass
- cargo clippy --all-targets — no errors

## Deferred
What was intentionally left out and why.
```
