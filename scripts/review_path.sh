#!/usr/bin/env bash
# Prints the review file path for a PR: `doc/reviews/review-NNNNN.md`
# with `NNNNN` zero-padded to 5 digits.
#
# Source of truth for the review-file naming convention. Callers that
# don't want to know about padding should use this instead of composing
# `doc/reviews/review-$(printf '%05d' ...).md` themselves.
#
# Usage:
#     scripts/review_path.sh              — path for the next-to-be-opened PR
#                                           (calls scripts/next_pr_number.sh)
#     scripts/review_path.sh <pr-number>  — path for a given PR number
#
# Accepts both unpadded (`17`) and zero-padded (`00017`) input; bash's
# octal interpretation is defused via `$((10#$n))`.
#
# Run from the repo root (output path is relative to CWD).
set -euo pipefail

if [ $# -gt 1 ]; then
  echo "usage: $0 [<pr-number>]" >&2
  exit 1
fi

if [ $# -eq 1 ]; then
  if ! [[ "$1" =~ ^[0-9]+$ ]]; then
    echo "error: pr number must be numeric: '$1'" >&2
    exit 1
  fi
  n="$1"
else
  script_dir=$(cd "$(dirname "$0")" && pwd)
  if ! n=$("$script_dir/next_pr_number.sh"); then
    echo "error: scripts/next_pr_number.sh failed; pass a pr number explicitly" >&2
    exit 1
  fi
fi

printf 'doc/reviews/review-%05d.md\n' "$((10#$n))"
