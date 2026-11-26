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
# A page named PARENT-TASK belongs to PARENT, which lists it in a toctree of its
# own, so the index toctree leaves it out.
_ORG = Path(__file__).resolve().parents[1] / "orgmode"
_ORDER = ["getting-started", "howto", "reference", "explanation", "architecture"]
PARENTS = ("howto", "reference")


def _parent(stem: str) -> str | None:
    return next((p for p in PARENTS if stem.startswith(p + "-")), None)


PAGES = [p for p in _ORDER if (_ORG / f"{p}.org").is_file()] + sorted(
    q.stem
    for q in _ORG.glob("*.org")
    if q.stem not in _ORDER and q.stem != "index" and _parent(q.stem) is None
)

TOCTREE = """

.. toctree::
   :maxdepth: 1
   :caption: Guides
   :hidden:

""" + "".join(f"   {page}\n" for page in PAGES)

_FOOTNOTE_DEF = re.compile(r"^\[\d+\]")
_COVER = "https://leidarljos.github.io/ljos/_static/og.png"
_ORG_LINK = re.compile(r"\[\[(?:[^\]]*)\]\[([^\]]+)\]\]")
_ORG_TARGET = re.compile(r"\[\[([^\]]+)\]\]")
_ORG_MARK = re.compile(r"(?<!\w)([=~/])([^\n]+?)\1")


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


# A description stays near 220 characters. A sentence that names the ljos
# version is the point of the line, so it stays even when that makes the
# description longer. Getting started is the page that hits this.
_LJOS_VERSION = re.compile(r"\bljos[ \t]+(?:\{version\}|v?\d+\.\d+\.\d+)\b")
_LJOS_NAMED_VERSION = re.compile(r"\bljos[ \t]+v?(\d+\.\d+\.\d+)\b")


def _names_ljos_version(text: str) -> bool:
    return _LJOS_VERSION.search(text) is not None


def _sentences(prose: str, limit: int = 220) -> str:
    parts = re.split(r"(?<=[.!?])\s+", prose.strip())
    kept: list[str] = []
    version_tail: list[str] = []
    for part in parts:
        part = part.strip().rstrip(":")
        if not part:
            continue
        trial = " ".join([*kept, *version_tail, part])
        if len(trial) <= limit:
            kept.extend(version_tail)
            version_tail.clear()
            kept.append(part)
            continue
        if _names_ljos_version(part):
            version_tail.append(part)
            continue
        if kept or version_tail:
            break
        cut = part[:limit].rsplit(" ", 1)[0].rstrip(",;:")
        return cut or part[:limit].rstrip()
    return " ".join([*kept, *version_tail])


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


_ORG_VERSION_COG = re.compile(
    r'# \[\[\[cog import ljos_version as v; v\.lines\("((?:[^"\\]|\\.)*)"\)\]\]\]'
)
_RST_DESC_ARG = re.compile(r'(:description: )(.*?)(", ":cover: )')


def _crate_version() -> str:
    root = Path(__file__).resolve().parents[2]
    scripts = str(root / "scripts")
    if scripts not in sys.path:
        sys.path.insert(0, scripts)
    import ljos_version

    return ljos_version.version()


def _org_inline_to_rst(text: str) -> str:
    text = re.sub(r"=([^=\n]+)=", r"``\1``", text)
    text = re.sub(r"~([^~\n]+)~", r"``\1``", text)
    return text


def _opening_version_template(org: str) -> str | None:
    """The version cog before the first heading, as the rst template."""
    head = org.split("\n* ", 1)[0]
    found = _ORG_VERSION_COG.search(head)
    if found is None or "{version}" not in found.group(1):
        return None
    return _org_inline_to_rst(found.group(1))


