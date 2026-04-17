#!/usr/bin/env bash
# Non-interactively rebase the current branch onto the provided base
# (default: origin/main), collapsing any `fixup!` commits into their
# targets via git's autosquash machinery.
#
# Use this before pushing a branch that accumulated CI-repair fixups so
# that main's linear history doesn't gain commits that temporarily broke
# the build.
#
# Usage:
#     scripts/autosquash.sh [base]
#
# `base` defaults to origin/main.
set -euo pipefail

base="${1:-origin/main}"

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "error: not inside a git worktree" >&2
  exit 1
fi

if [ -n "$(git status --porcelain)" ]; then
  echo "error: working tree not clean; commit or stash first" >&2
  exit 1
fi

if [[ "$base" == */* ]]; then
  remote="${base%%/*}"
  branch="${base#*/}"
  if git remote get-url "$remote" >/dev/null 2>&1 \
      && git check-ref-format --branch "$branch" >/dev/null 2>&1; then
    git fetch --quiet "$remote" "$branch"
  fi
fi

GIT_SEQUENCE_EDITOR=: git rebase -i --autosquash "$base"
