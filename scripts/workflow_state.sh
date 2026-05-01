#!/usr/bin/env bash
# Reports the repo's best-effort FSM state without mutating git state.
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(git -C "$script_dir/.." rev-parse --show-toplevel)
cd "$repo_root"

branch=$(git branch --show-current)
status=$(git status --porcelain)
base_commits=$(git rev-list --count origin/main..HEAD 2>/dev/null || printf 'unknown')
pr_number=''
if command -v gh >/dev/null 2>&1; then
  pr_number=$(gh pr view --json number --jq .number 2>/dev/null || true)
fi

ahead='unknown'
behind='unknown'
if [ -n "$branch" ] && git rev-parse --verify --quiet "origin/$branch" >/dev/null; then
  ahead=$(git rev-list --count "origin/$branch..HEAD")
  behind=$(git rev-list --count "HEAD..origin/$branch")
fi

review_file=''
if [ -n "$pr_number" ]; then
  review_file=$(scripts/review_path.sh "$pr_number")
elif [ -n "${WORKFLOW_REVIEW_FILE:-}" ]; then
  review_file="$WORKFLOW_REVIEW_FILE"
elif [ "${WORKFLOW_STATE_ALLOW_REVIEW_PATH_FALLBACK:-0}" = '1' ]; then
  # Opt-in only: the no-arg fallback may consult GitHub to predict the
  # next PR number, which is too expensive for the default quick probe.
  review_file=$(scripts/review_path.sh 2>/dev/null || true)
fi

review_summary='unknown'
local_review='unknown'
if [ -n "$review_file" ]; then
  if [ -f "$review_file" ]; then
    if grep -q '^## Summary[[:space:]]*$' "$review_file"; then
      review_summary='present'
    else
      review_summary='missing'
    fi
    if grep -q '^## Local review (' "$review_file"; then
      local_review='present'
    else
      local_review='missing'
    fi
  else
    review_summary='file-missing'
    local_review='file-missing'
  fi
fi

state='unknown'
if [ "$branch" = 'main' ]; then
  if [ -n "$status" ]; then
    state='main_dirty'
  elif [ "$ahead" != 'unknown' ] && [ "$ahead" -gt 0 ]; then
    state='main_unpushed'
  else
    state='main_clean'
  fi
elif [ -n "$status" ]; then
  state='working_tree_dirty'
elif [ "$ahead" != 'unknown' ] && [ "$ahead" -gt 0 ]; then
  state='round_unpushed'
elif [ -n "$pr_number" ]; then
  state='gh_review'
elif [ "$ahead" = '0' ] && [ "$behind" = '0' ] && [ "$local_review" = 'present' ]; then
  state='pushed'
elif [ "$local_review" = 'present' ]; then
  state='local_reviewed'
elif [ "$review_summary" = 'present' ]; then
  state='plan_finalized'
elif [ "$base_commits" = '0' ]; then
  state='on_branch'
elif [ "$base_commits" != 'unknown' ] && [ "$base_commits" -gt 0 ]; then
  state='impl_green'
fi

working_tree=dirty
if [ -z "$status" ]; then
  working_tree=clean
fi

printf 'state: %s\n' "$state"
printf 'branch: %s\n' "$branch"
printf 'origin_main_commits: %s\n' "$base_commits"
printf 'origin_branch_ahead: %s\n' "$ahead"
printf 'origin_branch_behind: %s\n' "$behind"
printf 'working_tree: %s\n' "$working_tree"
printf 'pr_number: %s\n' "${pr_number:-none}"
printf 'review_file: %s\n' "${review_file:-unknown}"
printf 'review_summary: %s\n' "$review_summary"
printf 'local_review: %s\n' "$local_review"
