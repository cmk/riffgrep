# PR #3 — Pin toolchain to 1.94 and fix all lint/fmt gates

<!-- gh-id: 3090903840 -->
### Copilot on `.github/workflows/ci.yml:30` (2026-04-16 05:14 UTC)

CI installs Rust toolchain "1.94" while rust-toolchain.toml pins "1.94.0" (and the PR description says exact matching). This can either fail to resolve or drift if the action treats it as a floating patch; set the workflow toolchain to "1.94.0" or omit the explicit toolchain so it follows rust-toolchain.toml.

<!-- gh-id: 4118456055 -->
### copilot-pull-request-reviewer[bot] — COMMENTED (2026-04-16 05:14 UTC)

## Pull request overview

Pins the Rust toolchain to a specific version and updates formatting/linting so local development and CI enforce the same strict `fmt` + `clippy -D warnings` gates across the project.

**Changes:**
- Pin Rust toolchain to 1.94.0 and update CI to enforce strict clippy/fmt gates.
- Apply repo-wide rustfmt and clippy-driven refactors (largely formatting/structure).
- Add/adjust lint configuration and targeted `allow(...)`s where appropriate (benches, dead_code).

### Reviewed changes

Copilot reviewed 26 out of 26 changed files in this pull request and generated 1 comment.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| tests/integration.rs | Reformat CLI argument arrays in integration tests. |
| tests/edge_cases.rs | Reformat predicates/args in edge case tests. |
| src/ui/widgets.rs | Formatting + let-chains refactors in TUI rendering utilities and tests. |
| src/ui/search.rs | Formatting + let-chains refactors in search/peaks loading and tests. |
| src/ui/mod.rs | Formatting refactors; derive `Default` for `PreviewData`; let-chains cleanups. |
| src/ui/actions.rs | Formatting refactors for action grouping/key parsing/keymap tests. |
| src/engine/workflow.rs | Derive `Default` for `WorkflowScript`; refactors; relocates `write_metadata_changes`. |
| src/engine/wav.rs | Formatting refactors and minor test cleanups. |
| src/engine/sqlite.rs | Formatting refactors; adds `#[allow(dead_code)]` to unused embedding/metadata helpers. |
| src/engine/source.rs | Formatting refactors; uses `workflow::write_metadata_changes` for RIFF writes. |
| src/engine/similarity.rs | Formatting refactors; adds `#[allow(dead_code)]` for unused fields/consts. |
| src/engine/riff_info.rs | Formatting refactors and minor test simplifications. |
| src/engine/playback.rs | Formatting refactors; let-chains cleanup in state handling and tests. |
| src/engine/mod.rs | Formatting refactors; let-chains cleanup; minor I/O formatting changes. |
| src/engine/marks.rs | Formatting refactors; let-chains cleanup around parent dir creation and locking. |
| src/engine/id3.rs | Moves `merge_id3_into_unified` earlier; adds it as a public helper; formatting changes. |
| src/engine/filesystem.rs | Formatting refactors; minor test cleanups. |
| src/engine/config.rs | Expands `column_def` formatting; small refactors in config loading/tests. |
| src/engine/cli.rs | Formatting refactors in help builder and CLI tests. |
| src/engine/bext.rs | Formatting refactors; `to_bytes` receivers changed to by-value (types are `Copy`). |
| rust-toolchain.toml | Pins toolchain channel to `1.94.0`. |
| benches/tui_render.rs | Adds `#![allow(missing_docs)]` to satisfy lint gates. |
| benches/sqlite_ops.rs | Adds `#![allow(missing_docs)]` and reformats benchmark code. |
| benches/metadata_read.rs | Adds `#![allow(missing_docs)]` and reformats benchmark code. |
| Cargo.toml | Adds `[lints.clippy]` configuration to allow selected lints. |
| .github/workflows/ci.yml | Uses dtolnay toolchain action; enforces `clippy -D warnings` and `fmt --check`. |
</details>






---

💡 <a href="/cmk/riffgrep/new/main?filename=.github/instructions/*.instructions.md" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Add Copilot custom instructions</a> for smarter, more guided reviews. <a href="https://docs.github.com/en/copilot/customizing-copilot/adding-repository-custom-instructions-for-github-copilot" class="Link--inTextBlock" target="_blank" rel="noopener noreferrer">Learn how to get started</a>.

<!-- gh-id: 3091086839 -->
#### ↳ cmk (2026-04-16 06:09 UTC)

@claude can you update this to match rust-toolchain.toml?

<!-- gh-id: 3091087680 -->
#### ↳ claude[bot] (2026-04-16 06:09 UTC)

**Claude encountered an error** —— [View job](https://github.com/cmk/riffgrep/actions/runs/24495034996)

---
I'll analyze this and get back to you.
