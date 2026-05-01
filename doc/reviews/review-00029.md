# PR #29 — Port multi-agent workflow

## Summary

Port the current multi-agent workflow from `template-rust` into `riffgrep`.

This makes `AGENTS.md` the canonical shared instruction file, leaves `CLAUDE.md` as a compatibility symlink, adds the FSM workflow scripts, and updates the Claude command docs to use the atomic review-round flow. It also adds the git pre-commit hook layer and enables the hook path in this checkout.

The new hook exposed a pre-existing SQLite integration test issue: `sqlite_count_mode` used the helper that always injects `--no-db`, so it was not actually exercising SQLite mode. That test now uses the raw command helper for both indexing and counting.

Validation:
- `bash -n` on workflow scripts and `.githooks/pre-commit`
- `git diff --check`
- `scripts/workflow_state.sh`
- `scripts/safe_merge.sh --help`
- `scripts/local_review.sh --check`
- commit hook: `cargo fmt --all -- --check`, `scripts/check-pii.sh`, `cargo test --workspace`, `cargo clippy --all-targets -- -D warnings`
