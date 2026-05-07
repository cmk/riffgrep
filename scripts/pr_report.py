#!/usr/bin/env python3
"""Report PR workflow paths, bodies, and GitHub review items.

Subcommands:
    path [PR_NUMBER]
        Print `doc/reviews/review-NNNNN.md`. Without a number, asks
        `scripts/pr_request.sh` for the next predicted PR number.

    body PR_NUMBER
        Extract the PR body from the review file's `## Summary` section.

    reviews PR_NUMBER [--repo owner/name] [--out doc/reviews]
        Fetch GitHub PR review bodies and inline comments and append
        them chronologically to `doc/reviews/review-NNNNN.md`.

The `reviews` subcommand is idempotent via set-membership on
`<!-- gh-id: N -->` markers: any item whose id is already present in
the target file is skipped. This avoids the trap of a single "high-water
mark" — GitHub assigns review IDs and inline-comment IDs from different
sequences, so a max-id across both would silently drop later items from
the lower-numbered sequence.

Paginated via explicit `?per_page=100&page=N` iteration (not
`gh api --paginate --slurp`, which needs gh >= 2.47), so PRs with
more than one page of items are fetched fully on any gh version.

The `path` subcommand without a PR number and the `reviews` subcommand
require `gh` CLI authentication.

Usage:
    scripts/pr_report.py path [PR_NUMBER]
    scripts/pr_report.py body PR_NUMBER
    scripts/pr_report.py reviews <PR_NUMBER> [--repo owner/name] [--out doc/reviews]
"""

from __future__ import annotations

import argparse
import datetime
import json
import pathlib
import re
import subprocess
import sys

from github_client import resolve_repo

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
DEFAULT_REVIEW_DIR = REPO_ROOT / "doc" / "reviews"


def parse_pr(value: str) -> int:
    if not re.fullmatch(r"[0-9]+", value):
        raise argparse.ArgumentTypeError(f"PR number must be numeric: {value!r}")
    return int(value, 10)


def review_path(n: int, out: pathlib.Path = DEFAULT_REVIEW_DIR) -> pathlib.Path:
    return out / f"review-{n:05d}.md"


def repo_path(value: str) -> pathlib.Path:
    path = pathlib.Path(value)
    return path if path.is_absolute() else REPO_ROOT / path


def request_next_pr_number() -> int:
    script = pathlib.Path(__file__).resolve().parent / "pr_request.sh"
    try:
        raw = subprocess.check_output([str(script)], text=True, stderr=subprocess.PIPE)
    except FileNotFoundError:
        print(f"error: missing helper: {script}", file=sys.stderr)
        raise SystemExit(1)
    except subprocess.CalledProcessError as exc:
        detail = (exc.stderr or "").strip()
        msg = f": {detail}" if detail else ""
        print(
            f"error: scripts/pr_request.sh failed; pass a PR number explicitly{msg}",
            file=sys.stderr,
        )
        raise SystemExit(1)
    return int(raw.strip(), 10)


def extract_body(path: pathlib.Path) -> str:
    if not path.exists():
        print(f"error: review file not found: {path}", file=sys.stderr)
        print(
            "  run TDD step 7 to create it (finalize plan + draft PR description).",
            file=sys.stderr,
        )
        raise SystemExit(1)

    found = False
    in_summary = False
    lines: list[str] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if not found and re.fullmatch(r"## Summary\s*", line):
            found = True
            in_summary = True
            continue
        if in_summary and (
            line.startswith("## Local review (") or line.startswith("<!-- gh-id: ")
        ):
            in_summary = False
        if in_summary:
            lines.append(line)

    if not found:
        print(f"error: '## Summary' section not found in {path}", file=sys.stderr)
        print("  write the PR body under '## Summary' before opening the PR.", file=sys.stderr)
        raise SystemExit(1)

    body = "\n".join(lines).strip()
    if not body:
        print(f"error: '## Summary' section in {path} is empty", file=sys.stderr)
        print("  write the PR body under '## Summary' before opening the PR.", file=sys.stderr)
        raise SystemExit(1)
    return body


def gh_api(path: str) -> list | dict:
    """Fetch a list endpoint, iterating pages explicitly.

    We don't use `gh api --paginate --slurp` because `--slurp` needs gh
    >= 2.47. Explicit `?page=N&per_page=100` iteration works on every
    version and is trivially inspectable.

    If the endpoint returns a dict (non-list), we return it as-is from
    page 1 without continuing to page.
    """
    all_items: list = []
    page = 1
    while True:
        sep = "&" if "?" in path else "?"
        paged = f"{path}{sep}per_page=100&page={page}"
        try:
            raw = json.loads(
                subprocess.check_output(
                    ["gh", "api", paged], text=True, stderr=subprocess.PIPE
                )
            )
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
                f"error: `gh api` failed for endpoint `{paged}`{msg}", file=sys.stderr
            )
            raise SystemExit(1)
        if not isinstance(raw, list):
            return raw
        if not raw:
            break
        all_items.extend(raw)
        if len(raw) < 100:
            break
        page += 1
    return all_items


