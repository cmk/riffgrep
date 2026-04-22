#!/usr/bin/env bash
# Extracts the PR body from doc/reviews/review-NNNNN.md.
#
# The review file's `## Summary` section is the single source of truth
# for the PR body (see CLAUDE.md "Tier 1 — Local review"). This script
# extracts that section so `gh pr create --body-file` can feed it
# straight to GitHub:
#
#     gh pr create --title "..." \
#       --body-file <(scripts/extract_pr_body.sh 17)
#
# Content is taken between the `## Summary` heading (exclusive) and
# the first review-round marker (exclusive) — `## Local review` from
# /sprint-review or `<!-- gh-id: N -->` from pull_reviews.py. The
# extracted region is written to stdout; bash's command substitution
# normalizes trailing newlines, so a caller using `<(...)` sees the
# body minus any trailing blank lines.
#
# Fails loudly with a nonzero exit and a message on stderr if:
#   - the argument is missing or not numeric
#   - the review file doesn't exist
#   - the `## Summary` section is missing
#   - the `## Summary` section is empty (whitespace only)
#
# These checks prevent silently opening a PR with a blank body. Run
# from the repo root.
set -euo pipefail

if [ $# -ne 1 ] || [ -z "${1:-}" ]; then
  echo "usage: $0 <pr-number>" >&2
  exit 1
fi

if ! [[ "$1" =~ ^[0-9]+$ ]]; then
  echo "error: pr number must be numeric: '$1'" >&2
  exit 1
fi

# Force base-10 so zero-padded inputs like `00017` don't trigger bash's
# octal interpretation (which would silently misroute `00017` → `00015`
# and error on digits 8-9).
n=$(printf '%05d' "$((10#$1))")
file="doc/reviews/review-${n}.md"

if [ ! -f "$file" ]; then
  echo "error: review file not found: $file" >&2
  echo "  run TDD step 7 to create it (finalize plan + draft PR description)." >&2
  exit 1
fi

# Extract between `## Summary` (exclusive) and the first review-round
# marker (exclusive). Stopping at review markers (rather than any
# `## ` heading) lets the PR body contain sibling sections like
# `## Test plan` without being truncated.
if ! body=$(awk '
  !found && /^## Summary[[:space:]]*$/                  { in_s = 1; found = 1; next }
  in_s && (/^## Local review \(/ || /^<!-- gh-id: /)    { in_s = 0 }
  in_s                                                  { print }
  END                                                   { if (!found) exit 2 }
' "$file"); then
  echo "error: '## Summary' section not found in $file" >&2
  echo "  write the PR body under '## Summary' before opening the PR." >&2
  exit 1
fi

if ! printf '%s' "$body" | grep -q '[^[:space:]]'; then
  echo "error: '## Summary' section in $file is empty" >&2
  echo "  write the PR body under '## Summary' before opening the PR." >&2
  exit 1
fi

printf '%s\n' "$body"
