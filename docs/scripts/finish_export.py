#!/usr/bin/env python3
"""Finish the org to RST export: restore the toctree, refuse a corrupting shape.

Two failures here are quiet rather than loud, which is why they are checked
instead of trusted:

- Pandoc drops a trailing ``rst`` export block often enough that a build comes
  out with every guide orphaned and no way to reach it from the index.
- A line that begins with ``[12]`` is a footnote *definition* in RST. When the
  citation markers in this project's prose land at column zero, docutils eats
  the surrounding sentence into a footnote nobody references. The build still
  succeeds; the page loses text.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# The pages a site has are the org files it has, index aside, in a fixed order.
_ORG = Path(__file__).resolve().parents[1] / "orgmode"
_ORDER = ["getting-started", "howto", "reference", "explanation", "architecture"]
PAGES = [p for p in _ORDER if (_ORG / f"{p}.org").is_file()] + sorted(
    q.stem for q in _ORG.glob("*.org") if q.stem not in _ORDER and q.stem != "index"
)

TOCTREE = """

.. toctree::
   :maxdepth: 1
   :caption: Guides
   :hidden:

""" + "".join(f"   {page}\n" for page in PAGES)

_FOOTNOTE_DEF = re.compile(r"^\[\d+\]")
_COVER = "https://leidarljos.github.io/ljos/_static/og.png"
_DEMOTE = {"=": "-", "-": "~", "~": "^", "^": "'", "'": '"'}
_ORG_LINK = re.compile(r"\[\[(?:[^\]]*)\]\[([^\]]+)\]\]")
_ORG_TARGET = re.compile(r"\[\[([^\]]+)\]\]")
_ORG_MARK = re.compile(r"(?<!\w)([=~/])([^\n]+?)\1")


def _adornment(line: str) -> str | None:
    if not line or line[0].isspace():
        return None
    text = line.rstrip("\n")
    if not text or len(set(text)) != 1:
        return None
    char = text[0]
    if char not in _DEMOTE:
        return None
    return char


def _org_title(text: str) -> str:
    for line in text.splitlines():
        if line.lower().startswith("#+title:"):
            title = line.split(":", 1)[1].strip()
            if title:
                return title
    raise SystemExit("an org page has no #+title")


def _plain(text: str) -> str:
    text = _ORG_LINK.sub(r"\1", text)
    text = _ORG_TARGET.sub(r"\1", text)
    text = _ORG_MARK.sub(r"\2", text)
    return " ".join(text.split())


def _explicit_description(text: str) -> str | None:
    for line in text.splitlines():
        if line.lower().startswith("#+description:"):
            written = line.split(":", 1)[1].strip()
            if written:
                return written
    return None


def _skip_prose_line(line: str) -> bool:
    """A figure or a bare link is not the page's description."""
    if re.fullmatch(r"\[\[file:[^\]]+\]\]", line):
        return True
    if re.fullmatch(r"\[\[(?:https?|file):[^\]]+\](?:\[[^\]]*\])?\]", line):
        return True
    return False


def _sentences(prose: str, limit: int = 220) -> str:
    parts = re.split(r"(?<=[.!?])\s+", prose.strip())
    kept: list[str] = []
    for part in parts:
        part = part.strip().rstrip(":")
        if not part:
            continue
        trial = " ".join([*kept, part])
        if len(trial) <= limit:
            kept.append(part)
            continue
        if kept:
            break
        cut = part[:limit].rsplit(" ", 1)[0].rstrip(",;:")
        return cut or part[:limit].rstrip()
    return " ".join(kept)


