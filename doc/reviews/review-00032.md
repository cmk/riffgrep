# PR #32 - Port template workflow contract

## Summary

Ports the current `template-rust` workflow contract into `riffgrep`.

- Renames only workflow entrypoints to the current `pr_*`, `git_*`, and underscore script names, leaving product search/indexing scripts untouched.
- Adds a pre-push hook for the expensive Rust test/clippy gate while keeping the commit-time hook cheap.
- Adds Python workflow-state tests and runs them in the existing Python CI job before the product pytest suite.

Verification:

- `bash -n scripts/*.sh .githooks/pre-commit .githooks/pre-push`
- `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest`
- `scripts/pr_report.py path 1`
- `scripts/workflow_state.sh`
- `scripts/check_pii.sh`
- `cargo fmt --all -- --check`
- `git diff --cached --check`
