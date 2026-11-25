#!/usr/bin/env python3
"""Fail when the changelog does not match the version bump it ships under.

CHANGELOG.md says a minor bump is a feature and a patch is a fix. Nothing
tied the two together: towncrier puts every `feature` fragment in a
"Features" section whatever version `towncrier build --version` is given,
so v0.45.4, a patch, shipped a Features section of eight entries. This
reads the workspace version, the changelog and the pending fragments and
checks:

1. A release whose patch number is not 0 has no "Features" or "Removals"
   section. A feature or a removal needs a minor bump.
2. When the workspace version is a patch that has no section in the
   changelog yet (the release is being prepared and `towncrier build` has
   not run), no `feature` or `removal` fragment is waiting. Move the
   fragment to the next minor bump, or retype it.
3. One release section, or the pending fragments together, does not state
   the same change twice: the same bullet twice, or the same RUSTSEC, GHSA
   or CVE advisory in two bullets. v0.45.4 said url 2.5.9 twice, once under
   Fixes and once under Miscellaneous, from two fragments for one bump.

Releases up to and including BASELINE shipped before this check and are not
held to rule 1. Many of them, 0.45.4 among them, have a Features section
under a patch number, and they are tagged. BASELINE does not move.

    python3 scripts/changelog-check.py
    python3 scripts/changelog-check.py --selftest

Run it from the checkout root.
"""

from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# The last release made before this check. Not to be moved.
BASELINE = (0, 45, 4)

MINOR_ONLY = ("Features", "Removals")
FRAGMENT_MINOR_ONLY = ("feature", "removal")
HEADING = re.compile(r"^## (\d+)\.(\d+)\.(\d+) \(")
SECTION = re.compile(r"^### (.+?)\s*$")
ADVISORY = re.compile(r"\b(RUSTSEC-\d{4}-\d{4}|GHSA(?:-[0-9a-z]{4}){3}|CVE-\d{4}-\d{4,})\b", re.I)
FRAGMENT = re.compile(r"^(?P<issue>.+?)\.(?P<type>[a-z]+)(?:\.\d+)?\.md$")


def parse_changelog(text: str):
    """Releases as (name, (major, minor, patch), {section: [bullet, ...]}), newest first."""
    releases = []
    section = None
    for line in text.splitlines():
        heading = HEADING.match(line)
        if heading:
            major, minor, patch = heading.groups()
            releases.append((f"{major}.{minor}.{patch}", (int(major), int(minor), int(patch)), {}))
            section = None
            continue
        if not releases:
            continue
        found = SECTION.match(line)
        if found:
            section = found.group(1)
            releases[-1][2][section] = []
        elif line.startswith("- ") and section is not None:
            releases[-1][2][section].append(line[2:])
        elif line.startswith("  ") and section is not None and releases[-1][2][section]:
            releases[-1][2][section][-1] += " " + line.strip()
    return releases


def normal(text: str) -> str:
    return " ".join(text.split()).lower()


def duplicates(entries: list[str]) -> list[str]:
    """One message per bullet that repeats an earlier bullet or advisory."""
    problems = []
    seen_text: dict[str, int] = {}
    seen_advisory: dict[str, int] = {}
    for index, entry in enumerate(entries):
        key = normal(entry)
        if key in seen_text:
            problems.append(f"the same entry twice: {entry[:80]}")
        seen_text.setdefault(key, index)
        for advisory in {a.upper() for a in ADVISORY.findall(entry)}:
            if advisory in seen_advisory and seen_advisory[advisory] != index:
                problems.append(f"{advisory} is in two entries: {entry[:80]}")
            seen_advisory.setdefault(advisory, index)
    return problems


def pending(directory: Path):
    """Fragments as (name, type, text)."""
    found = []
    if not directory.is_dir():
        return found
    for path in sorted(directory.iterdir()):
        match = FRAGMENT.match(path.name)
        if match and path.name != "README.md":
            found.append((path.name, match.group("type"), path.read_text(encoding="utf-8")))
    return found


