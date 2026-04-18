"""Shared helpers for the scripts/pull_reviews.py and scripts/reply_review.py
GitHub-review CLIs.

Private sibling module so updates land in one place. Scripts invoked as
`scripts/foo.py` get `scripts/` on `sys.path[0]` automatically, which is
enough for `from _gh import ...` to resolve without any package setup.
"""

from __future__ import annotations

import os
import subprocess
import sys


def gh_repo() -> str:
    """Return the nameWithOwner of the repo for the current working directory."""
    try:
        return subprocess.check_output(
            ["gh", "repo", "view", "--json", "nameWithOwner", "--jq", ".nameWithOwner"],
            text=True,
            stderr=subprocess.PIPE,
        ).strip()
    except FileNotFoundError:
        print(
            "error: `gh` CLI not found; install GitHub CLI and ensure it is on PATH",
            file=sys.stderr,
        )
        raise SystemExit(1)
    except subprocess.CalledProcessError as exc:
        detail = (exc.stderr or "").strip()
        msg = f": {detail}" if detail else ""
        print(
            f"error: failed to determine repository via `gh repo view`{msg}",
            file=sys.stderr,
        )
        raise SystemExit(1)


def resolve_repo(pr: int, repo_override: str | None) -> str:
    """Pick the target repo, verifying the PR exists in it.

    If `--repo` was passed, trust it (explicit beats inferred).
    Otherwise auto-detect via `gh repo view` from cwd, then pre-flight
    `gh api repos/{repo}/pulls/{pr}`. On 404, error with both the
    detected repo and cwd so a user whose shell drifted into the wrong
    directory sees the mismatch immediately instead of getting an
    opaque 404 from the reply/review endpoints later.
    """
    if repo_override:
        return repo_override
    repo = gh_repo()
    try:
        subprocess.run(
            ["gh", "api", f"repos/{repo}/pulls/{pr}"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
            check=True,
        )
    except FileNotFoundError:
        print(
            "error: `gh` CLI not found; install GitHub CLI and ensure it is on PATH",
            file=sys.stderr,
        )
        raise SystemExit(1)
    except subprocess.CalledProcessError as exc:
        detail = (exc.stderr or "").strip()
        if "Not Found" in detail or "404" in detail:
            cwd = os.getcwd()
            lines = [
                f"error: couldn't verify PR #{pr} in {repo} "
                f"(repo detected from cwd: {cwd}).",
                "  The PR may be in a different repo — pass "
                "--repo owner/name to override.",
                "  A 404 here can also mean your gh token lacks access "
                "to this repo/PR.",
            ]
            if detail:
                lines.append(f"  gh api detail: {detail}")
            print("\n".join(lines), file=sys.stderr)
            raise SystemExit(1)
        msg = f": {detail}" if detail else ""
        print(
            f"error: couldn't verify PR #{pr} in {repo} via `gh api`{msg}",
            file=sys.stderr,
        )
        raise SystemExit(1)
    return repo
