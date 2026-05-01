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
  scripts/review_path.sh >/dev/null
  scripts/workflow_state.sh
  exit 0
fi

if ! command -v codex >/dev/null; then
  echo "error: codex CLI not found on PATH" >&2
  exit 1
fi

git fetch --quiet origin main

if [ -n "$(git status --porcelain)" ]; then
  echo "error: working tree must be clean before local review" >&2
  exit 1
fi

if git -c color.ui=never log --oneline origin/main..HEAD | grep -E '^[0-9a-f]+ fixup!' >/dev/null; then
  scripts/autosquash.sh
fi

if git diff --quiet origin/main...HEAD; then
  echo "error: nothing to review against origin/main" >&2
  exit 1
fi

review_file=$(scripts/review_path.sh)
if [ ! -f "$review_file" ]; then
  echo "error: review file not found: $review_file" >&2
  echo "  run TDD step 7 first: finalize the plan and draft PR description." >&2
  exit 1
fi

if ! grep -q '^## Summary[[:space:]]*$' "$review_file"; then
  echo "error: review file is missing ## Summary: $review_file" >&2
  exit 1
fi

plan_context=''
latest_plan=$(ls -t doc/plans/plan-*.md 2>/dev/null | head -1 || true)
if [ -n "$latest_plan" ]; then
  plan_context=$(printf '\n## Sprint plan candidate: %s\n\n' "$latest_plan"; cat "$latest_plan")
fi

calibration_context=''
if [ -f doc/reviews/review-calibration.md ]; then
  calibration_context=$(printf '\n## Review calibration examples\n\n'; cat doc/reviews/review-calibration.md)
fi

branch=$(git branch --show-current)
commits=$(git rev-list --count origin/main..HEAD)
date=$(date +%F)

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

{
  cat <<'EOF'
You are reviewing code on a local feature branch before it is pushed to
GitHub. This is a pre-push quality gate. Review the diff between
origin/main and the branch HEAD.

Be direct and specific. Prioritize bugs, behavioral regressions,
missing tests, and violations of repo workflow. Cite file paths and
line numbers where possible. Separate must-fix issues from follow-ups.

Use the repo conventions and calibration examples below as context.
EOF
  printf '\n## Repo conventions\n\n'
  cat AGENTS.md
  printf '%s\n' "$plan_context"
  printf '%s\n' "$calibration_context"
} | codex review --base origin/main - >"$tmp"

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
