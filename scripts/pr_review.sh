#!/usr/bin/env bash
# Codex implementation of the plan_finalized -> local_reviewed FSM transition.
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(git -C "$script_dir/.." rev-parse --show-toplevel)
cd "$repo_root"

if [ "${1:-}" = "--check" ]; then
  command -v codex >/dev/null
  codex review --help >/dev/null
  command -v gh >/dev/null
  scripts/pr_report.py path 1 >/dev/null
  scripts/workflow_state.sh
  exit 0
fi

if ! command -v codex >/dev/null; then
  echo "error: codex CLI not found on PATH" >&2
  exit 1
fi

if ! command -v gh >/dev/null; then
  echo "error: gh CLI not found on PATH" >&2
  exit 1
fi

git fetch --quiet origin main

if [ -n "$(git status --porcelain)" ]; then
  echo "error: working tree must be clean before local review" >&2
  exit 1
fi

if git -c color.ui=never log --oneline origin/main..HEAD | grep -E '^[0-9a-f]+ fixup!' >/dev/null; then
  scripts/git_squash.sh
fi

if git diff --quiet origin/main...HEAD; then
  echo "error: nothing to review against origin/main" >&2
  exit 1
fi

if ! review_file=$(scripts/pr_report.py path); then
  echo "error: could not determine review file path" >&2
  echo "  ensure gh is authenticated, or run scripts/pr_request.sh owner/name to diagnose GitHub access." >&2
  exit 1
fi
if [ ! -f "$review_file" ]; then
  echo "error: review file not found: $review_file" >&2
  echo "  run TDD step 7 first: finalize the plan and draft PR description." >&2
  exit 1
fi

if ! grep -q '^## Summary[[:space:]]*$' "$review_file"; then
  echo "error: review file is missing ## Summary: $review_file" >&2
  exit 1
fi

branch=$(git branch --show-current)
commits=$(git rev-list --count origin/main..HEAD)
date=$(date +%F)

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

# Codex CLI 0.125 rejects a custom prompt together with --base, even
# though help advertises both. AGENTS.md is discovered from the repo
# root, so use the supported base-review invocation.
codex review --base origin/main >"$tmp"

{
  printf '\n## Local review (%s)\n\n' "$date"
  printf '**Branch:** %s\n' "$branch"
  printf '**Commits:** %s (origin/main..%s)\n' "$commits" "$branch"
  printf '**Reviewer:** Codex (`codex review --base origin/main`)\n\n'
  printf '%s\n\n' '---'
  cat "$tmp"
  printf '\n'
} >>"$review_file"

printf 'local review appended: %s\n' "$review_file"