def _refresh_leading_cog(body: str, org: str) -> str | None:
    """Rewrite a leading version cog's description from the org template.

    The description argument keeps ``{version}``. A rendered number in that
    argument stays at whatever release wrote it, and cog will not move it.
    Returns None when the page has no such cog.
    """
    if not body.startswith(".. [[[cog import ljos_version as v; v.lines("):
        return None
    template = _opening_version_template(org)
    if template is None or '"' in template:
        return None
    first, sep, rest = body.partition("\n")
    if ":description:" not in first or '", ":cover:' not in first:
        return None
    new_first, count = _RST_DESC_ARG.subn(
        lambda match: match.group(1) + template + match.group(3),
        first,
        count=1,
    )
    if count != 1:
        return body
    rendered = template.format(version=_crate_version())
    lines = (new_first + sep + rest).splitlines(keepends=True)
    replaced = False
    out: list[str] = []
    for line in lines:
        if not replaced and line.startswith(":description:"):
            out.append(f":description: {rendered}\n")
            replaced = True
            continue
        out.append(line)
    return "".join(out)


def promote_page(rst: Path, org: Path) -> None:
    """The page description, as docinfo. Headings stay as the org file wrote them."""
    source = org.read_text(encoding="utf-8")
    title = _org_title(source)
    body = rst.read_text(encoding="utf-8")
    refreshed = _refresh_leading_cog(body, source)
    if refreshed is not None:
        if refreshed != body:
            rst.write_text(refreshed, encoding="utf-8")
            print(f"{rst.name}: description refreshed from org")
        else:
            print(f"{rst.name}: description already written")
        return
    if body.startswith(":description:") or body.startswith(".. meta::\n"):
        print(f"{rst.name}: description already written")
        return
    description = _description(source, title)
    header = f":description: {description}\n:cover: {_COVER}\n\n"
    rst.write_text(header + body.lstrip("\n"), encoding="utf-8")
    print(f"{rst.name}: description written")


def mismatched_ljos_versions(root: Path) -> list[str]:
    """Rst and org lines that name an ljos version other than the crate."""
    want = _crate_version()
    found: list[str] = []
    for folder in ("docs/orgmode", "docs/source"):
        base = root / folder
        if not base.is_dir():
            continue
        for path in sorted(base.rglob("*")):
            if path.suffix not in {".org", ".rst"}:
                continue
            text = path.read_text(encoding="utf-8")
            for number, line in enumerate(text.splitlines(), 1):
                for match in _LJOS_NAMED_VERSION.finditer(line):
                    have = match.group(1)
                    if have != want:
                        rel = path.relative_to(root)
                        found.append(f"{rel}:{number}: ljos {have}, crate is {want}")
    return found


def check_versions(root: Path) -> int:
    bad = mismatched_ljos_versions(root)
    if bad:
        print(
            "check_docs_export: a doc names an ljos version other than the crate",
            file=sys.stderr,
        )
        for line in bad:
            print(line, file=sys.stderr)
        return 1
    print(f"check_docs_export: doc versions match {_crate_version()}")
    return 0


def restore_toctree(index: Path) -> None:
    text = index.read_text(encoding="utf-8")
    if ".. toctree::" in text:
        print("index.rst: toctree present")
        return
    index.write_text(text.rstrip("\n") + TOCTREE, encoding="utf-8")
    print("index.rst: toctree restored")


def unlisted_children(source: Path) -> list[str]:
    """Child pages their parent's toctree does not list. Sphinx would orphan them."""
    out = []
    for org in sorted(_ORG.glob("*.org")):
        parent = _parent(org.stem)
        if parent is None:
            continue
        rst = source / f"{parent}.rst"
        text = rst.read_text(encoding="utf-8") if rst.is_file() else ""
        if ".. toctree::" not in text or f"\n   {org.stem}\n" not in text:
            out.append(f"{org.stem} (parent {parent})")
    return out


def bracket_leading_lines(path: Path) -> list[tuple[int, str]]:
    return [
        (number, line)
        for number, line in enumerate(path.read_text(encoding="utf-8").split("\n"), 1)
        if _FOOTNOTE_DEF.match(line)
    ]


def main() -> int:
    if len(sys.argv) > 1 and sys.argv[1] == "--check-versions":
        root = Path(sys.argv[2]) if len(sys.argv) > 2 else Path(".")
        return check_versions(root)
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

    unlisted = unlisted_children(source)
    if unlisted:
        print(f"pages missing from their parent's toctree: {', '.join(unlisted)}", file=sys.stderr)
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
