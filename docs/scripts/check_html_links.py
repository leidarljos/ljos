#!/usr/bin/env python3
"""Fail when a built HTML page links to a missing page or a missing anchor.

External links are left alone. A relative link, including a fragment on this
page or another, has to name a file in the build and, when it has a fragment,
an id on that file.

docs/source/_static/legacy-anchors.json is the old howto.html and
reference.html section ids from 0eb8cdc9. An empty value means the id is
still on that page. Any other value is the page and id the section moved to,
and the built page has to carry that forwarder.
"""

from __future__ import annotations

import json
import sys
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlparse

ROOT = Path(__file__).resolve().parents[2]
MAP_PATH = ROOT / "docs" / "source" / "_static" / "legacy-anchors.json"

# Section ids on howto.html and reference.html built from 0eb8cdc9.
OLD = {
    "howto": {
        "how-to",
        "wire-the-seat-into-an-agent-runner",
        "seat-a-shell-only-agent",
        "add-a-seat-and-let-it-close-tickets",
        "write-to-another-seat",
        "wake-a-seat-when-mail-reaches-it",
        "inject-memory-before-the-agent-acts",
        "read-what-the-command-check-warns-on",
        "let-policyd-fix-a-command",
        "block-on-warning-warns-with-a-label",
        "choose-which-branches-the-push-check-covers",
        "let-jev-judge-the-prompt",
        "open-persona-panes-in-another-tool",
        "wire-the-seat-into-the-editor-harness",
        "spawn-a-persona-as-a-subagent",
        "show-the-seat-in-the-status-bar",
        "put-the-judgments-to-a-local-model-or-a-runner",
        "give-an-agent-the-protocol-without-a-runner",
        "ask-what-a-task-involves",
        "bump-a-recipe-with-eb-stack-and-keep-what-the-campaign-taught",
        "keep-a-number",
        "ask-when",
        "start-a-sitting-and-end-one",
        "vote-with-personas",
        "memories-a-no-vote-and-a-child",
        "cast-a-reviewer-s-ballot-from-its-own-run",
        "cast-persona-ballots-through-jev",
        "hold-a-done-claim-beside-a-red-test",
        "move-the-trust-rows-without-an-outcome",
        "keep-the-cards",
        "sign-what-you-hand-over",
        "receive-from-the-same-sender-again",
        "close-what-a-later-lesson-replaced",
        "retire-a-claim-with-its-reason",
        "set-trust-by-hand",
        "settle-under-the-pack-s-trust",
        "check-the-seat",
    },
    "reference": {
        "reference",
        "command-line",
        "user-rules",
        "threat-model",
        "git-hooks",
        "mcp-tools",
        "environment",
        "a-temporary-home-or-working-directory",
        "what-a-rule-reads",
        "checks-ljos-keeps",
        "sync-files",
        "jev-settings",
        "the-learning-rules",
        "crates",
        "cite",
        "help-text",
    },
}


class _Page(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.ids: set[str] = set()
        self.hrefs: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        values = {key: value for key, value in attrs if value is not None}
        if "id" in values:
            self.ids.add(values["id"])
        if tag == "a" and "href" in values:
            self.hrefs.append(values["href"])


def _resolve(page: Path, build: Path, href: str) -> tuple[Path, str] | None:
    parsed = urlparse(href)
    if parsed.scheme or parsed.netloc:
        return None
    path = unquote(parsed.path)
    fragment = unquote(parsed.fragment)
    if not path:
        return page, fragment
    parts: list[str] = []
    for part in (page.parent / path).parts:
        if part == "..":
            if parts:
                parts.pop()
            continue
        if part == ".":
            continue
        parts.append(part)
    # The walk above is absolute because page.parent is absolute. Rebuild
    # from the build root so a link cannot escape the build.
    try:
        relative = Path(*parts).resolve().relative_to(build.resolve())
    except ValueError:
        return Path(*parts), fragment
    return build / relative, fragment


def _pages(build: Path) -> dict[Path, _Page]:
    found: dict[Path, _Page] = {}
    for path in sorted(build.rglob("*.html")):
        parser = _Page()
        parser.feed(path.read_text(encoding="utf-8", errors="replace"))
        found[path.resolve()] = parser
    return found


def main() -> int:
    build = Path(sys.argv[1] if len(sys.argv) > 1 else ROOT / "docs" / "build").resolve()
    if not build.is_dir():
        print(f"check_html_links: no build at {build}", file=sys.stderr)
        return 1
    pages = _pages(build)
    if not pages:
        print(f"check_html_links: {build} has no html", file=sys.stderr)
        return 1
    problems: list[str] = []
    for path, page in pages.items():
        for href in page.hrefs:
            resolved = _resolve(path, build, href)
            if resolved is None:
                continue
            target, fragment = resolved
            where = path.relative_to(build)
            if not target.is_file():
                problems.append(f"{where}: {href} -> missing {target}")
                continue
            if not fragment:
                continue
            parsed = pages.get(target.resolve())
            if parsed is None:
                continue
            if fragment not in parsed.ids:
                problems.append(f"{where}: {href} -> no id {fragment}")
    legacy = json.loads(MAP_PATH.read_text(encoding="utf-8"))
    if set(legacy) != set(OLD):
        problems.append(
            "legacy-anchors.json pages "
            f"{sorted(legacy)} != {sorted(OLD)}"
        )
    for name, want in OLD.items():
        got = legacy.get(name, {})
        if set(got) != want:
            missing = sorted(want - set(got))
            extra = sorted(set(got) - want)
            problems.append(
                f"legacy-anchors.json {name}: missing {missing} extra {extra}"
            )
        html_path = (build / f"{name}.html").resolve()
        parser = pages.get(html_path)
        text = html_path.read_text(encoding="utf-8") if html_path.is_file() else ""
        if parser is None:
            problems.append(f"legacy: {name}.html was not built")
            continue
        for anchor, target in got.items():
            if not target:
                if anchor not in parser.ids:
                    problems.append(f"{name}.html#{anchor} is gone and has no forward")
                continue
            if target not in text:
                problems.append(f"{name}.html does not forward #{anchor} to {target}")
            file_name, _, fragment = target.partition("#")
            dest = (build / file_name).resolve()
            dest_page = pages.get(dest)
            if dest_page is None:
                problems.append(f"#{anchor} -> missing {file_name}")
                continue
            if fragment and fragment not in dest_page.ids:
                problems.append(f"#{anchor} -> {target} has no such id")
    if problems:
        print(f"check_html_links: {len(problems)} broken", file=sys.stderr)
        for problem in problems:
            print(f"check_html_links: {problem}", file=sys.stderr)
        return 1
    print(f"check_html_links: {len(pages)} pages, internal links ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
