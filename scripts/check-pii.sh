#!/usr/bin/env bash
# Fail the commit if the staged diff introduces PII or likely secrets.
#
# Scans only lines the commit ADDS (drops unchanged context, deletions,
# and pre-existing content). Checks for:
#   - Absolute user-home paths: /Users/<name>/ (macOS), /home/<name>/
#     (Linux) — catches any committer, including CI runner paths
#   - Private-key headers: -----BEGIN ... PRIVATE KEY-----
#   - Cloud/API token shapes: AWS AKIA, GitHub ghp_, Anthropic/OpenAI sk-
#
# Allow-list exceptions live in `.pii-allow` (one extended regex per
# line; blank lines and `#`-comments ignored). A hit is dropped if the
# offending line matches any allow-list pattern, so exceptions stay
# explicit and reviewable. `/home/runner/` paths in CI docs are a
# typical allow-list candidate.
#
# Runtime is O(diff), not O(repo).
set -euo pipefail

patterns=(
  '/Users/[a-zA-Z0-9._-]+/'
  '/home/[a-zA-Z0-9._-]+/'
  '-----BEGIN [A-Z ]*PRIVATE KEY-----'
  'AKIA[0-9A-Z]{16}'
  'ghp_[A-Za-z0-9]{36}'
  'sk-[A-Za-z0-9]{20,}'
)
alt=$(IFS='|'; echo "${patterns[*]}")

report=''
# ACMR = Added / Copied / Modified / Renamed; excludes pure deletions.
# The script and the allow-list itself are skipped so self-inclusion
# of the patterns doesn't trip the check.
#
# Read names via NUL terminators so paths with spaces/newlines survive.
while IFS= read -r -d '' f; do
  [ -z "$f" ] && continue
  added=$(git diff --cached -U0 --no-color -- "$f" \
    | grep -E '^\+[^+]' | sed 's/^+//' || true)
  [ -z "$added" ] && continue

  matches=$(printf '%s\n' "$added" | grep -E "$alt" || true)
  [ -z "$matches" ] && continue

  if [ -f .pii-allow ]; then
    allow_patterns=$(grep -vE '^\s*(#|$)' .pii-allow || true)
    if [ -n "$allow_patterns" ]; then
      matches=$(printf '%s\n' "$matches" \
        | grep -vE -f <(printf '%s\n' "$allow_patterns") || true)
    fi
  fi
  [ -z "$matches" ] && continue

  report+="  $f:"$'\n'
  while IFS= read -r line; do
    report+="    $line"$'\n'
  done <<< "$matches"
done < <(
  git diff --cached --name-only -z --diff-filter=ACMR -- \
    . ':(exclude)scripts/check-pii.sh' ':(exclude).pii-allow'
)

if [ -z "$report" ]; then
  exit 0
fi

{
  echo "error: staged diff contains potential PII or secrets:"
  printf '%s' "$report"
  echo "  If these are false positives, add a regex to .pii-allow and re-stage."
} >&2
exit 1