def _description(text: str, title: str) -> str:
    """The page's opening prose, markup stripped, for the meta description."""
    written = _explicit_description(text)
    if written:
        return _sentences(written)
    end: str | None = None
    paragraph: list[str] = []
    for raw in text.splitlines():
        line = raw.strip()
        lower = line.lower()
        if end:
            if lower == end:
                end = None
            continue
        if lower.startswith("#+begin_src"):
            end = "#+end_src"
            continue
        if lower.startswith("#+begin_export"):
            end = "#+end_export"
            continue
        if lower.startswith("#+begin_example"):
            end = "#+end_example"
            continue
        if lower.startswith("#+") or line.startswith("# "):
            continue
        if _skip_prose_line(line):
            continue
        if line.startswith("*") or line.startswith("|") or line.startswith(":"):
            if paragraph:
                break
            continue
        if not line:
            if paragraph:
                break
            continue
        paragraph.append(line)
    prose = _plain(" ".join(paragraph))
    if not prose:
        prose = title
    return _sentences(prose)


def _is_section_rule(line: str, previous: str) -> bool:
    char = _adornment(line)
    title = previous.rstrip("\n")
    return bool(
        char
        and title
        and not title[0].isspace()
        and not title.startswith(("..", "+", "|", ":"))
        and len(line.rstrip("\n")) >= len(title.rstrip())
    )


def _first_section(text: str) -> str | None:
    previous = ""
    for line in text.splitlines():
        if _is_section_rule(line, previous):
            return previous.strip()
        if line.strip():
            previous = line
    return None


def _demote(text: str, keep_first: bool = False) -> str:
    lines = text.splitlines(keepends=True)
    out: list[str] = []
    previous = ""
    kept = False
    for line in lines:
        char = _adornment(line)
        if _is_section_rule(line, previous) and char:
            if keep_first and not kept:
                kept = True
                out.append(line)
            else:
                ruled = _DEMOTE[char] * len(line.rstrip("\n"))
                if line.endswith("\n"):
                    ruled += "\n"
                out.append(ruled)
        else:
            out.append(line)
        if line.strip():
            previous = line
    return "".join(out)


def promote_page(rst: Path, org: Path) -> None:
    """One document title, sections under it, and the page's description."""
    source = org.read_text(encoding="utf-8")
    title = _org_title(source)
    body = rst.read_text(encoding="utf-8")
    if body.startswith(":description:") or body.startswith(".. meta::\n"):
        print(f"{rst.name}: title already promoted")
        return
    description = _description(source, title)
    # A page whose first section is already its title keeps that one heading.
    # The field list is docinfo, which the theme reads as the page description.
    keep_first = _first_section(body) == title
    promoted = _demote(body, keep_first=keep_first).lstrip("\n")
    header = f":description: {description}\n:cover: {_COVER}\n\n"
    if not keep_first:
        header += f"{title}\n{'=' * len(title)}\n\n"
    rst.write_text(header + promoted, encoding="utf-8")
    print(f"{rst.name}: title {title!r}")


def restore_toctree(index: Path) -> None:
    text = index.read_text(encoding="utf-8")
    if ".. toctree::" in text:
        print("index.rst: toctree present")
        return
    index.write_text(text.rstrip("\n") + TOCTREE, encoding="utf-8")
    print("index.rst: toctree restored")


def bracket_leading_lines(path: Path) -> list[tuple[int, str]]:
    return [
        (number, line)
        for number, line in enumerate(path.read_text(encoding="utf-8").split("\n"), 1)
        if _FOOTNOTE_DEF.match(line)
    ]


def main() -> int:
    source = Path("docs/source")
    index = source / "index.rst"
    if not index.is_file():
        print("missing docs/source/index.rst after export", file=sys.stderr)
        return 1
    restore_toctree(index)

    for org in sorted(Path("docs/orgmode").glob("*.org")):
        rst = source / f"{org.stem}.rst"
        if rst.is_file():
            promote_page(rst, org)

    missing = [page for page in PAGES if not (source / f"{page}.rst").is_file()]
    if missing:
        print(f"missing exported pages: {', '.join(missing)}", file=sys.stderr)
        return 1

    failed = False
    for org in sorted(Path("docs/orgmode").glob("*.org")):
        for number, line in bracket_leading_lines(org):
            print(
                f"{org}:{number}: a line starting with a citation marker becomes an "
                f"RST footnote and swallows the sentence; rewrap it\n    {line}",
                file=sys.stderr,
            )
            failed = True
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
