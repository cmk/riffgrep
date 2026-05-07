from __future__ import annotations

import os
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


REPO_ROOT = Path(__file__).resolve().parents[1]
WORKFLOW_STATE_PATH = REPO_ROOT / "scripts" / "workflow_state.sh"


def git(repo: Path, *args: str) -> None:
    subprocess.run(["git", *args], cwd=repo, check=True, capture_output=True, text=True)


def run_state(repo: Path, script: Path, path_prefix: Path) -> dict[str, str]:
    env = os.environ.copy()
    env.pop("WORKFLOW_REVIEW_FILE", None)
    env.pop("WORKFLOW_STATE_ALLOW_REVIEW_PATH_FALLBACK", None)
    base_path = env.get("PATH", "")
    env["PATH"] = f"{path_prefix}{os.pathsep}{base_path}" if base_path else str(path_prefix)
    result = subprocess.run(
        [str(script)],
        cwd=repo,
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )
    return dict(line.split(": ", 1) for line in result.stdout.strip().splitlines())


class WorkflowStateTests(unittest.TestCase):
    def test_clean_pushed_branch_without_gh_reports_pushed_without_review_file(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            remote = root / "origin.git"
            repo = root / "repo"

            subprocess.run(["git", "init", "--bare", remote], check=True, capture_output=True)
            repo.mkdir()
            git(repo, "init", "--initial-branch=main")
            git(repo, "config", "user.name", "Workflow Test")
            git(repo, "config", "user.email", "workflow@example.invalid")

            scripts = repo / "scripts"
            bin_dir = root / "bin"
            scripts.mkdir(parents=True)
            bin_dir.mkdir()
            gh = bin_dir / "gh"
            gh.write_text(f"#!{sys.executable}\nimport sys\nsys.exit(1)\n", encoding="utf-8")
            gh.chmod(gh.stat().st_mode | stat.S_IXUSR)
            script = scripts / "workflow_state.sh"
            script.write_text(WORKFLOW_STATE_PATH.read_text(encoding="utf-8"), encoding="utf-8")
            script.chmod(script.stat().st_mode | stat.S_IXUSR)
            (repo / "README.md").write_text("seed\n", encoding="utf-8")

            git(repo, "add", ".")
            git(repo, "commit", "-m", "initial")
            git(repo, "remote", "add", "origin", str(remote))
            git(repo, "push", "-u", "origin", "main")

            branch = "flat-fallback"
            git(repo, "switch", "-c", branch)
            (repo / "README.md").write_text("branch work\n", encoding="utf-8")
            git(repo, "add", "README.md")
            git(repo, "commit", "-m", "feat: branch work")
            git(repo, "push", "-u", "origin", branch)

            with mock.patch.dict(
                os.environ,
                {
                    "WORKFLOW_REVIEW_FILE": "doc/reviews/review-99999.md",
                    "WORKFLOW_STATE_ALLOW_REVIEW_PATH_FALLBACK": "1",
                },
            ):
                fields = run_state(repo, script, bin_dir)
            self.assertEqual(fields["state"], "pushed")
            self.assertEqual(fields["origin_branch_ahead"], "0")
            self.assertEqual(fields["origin_branch_behind"], "0")
            self.assertEqual(fields["review_file"], "unknown")
            self.assertEqual(fields["local_review"], "unknown")


if __name__ == "__main__":
    unittest.main()
