#!/usr/bin/env python3

import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

from check_export_sync import exports_match_changes

ROOT = pathlib.Path(__file__).resolve().parents[2]


class ExportSyncTests(unittest.TestCase):
    def test_committed_rst_drift_is_refused(self) -> None:
        base = "any project name is checked before a file is written".split()
        exported = "any safe project name is checked before a file is written".split()

        self.assertFalse(exports_match_changes(base, base, base, exported))

    def test_matching_edits_are_accepted(self) -> None:
        base = "the tracker and deed store are set up".split()
        updated = "the tracker is set up".split()

        self.assertTrue(exports_match_changes(base, updated, base, updated))

    def test_new_rst_matching_its_export_is_accepted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = pathlib.Path(directory)
            repo = fixture / "repo"
            base_export = fixture / "base-export"
            head_export = fixture / "head-export"
            (repo / "docs/source").mkdir(parents=True)
            base_export.mkdir()
            head_export.mkdir()
            subprocess.run(["git", "init", "--quiet", str(repo)], check=True)
            subprocess.run(
                ["git", "-C", str(repo), "config", "user.name", "Docs Test"],
                check=True,
            )
            subprocess.run(
                ["git", "-C", str(repo), "config", "user.email", "docs@example.test"],
                check=True,
            )
            (repo / "docs/source/a.rst").write_text("hello world\n")
            (base_export / "a.rst").write_text("hello world\n")
            (head_export / "a.rst").write_text("hello world\n")
            subprocess.run(["git", "-C", str(repo), "add", "."], check=True)
            subprocess.run(
                ["git", "-C", str(repo), "commit", "--quiet", "-m", "base"],
                check=True,
            )
            base = subprocess.run(
                ["git", "-C", str(repo), "rev-parse", "HEAD"],
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
            (repo / "docs/source/b.rst").write_text("new page\n")
            (head_export / "b.rst").write_text("new page\n")
            subprocess.run(["git", "-C", str(repo), "add", "."], check=True)
            subprocess.run(
                ["git", "-C", str(repo), "commit", "--quiet", "-m", "head"],
                check=True,
            )
            checked = subprocess.run(
                [
                    "python3",
                    str(ROOT / "docs/scripts/check_export_sync.py"),
                    "--root",
                    str(repo),
                    "--base-ref",
                    base,
                    "--head-ref",
                    "HEAD",
                    "--base-export",
                    str(base_export),
                    "--head-export",
                    str(head_export),
                ],
                capture_output=True,
                text=True,
            )

        self.assertEqual(checked.returncode, 0, checked.stdout + checked.stderr)

    def test_only_checks_the_named_pages(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = pathlib.Path(directory)
            repo = fixture / "repo"
            base_export = fixture / "base-export"
            head_export = fixture / "head-export"
            (repo / "docs/source").mkdir(parents=True)
            base_export.mkdir()
            head_export.mkdir()
            subprocess.run(["git", "init", "--quiet", str(repo)], check=True)
            for page in ["a.rst", "b.rst"]:
                (repo / "docs/source" / page).write_text("hello world\n")
                (base_export / page).write_text("hello world\n")
                (head_export / page).write_text("hello world\n")
            (head_export / "a.rst").write_text("hello new world\n")
            subprocess.run(["git", "-C", str(repo), "add", "."], check=True)
            subprocess.run(
                [
                    "git",
                    "-C",
                    str(repo),
                    "-c",
                    "user.name=Docs Test",
                    "-c",
                    "user.email=docs@example.test",
                    "commit",
                    "--quiet",
                    "-m",
                    "base",
                ],
                check=True,
            )

            def check(page: str) -> int:
                return subprocess.run(
                    [
                        "python3",
                        str(ROOT / "docs/scripts/check_export_sync.py"),
                        "--root",
                        str(repo),
                        "--base-ref",
                        "HEAD",
                        "--head-ref",
                        "HEAD",
                        "--base-export",
                        str(base_export),
                        "--head-export",
                        str(head_export),
                        "--only",
                        f"docs/source/{page}",
                    ],
                    capture_output=True,
                ).returncode

            self.assertEqual(check("a.rst"), 1)
            self.assertEqual(check("b.rst"), 0)

    def test_shallow_topic_clone_fetches_develop_for_comparison(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = pathlib.Path(directory)
            bare = fixture / "repo.git"
            clone = fixture / "clone"
            subprocess.run(
                ["git", "clone", "--quiet", "--bare", str(ROOT), str(bare)],
                check=True,
            )
            subprocess.run(
                ["git", "--git-dir", str(bare), "branch", "-f", "develop", "HEAD^"],
                check=True,
            )
            subprocess.run(
                ["git", "--git-dir", str(bare), "branch", "-f", "topic", "HEAD"],
                check=True,
            )
            subprocess.run(
                ["git", "--git-dir", str(bare), "symbolic-ref", "HEAD", "refs/heads/develop"],
                check=True,
            )
            subprocess.run(
                ["git", "--git-dir", str(bare), "branch", "-D", "work"],
                check=False,
                capture_output=True,
            )
            subprocess.run(
                [
                    "git",
                    "clone",
                    "--quiet",
                    "--depth",
                    "1",
                    "--branch",
                    "topic",
                    f"file://{bare}",
                    str(clone),
                ],
                check=True,
            )
            shutil.copy(
                ROOT / "docs/scripts/check_docs_export.sh",
                clone / "docs/scripts/check_docs_export.sh",
            )
            checked = subprocess.run(
                ["bash", "docs/scripts/check_docs_export.sh", "--resolve-base"],
                cwd=clone,
                env={**os.environ, "CHECK_DOCS_EXPORT_REQUIRED": "1"},
                capture_output=True,
                text=True,
            )

        self.assertEqual(checked.returncode, 0, checked.stdout + checked.stderr)
        self.assertTrue(checked.stdout.strip(), checked.stdout + checked.stderr)


if __name__ == "__main__":
    unittest.main()
