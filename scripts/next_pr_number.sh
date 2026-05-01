#!/usr/bin/env bash
# Prints the number that will be assigned to the *next* PR (or issue)
# opened in the current repo.
#
# GitHub uses a single sequence per repo for both issues and pull
# requests, so the next PR number is:
#
#     max(last_issue_number, last_pr_number) + 1
#
# The REST `issues` endpoint returns both issues and PRs (PRs are
# issues with a `pull_request` key), so `state=all` ordered by
# creation gives the current maximum in a single call.
#
# Used by the local review transition to name
# `doc/reviews/review-NNNNN.md` before the PR is opened. The prediction
# is usually final, but if another issue or PR is opened between the
# prediction and the PR being created, the number drifts and the review
# file has to be renamed to match the number GitHub actually assigned.
#
# Usage:
#     scripts/next_pr_number.sh [owner/name]
#
# If `owner/name` is omitted, the repo is inferred via `gh repo view`.
#
# Requires: `gh` authenticated for the target repo. Prints only the
# number on stdout (so callers can capture it with `$(…)`); diagnostics
# go to stderr.
set -euo pipefail

if [ $# -ge 1 ] && [ -n "$1" ]; then
  repo="$1"
else
  if ! repo=$(gh repo view --json nameWithOwner --jq .nameWithOwner 2>/dev/null); then
    echo "error: could not determine repo; pass owner/name or run inside a gh-configured clone" >&2
    exit 1
  fi
fi

if ! last=$(gh api -X GET "repos/$repo/issues" \
    -f state=all -f per_page=1 -f sort=created -f direction=desc \
    --jq '.[0].number // 0' 2>/dev/null); then
  echo "error: gh api failed for repos/$repo/issues (check auth and network)" >&2
  exit 1
fi

printf '%d\n' "$((last + 1))"
