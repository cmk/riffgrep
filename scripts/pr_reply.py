#!/usr/bin/env python3
"""Post a reply to a PR review comment thread on GitHub.

Wraps `gh api repos/{repo}/pulls/{pr}/comments/{id}/replies -f body=...`
so the agent doesn't have to remember the endpoint shape. Prints the
new comment's id and html_url on success.

Requires: `gh` CLI authenticated for the current repo.

Usage:
    scripts/pr_reply.py <PR> <IN_REPLY_TO_ID> <BODY>
    scripts/pr_reply.py <PR> <IN_REPLY_TO_ID> -    # read body from stdin
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys

from github_client import resolve_repo


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("pr", type=int, help="PR number")
    ap.add_argument(
        "in_reply_to_id",
        type=int,
        help="gh-id of the comment you are replying to",
    )
    ap.add_argument(
        "body",
        help="Reply body (markdown). Pass '-' to read from stdin.",
    )
    ap.add_argument("--repo", default=None, help="owner/name (default: auto)")
    args = ap.parse_args()

    body = sys.stdin.read() if args.body == "-" else args.body
    body = body.strip()
    if not body:
        print("error: empty body", file=sys.stderr)
        return 1

    repo = resolve_repo(args.pr, args.repo)
    path = f"repos/{repo}/pulls/{args.pr}/comments/{args.in_reply_to_id}/replies"

    try:
        result = subprocess.run(
            ["gh", "api", "--method", "POST", path, "-f", f"body={body}"],
            capture_output=True,
            text=True,
            check=True,
        )
    except FileNotFoundError:
        print(
            "error: `gh` CLI not found; install GitHub CLI and ensure it is on PATH",
            file=sys.stderr,
        )
        return 1
    except subprocess.CalledProcessError as exc:
        if exc.stderr:
            sys.stderr.write(exc.stderr)
        return exc.returncode or 1

    data = json.loads(result.stdout)
    print(f"posted reply id={data['id']} url={data['html_url']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
