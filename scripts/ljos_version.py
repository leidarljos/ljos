#!/usr/bin/env python3
"""The one ljos version, and every copy of it.

The version is written by hand in one place: [workspace.package] version in
the workspace Cargo.toml. cog writes the copies in the files cogfiles.txt lists,
between its markers, from version() below. The JSON manifests cannot hold a
marker, so the block at the end of this file checks them.

    cog -I scripts --check @cogfiles.txt     fails on any copy that differs
    python3 scripts/ljos_version.py       rewrites every copy

Run cog from the checkout root.
"""

from __future__ import annotations

import json
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
COGFILES = ROOT / "cogfiles.txt"

# A str step into a list picks the entry of that name.
JSON_COPIES = [
    (".claude-plugin/plugin.json", ("version",)),
    (".claude-plugin/marketplace.json", ("plugins", "ljos", "version")),
    (".cursor-plugin/plugin.json", ("version",)),
    ("server.json", ("version",)),
    ("server.json", ("packages", 0, "version")),
]

_VERSION_VALUE = re.compile(r'("version"\s*:\s*")([^"]*)(")')


def version() -> str:
    with open(ROOT / "Cargo.toml", "rb") as f:
        return tomllib.load(f)["workspace"]["package"]["version"]


def lines(*templates: str) -> None:
    """Each template on its own line, with {version} filled in."""
    import cog

    for template in templates:
        cog.outl(template.format(version=version()))


def _step(node, key):
    if isinstance(node, list) and isinstance(key, str):
        return next(entry for entry in node if entry.get("name") == key)
    return node[key]


def _at(doc, path):
    for key in path:
        doc = _step(doc, key)
    return doc


def _set(doc, path, value):
    parent = _at(doc, path[:-1])
    parent[path[-1]] = value


def json_drift() -> list[str]:
    want = version()
    drift = []
    for name, path in JSON_COPIES:
        doc = json.loads((ROOT / name).read_text(encoding="utf-8"))
        have = _at(doc, path)
        if have != want:
            drift.append(f"{name} {'.'.join(map(str, path))} is {have}, Cargo.toml is {want}")
    return drift


def write_json() -> None:
    """Set each JSON copy to the version, keeping the file's layout."""
    want = version()
    for name in dict.fromkeys(name for name, _ in JSON_COPIES):
        file = ROOT / name
        text = file.read_text(encoding="utf-8")
        expected = json.loads(text)
        for copy, path in JSON_COPIES:
            if copy == name:
                _set(expected, path, want)
        written = _VERSION_VALUE.sub(lambda m: m.group(1) + want + m.group(3), text)
        if json.loads(written) != expected:
            raise SystemExit(f"{name}: a version outside the ljos copies would change; set it by hand")
        if written != text:
            file.write_text(written, encoding="utf-8")
            print(f"{name}: {want}")


def cogfiles() -> list[str]:
    return [line.strip() for line in COGFILES.read_text(encoding="utf-8").splitlines() if line.strip()]


def check_copies() -> None:
    """Fail the cog run on a JSON copy that differs, or a listed file without markers."""
    import cog

    marker = "[" * 3 + "cog"
    for name in cogfiles():
        if marker not in (ROOT / name).read_text(encoding="utf-8"):
            cog.error(f"{name} is in cogfiles.txt and has no cog marker; an export may have dropped it")
    drift = json_drift()
    if drift:
        cog.error("; ".join(drift) + "; run python3 scripts/ljos_version.py")


def main() -> int:
    from cogapp import Cog

    sys.dont_write_bytecode = True
    write_json()
    return Cog().main(["cog", "-I", str(ROOT / "scripts"), "-r", "--verbosity=1", f"&{COGFILES}"])


if __name__ == "__main__":
    sys.exit(main())

# [[[cog import ljos_version; ljos_version.check_copies()]]]
# [[[end]]]
