#!/usr/bin/env python3
"""Check that Org exports and committed RST change by the same words."""

from __future__ import annotations

import argparse
import difflib
import pathlib
import re
import subprocess
import sys


def changed_words(before: list[str], after: list[str]) -> list[tuple[list[str], list[str]]]:
    """Return the replaced word spans, without unchanged context."""
    matcher = difflib.SequenceMatcher(None, before, after, autojunk=False)
    return [
        (before[first:last], after[new_first:new_last])
        for tag, first, last, new_first, new_last in matcher.get_opcodes()
        if tag != "equal"
    ]


def exports_match_changes(
    base_committed: list[str],
    head_committed: list[str],
    base_export: list[str],
    head_export: list[str],
) -> bool:
    """Whether committed and exported documents received the same edits."""
    return changed_words(base_committed, head_committed) == changed_words(
        base_export, head_export
    )


def rst_words(data: bytes) -> list[str]:
    plain = subprocess.run(
        ["pandoc", "-f", "rst", "-t", "plain", "--wrap=none"],
        input=data,
        check=True,
        capture_output=True,
    ).stdout.decode()
    return re.findall(r"\w+", plain, flags=re.UNICODE)


def committed(root: pathlib.Path, ref: str, relative: str) -> bytes:
    shown = subprocess.run(
        ["git", "-C", str(root), "show", f"{ref}:{relative}"],
        capture_output=True,
    )
    if shown.returncode != 0:
        return b""
    return shown.stdout


def exported(path: pathlib.Path) -> bytes:
    """Read an export, or the empty document when that revision has no file."""
    try:
        return path.read_bytes()
    except FileNotFoundError:
        return b""


def rst_names(root: pathlib.Path, base_ref: str, head_ref: str) -> list[str]:
    names: set[str] = set()
    for ref in [base_ref, head_ref]:
        listed = subprocess.run(
            ["git", "-C", str(root), "ls-tree", "-r", "--name-only", ref, "docs/source"],
            check=True,
            capture_output=True,
            text=True,
        )
        names.update(line for line in listed.stdout.splitlines() if line.endswith(".rst"))
    return sorted(names)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=pathlib.Path, required=True)
    parser.add_argument("--base-ref", required=True)
    parser.add_argument("--head-ref", required=True)
    parser.add_argument("--base-export", type=pathlib.Path, required=True)
    parser.add_argument("--head-export", type=pathlib.Path, required=True)
    parser.add_argument(
        "--only",
        action="append",
        help="check only this docs/source path; may be given more than once",
    )
    args = parser.parse_args()

    names = rst_names(args.root, args.base_ref, args.head_ref)
    if args.only is not None:
        names = [relative for relative in names if relative in args.only]
    mismatches = []
    for relative in names:
        name = pathlib.Path(relative).name
        base_export = args.base_export / name
        head_export = args.head_export / name
        if not exports_match_changes(
            rst_words(committed(args.root, args.base_ref, relative)),
            rst_words(committed(args.root, args.head_ref, relative)),
            rst_words(exported(base_export)),
            rst_words(exported(head_export)),
        ):
            mismatches.append(relative)

    for relative in mismatches:
        print(
            f"check_docs_export: committed {relative} differs from its Org export",
            file=sys.stderr,
        )
    if mismatches:
        return 1
    print("check_docs_export: committed RST follows the Org export")
    return 0


if __name__ == "__main__":
    sys.exit(main())
