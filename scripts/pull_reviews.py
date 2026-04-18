#!/usr/bin/env python3
"""Fetch GitHub PR review bodies and inline comments and append them
chronologically to `doc/reviews/review-NNNN.md`.

Idempotent via set-membership on `<!-- gh-id: N -->` markers: any item
whose id is already present in the target file is skipped. This avoids
the trap of a single "high-water mark" — GitHub assigns review IDs and
inline-comment IDs from different sequences, so a max-id across both
would silently drop later items from the lower-numbered sequence.

Paginated via explicit `?per_page=100&page=N` iteration (not
`gh api --paginate --slurp`, which needs gh >= 2.47), so PRs with
more than one page of items are fetched fully on any gh version.

Requires: `gh` CLI authenticated for the current repo.

Usage:
    scripts/pull_reviews.py <PR_NUMBER> [--repo owner/name] [--out doc/reviews]
"""

from __future__ import annotations

import argparse
import datetime
import json
import pathlib
import re
import subprocess
import sys

from _gh import resolve_repo


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


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("pr", type=int, help="PR number")
    ap.add_argument("--repo", default=None, help="owner/name (default: auto)")
    ap.add_argument(
        "--out",
        default="doc/reviews",
        help="directory for review-NNNN.md (default: doc/reviews)",
    )
    args = ap.parse_args()

    repo = resolve_repo(args.pr, args.repo)
    out_dir = pathlib.Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    path = out_dir / f"review-{args.pr:04d}.md"

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


if __name__ == "__main__":
    sys.exit(main())
