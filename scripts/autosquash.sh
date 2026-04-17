#!/usr/bin/env bash
# Non-interactively rebase the current branch onto origin/main, collapsing
# any `fixup!` commits into their targets via git's autosquash machinery.
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

if [ -n "$(git status --porcelain)" ]; then
  echo "error: working tree not clean; commit or stash first" >&2
  exit 1
fi

git fetch --quiet origin
GIT_SEQUENCE_EDITOR=: git rebase -i --autosquash "$base"