def check(version: str, changelog: str, fragments) -> list[str]:
    errors = []
    releases = parse_changelog(changelog)
    for name, number, sections in releases:
        if number > BASELINE and number[2] != 0:
            for kind in MINOR_ONLY:
                if kind in sections:
                    errors.append(
                        f"CHANGELOG.md {name} is a patch release with a {kind} section; "
                        "a feature or a removal needs a minor bump"
                    )
        every = [entry for entries in sections.values() for entry in entries]
        errors += [f"CHANGELOG.md {name}: {message}" for message in duplicates(every)]

    parts = version.split(".")
    patch = int(parts[2]) if len(parts) == 3 and parts[2].isdigit() else 0
    released = {name for name, _, _ in releases}
    if patch != 0 and version not in released:
        for name, kind, _ in fragments:
            if kind in FRAGMENT_MINOR_ONLY:
                errors.append(
                    f"changelog.d/{name} is a {kind} fragment and Cargo.toml says {version}, "
                    "a patch; a feature or a removal needs a minor bump"
                )
    errors += [f"changelog.d: {message}" for message in duplicates([text for _, _, text in fragments])]
    return errors


def workspace_version() -> str:
    with open(ROOT / "Cargo.toml", "rb") as f:
        return tomllib.load(f)["workspace"]["package"]["version"]


def selftest() -> int:
    top = "# Changelog\n\n<!-- towncrier release notes start -->\n\n"

    def release(name: str, body: str) -> str:
        return f"## {name} (2026-01-01)\n\n{body}\n"

    feat = release("1.2.3", "### Features\n\n- A thing.\n\n### Fixes\n\n- A fix.\n")
    clean = release("1.2.3", "### Fixes\n\n- A fix.\n")
    cases = [
        ("patch with features", "1.2.3", top + feat, [], 1),
                ("minor with features", "1.3.0", top + release("1.3.0", "### Features\n\n- A.\n"), [], 0),
        ("patch with removals", "1.2.3", top + release("1.2.3", "### Removals\n\n- A.\n"), [], 1),
        ("patch with fixes", "1.2.3", top + release("1.2.3", "### Fixes\n\n- A.\n"), [], 0),
        ("patch fragment feature", "1.2.4", top + clean, [("+a.feature.md", "feature", "x")], 1),
        ("patch fragment bugfix", "1.2.4", top + clean, [("+a.bugfix.md", "bugfix", "x")], 0),
        ("minor fragment feature", "1.3.0", top + clean, [("+a.feature.md", "feature", "x")], 0),
        ("queued after patch release", "1.2.3", top + clean, [("+a.feature.md", "feature", "x")], 0),
        ("history before the baseline", "0.45.4", top + release("0.45.4", "### Features\n\n- A.\n")
         + release("0.41.3", "### Features\n\n- B.\n"), [], 0),
        ("same bullet twice", "1.3.0",
         top + release("1.3.0", "### Fixes\n\n- Same.\n\n### Miscellaneous\n\n- same.\n"), [], 1),
        ("advisory twice", "1.3.0",
         top + release("1.3.0", "### Fixes\n\n- Takes url 2.5.9 for RUSTSEC-2026-0592.\n\n"
                       "### Miscellaneous\n\n- The lock fixes RUSTSEC-2026-0592.\n"), [], 1),
        ("advisory twice in fragments", "1.3.0", top,
         [("+a.bugfix.md", "bugfix", "Fixes RUSTSEC-2026-0592."),
          ("+b.misc.md", "misc", "Also RUSTSEC-2026-0592.")], 1),
        ("two advisories", "1.3.0",
         top + release("1.3.0", "### Fixes\n\n- RUSTSEC-2026-0592.\n- RUSTSEC-2026-0593.\n"), [], 0),
    ]
    bad = 0
    for label, version, text, fragments, want in cases:
        got = len(check(version, text, fragments))
        mark = "ok" if (got > 0) == (want > 0) else "WRONG"
        bad += mark == "WRONG"
        print(f"selftest {mark}: {label}: {got} problem(s)")
    return 1 if bad else 0


def main() -> int:
    if sys.argv[1:] == ["--selftest"]:
        return selftest()
    if sys.argv[1:]:
        print(__doc__)
        return 2
    version = workspace_version()
    errors = check(
        version,
        (ROOT / "CHANGELOG.md").read_text(encoding="utf-8"),
        pending(ROOT / "changelog.d"),
    )
    for error in errors:
        print(f"changelog-check: {error}", file=sys.stderr)
    if errors:
        return 1
    print(f"changelog-check: ok at {version}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
