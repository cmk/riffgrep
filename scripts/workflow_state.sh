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

cat <<EOF
state: $state
branch: $branch
origin_main_commits: $base_commits
origin_branch_ahead: $ahead
origin_branch_behind: $behind
working_tree: $(if [ -z "$status" ]; then printf clean; else printf dirty; fi)
pr_number: ${pr_number:-none}
review_file: ${review_file:-unknown}
review_summary: $review_summary
local_review: $local_review
EOF
