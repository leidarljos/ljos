#!/usr/bin/env python3
"""Rewrite org/rst file hyperlinks in exported RST to Sphinx :doc: roles."""

from __future__ import annotations

import re
import sys
from pathlib import Path

# Pandoc anonymous links on one line: `label <foo.org>`__ and the single `_` form.
# The label and the target stay on that line. A heredoc `<<'TOML'` is not a
# link, and a match must not run from a code span into a later `.org`.
_LINK = re.compile(
    r"(?<!<)`([^`<\n]+)\s+<((?![a-z][a-z0-9+.-]*:)[^>\s]+?\.(?:rst|org))>`__?"
)


def pad_grid_table_rows(t: str) -> str:
    """Keep grid-table row widths aligned after in-cell rewrites.

    Pandoc emits fixed-width grid tables. If we rewrite a link within a table
    row and make it shorter, the row no longer reaches the border width and
    docutils rejects the table as malformed.
    """

    lines = t.splitlines()
    i = 0
    while i < len(lines):
        line = lines[i]
        if not (line.startswith("+") and line.endswith("+") and ("-" in line or "=" in line)):
            i += 1
            continue

        width = len(line)
        j = i + 1
        while j < len(lines) and lines[j].strip():
            row = lines[j]
            if row.startswith("|") and row.endswith("|") and len(row) < width:
                # Extend the final cell padding while keeping the closing '|'.
                lines[j] = row[:-1].ljust(width - 1) + "|"
            j += 1
        i = j

    return "\n".join(lines) + ("\n" if t.endswith("\n") else "")


def fix_text(t: str) -> str:
    def repl(m: re.Match[str]) -> str:
        label = m.group(1).strip().replace("\n", " ")
        target = m.group(2)
        name = Path(target).name
        if name.endswith(".org"):
            name = name[: -len(".org")]
        elif name.endswith(".rst"):
            name = name[: -len(".rst")]
        return f":doc:`{label} <{name}>`"

    t = _LINK.sub(repl, t)
    t = pad_grid_table_rows(t)
    # Pandoc glues the next section title onto the end of a raw:: html block,
    # which Sphinx reads as an unexpected unindent. Re-separate them wherever
    # a directive body ends and a section title begins.
    t = re.sub(r"\n(   </div>)\n(?=\S)", r"\n\1\n\n", t)
    return t


def main() -> int:
    src = Path("docs/source")
    if not src.is_dir():
        print("docs/source missing", file=sys.stderr)
        return 1
    n = 0
    for path in sorted(src.glob("*.rst")):
        raw = path.read_text(encoding="utf-8")
        fixed = fix_text(raw)
        if fixed != raw:
            path.write_text(fixed, encoding="utf-8")
            n += 1
            print(f"fixed links in {path.name}")
    print(f"fix_doc_links: updated {n} file(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
