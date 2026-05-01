#!/usr/bin/env bash
# safe_merge.sh — guard `gh pr merge` against the `round_unpushed`
# trap.
#
# `doc/workflow.md`'s state machine has no edge from `round_unpushed`
# to `merged`. The only path is `round_unpushed → push → gh_review →
# merged`. But `gh pr merge` is a GitHub-side operation; it doesn't
# know about local state. Merging while a round commit sits unpushed
# on the local branch silently drops it on the floor — the merge
# takes the remote head, and the local commit stays orphaned in the
# reflog.
#
# This script is the local-side enforcement: it resolves the PR's head
# branch (via `gh pr view`, *not* the currently-checked-out branch —
# safe_merge.sh 17 might be invoked from main), then refuses to invoke
# `gh pr merge` if that PR's local branch is ahead of its remote
# tracking ref. Re-run after `git push`.
#
# Usage:
#   scripts/safe_merge.sh [<gh-pr-merge-args...>]
#
# Examples:
#   scripts/safe_merge.sh 17                      # interactive
#   scripts/safe_merge.sh 17 --rebase --delete-branch
#   scripts/safe_merge.sh --rebase                # current branch's open PR
#
# All arguments are forwarded verbatim to `gh pr merge` after the
# guard passes.
set -euo pipefail

if ! command -v gh >/dev/null 2>&1; then
  echo "safe_merge.sh: gh CLI not found on PATH. Install GitHub CLI and authenticate before merging." >&2
  exit 1
fi

if [ $# -eq 1 ] && { [ "$1" = "-h" ] || [ "$1" = "--help" ]; }; then
  cat >&2 <<'USAGE'
usage: safe_merge.sh [<gh-pr-merge-args...>]

Resolves the PR's head branch via `gh pr view`, then refuses to run
if that local branch is ahead of its remote tracking ref. All
arguments are forwarded to `gh pr merge` once the guard passes.
USAGE
  exit 0
fi

# Resolve the PR's head ref. `gh pr view` accepts the same first-arg
# shapes as `gh pr merge` — number, URL, branch name, or no arg
# (defaulting to the current branch's open PR). To keep the guard and
# forwarded merge command checking the same PR, require any explicit
# selector to come before flags.
if [ $# -ge 1 ] && [ "${1#-}" != "$1" ]; then
  previous_takes_value=false
  for arg in "$@"; do
    if [ "$previous_takes_value" = true ]; then
      previous_takes_value=false
    elif [ "${arg#-}" = "$arg" ]; then
      echo "safe_merge.sh: PR selector must come before merge flags: $arg" >&2
      echo "  usage: scripts/safe_merge.sh [<pr>] [<gh-pr-merge-flags...>]" >&2
      exit 1
    else
      case "$arg" in
        -A|--author-email|-b|--body|-F|--body-file|--match-head-commit|-t|--subject|-R|--repo)
          previous_takes_value=true
          ;;
      esac
    fi
  done
fi

declare -a repo_args
repo_args=()
expect_repo_value=false
for arg in "$@"; do
  if [ "$expect_repo_value" = true ]; then
    repo_args+=("$arg")
    expect_repo_value=false
    continue
  fi
  case "$arg" in
    -R|--repo)
      repo_args+=("$arg")
      expect_repo_value=true
      ;;
    --repo=*)
      repo_args+=("$arg")
      ;;
  esac
done

declare -a pr_selector
pr_selector_text=''
if [ $# -ge 1 ] && [ "${1#-}" = "$1" ]; then
  pr_selector=("$1")
  pr_selector_text="$1"
else
  pr_selector=()
fi

if [ ${#pr_selector[@]} -gt 0 ]; then
  head_ref_cmd=(gh pr view "${pr_selector[@]}" "${repo_args[@]}" --json headRefName --jq .headRefName)
else
  head_ref_cmd=(gh pr view "${repo_args[@]}" --json headRefName --jq .headRefName)
fi

if ! head_ref=$("${head_ref_cmd[@]}" 2>/dev/null); then
  echo "safe_merge.sh: failed to resolve PR head ref via 'gh pr view $pr_selector_text'." >&2
  echo "  is the PR specifier valid, and are you authenticated to gh?" >&2
  exit 1
fi
if [ -z "$head_ref" ]; then
  echo "safe_merge.sh: 'gh pr view' returned empty headRefName." >&2
  exit 1
fi

# Refresh the remote tracking ref so the comparison isn't stale. A
# silent failure here (offline, auth) is fine — the next check will
# still compare against whatever's local, and the user will see if
# something's wrong.
git fetch --quiet origin "$head_ref" || true

upstream="origin/$head_ref"
if ! git rev-parse --verify --quiet "$upstream" >/dev/null; then
  echo "safe_merge.sh: no remote tracking ref '$upstream'." >&2
  echo "  push the branch first, then re-run." >&2
  exit 1
fi

# Resolve local branches that could contain unpushed commits for this
# PR head. Usually the local branch has the same name as the PR head,
# but a differently named branch can also track origin/<head_ref>.
local_refs=()
if local_sha=$(git rev-parse --verify --quiet "refs/heads/$head_ref"); then
  local_refs+=("$head_ref:$local_sha")
fi
while IFS=' ' read -r local_branch local_upstream; do
  if [ "$local_upstream" = "$upstream" ] && [ "$local_branch" != "$head_ref" ]; then
    local_sha=$(git rev-parse --verify "refs/heads/$local_branch")
    local_refs+=("$local_branch:$local_sha")
  fi
done < <(git for-each-ref --format='%(refname:short) %(upstream:short)' refs/heads)

if [ ${#local_refs[@]} -eq 0 ]; then
  exec gh pr merge "$@"
fi

for local_ref in "${local_refs[@]}"; do
  local_branch=${local_ref%%:*}
  local_sha=${local_ref#*:}
  ahead=$(git log "$upstream..$local_sha" --oneline)
  if [ -z "$ahead" ]; then
    continue
  fi
  cat >&2 <<EOF
safe_merge.sh: REFUSING TO MERGE — local branch '$local_branch' is ahead of $upstream.

Unpushed commits would be silently dropped by the merge:

$ahead

Per doc/workflow.md, the merge transition starts from gh_review (push
complete), not round_unpushed. Push first, then re-run:

EOF
  printf '    git push origin %q\n' "$head_ref" >&2
  printf '    %q' "$0" >&2
  printf ' %q' "$@" >&2
  printf '\n\n' >&2
  exit 1
done

exec gh pr merge "$@"