def pr_title(n: int, repo: str | None = None) -> str:
    cmd = ["gh", "pr", "view", str(n)]
    if repo is not None:
        cmd.extend(["--repo", repo])
    cmd.extend(["--json", "title", "--jq", ".title"])
    try:
        return subprocess.check_output(cmd, text=True, stderr=subprocess.PIPE).strip()
    except FileNotFoundError:
        print(
            "error: `gh` CLI not found; install GitHub CLI and ensure it is on PATH",
            file=sys.stderr,
        )
        raise SystemExit(1)
    except subprocess.CalledProcessError as exc:
        detail = (exc.stderr or "").strip()
        msg = f": {detail}" if detail else ""
        target = f"PR #{n}" + (f" in {repo}" if repo else "")
        print(
            f"error: failed to determine title for {target} via `gh pr view`{msg}",
            file=sys.stderr,
        )
        raise SystemExit(1)


def fmt_ts(t: str) -> str:
    d = datetime.datetime.fromisoformat(t.replace("Z", "+00:00"))
    return d.strftime("%Y-%m-%d %H:%M UTC")


def existing_ids(path: pathlib.Path) -> set[int]:
    if not path.exists():
        return set()
    return {
        int(h)
        for h in re.findall(r"<!-- gh-id: (\d+) -->", path.read_text(encoding="utf-8"))
    }


def absolutize(body: str) -> str:
    """Rewrite relative GitHub links so they resolve outside github.com."""
    body = re.sub(r'href="/([^"]*)"', r'href="https://github.com/\1"', body)
    body = re.sub(r"href='/([^']*)'", r"href='https://github.com/\1'", body)
    body = re.sub(r"\]\(/(?!/)([^)]*)\)", r"](https://github.com/\1)", body)
    return body


def collect_items(repo: str, n: int) -> list[dict]:
    items: list[dict] = []
    for r in gh_api(f"repos/{repo}/pulls/{n}/reviews"):
        if not r.get("body"):
            continue
        submitted_at = r.get("submitted_at")
        if not submitted_at:
            continue
        items.append(
            {
                "kind": "review",
                "ts": submitted_at,
                "id": r["id"],
                "user": r["user"]["login"],
                "state": r["state"],
                "body": r["body"],
                "html_url": r["html_url"],
            }
        )
    for c in gh_api(f"repos/{repo}/pulls/{n}/comments"):
        items.append(
            {
                "kind": "comment",
                "ts": c["created_at"],
                "id": c["id"],
                "user": c["user"]["login"],
                "path": c["path"],
                "line": c.get("line"),
                "body": c["body"],
                "in_reply_to_id": c.get("in_reply_to_id"),
                "html_url": c["html_url"],
            }
        )
    items.sort(key=lambda x: x["ts"])
    return items


def render(it: dict) -> str:
    ts = fmt_ts(it["ts"])
    body = absolutize(it["body"])
    url = it["html_url"]
    out = [f"\n<!-- gh-id: {it['id']} -->"]
    if it["kind"] == "review":
        out.append(f"### {it['user']} — {it['state']} ([{ts}]({url}))")
    else:
        loc = it["path"] + (f":{it['line']}" if it["line"] else "")
        if it.get("in_reply_to_id"):
            out.append(f"#### ↳ {it['user']} ([{ts}]({url}))")
        else:
            out.append(f"### {it['user']} on [`{loc}`]({url}) ({ts})")
    out.append("")
    out.append(body)
    return "\n".join(out) + "\n"


def cmd_path(args: argparse.Namespace) -> int:
    pr = args.pr if args.pr is not None else request_next_pr_number()
    print(review_path(pr).relative_to(REPO_ROOT))
    return 0


def cmd_body(args: argparse.Namespace) -> int:
    print(extract_body(review_path(args.pr)))
    return 0


def cmd_reviews(args: argparse.Namespace) -> int:
    repo = resolve_repo(args.pr, args.repo)
    out_dir = repo_path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    path = review_path(args.pr, out_dir)

    if not path.exists():
        path.write_text(
            f"# PR #{args.pr} — {pr_title(args.pr, repo)}\n", encoding="utf-8"
        )

    seen = existing_ids(path)
    new_items = [it for it in collect_items(repo, args.pr) if it["id"] not in seen]

    if not new_items:
        print(f"PR #{args.pr}: no new items ({len(seen)} already recorded)")
        return 0

    with path.open("a", encoding="utf-8") as f:
        for it in new_items:
            f.write(render(it))

    print(f"PR #{args.pr}: appended {len(new_items)} items -> {path}")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)

    p_path = sub.add_parser("path", help="print the review file path")
    p_path.add_argument("pr", nargs="?", type=parse_pr, help="PR number")
    p_path.set_defaults(func=cmd_path)

    p_body = sub.add_parser("body", help="extract the PR body from ## Summary")
    p_body.add_argument("pr", type=parse_pr, help="PR number")
    p_body.set_defaults(func=cmd_body)

    p_reviews = sub.add_parser("reviews", help="mirror GitHub review items")
    p_reviews.add_argument("pr", type=parse_pr, help="PR number")
    p_reviews.add_argument("--repo", default=None, help="owner/name (default: auto)")
    p_reviews.add_argument(
        "--out",
        default="doc/reviews",
        help="directory for review-NNNNN.md (default: doc/reviews)",
    )
    p_reviews.set_defaults(func=cmd_reviews)

    args = ap.parse_args()
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
