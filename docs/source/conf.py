"""Sphinx configuration for the ljos project site (Shibuya theme)."""

from __future__ import annotations

import html
import re
from pathlib import Path

_DOCS = Path(__file__).resolve().parent
_ROOT = _DOCS.parent.parent

project = "ljos"
copyright = "2026, Rohit Goswami"
author = "Rohit Goswami"
release = "0.1.0"
version = "0.1"

extensions = [
    "sphinx.ext.mathjax",
    "sphinx_copybutton",
    "sphinx_design",
]

templates_path = ["_templates"]
exclude_patterns: list[str] = []

html_theme = "shibuya"
html_static_path = ["_static"]
html_favicon = "_static/favicon.svg"
html_logo = "_static/mark.svg"
html_title = "ljos"
html_baseurl = "https://leidarljos.github.io/ljos/"
html_copy_source = True
html_css_files = ["custom.css"]

# Preview card. .github/render-og-png.sh paints docs/source/_static/og.svg.
_OG_IMAGE = "https://leidarljos.github.io/ljos/_static/og.png"
_PAGE_DESCRIPTION = {
    "search": "Search the ljos documentation.",
    "genindex": "Index of the ljos documentation.",
}

html_context = {
    "source_type": "github",
    "source_user": "leidarljos",
    "source_repo": "ljos",
    "source_version": "main",
    "source_docs_path": "/docs/source/",
}

html_theme_options = {
    "accent_color": "gold",
    "color_mode": "auto",
    "dark_code": True,
    "open_in_chatgpt": False,
    "open_in_claude": False,
    "github_url": "https://github.com/leidarljos/ljos",
    "og_image_url": _OG_IMAGE,
    "nav_links": [
        {"title": "Get started", "url": "getting-started"},
        {"title": "How-to", "url": "howto"},
        {"title": "Reference", "url": "reference"},
        {"title": "Explanation", "url": "explanation"},
        {"title": "Search", "url": "search"},
    ],
}

# Offline builds must not reach for an inventory.
intersphinx_mapping: dict = {}

copybutton_prompt_text = r"\$ "
copybutton_prompt_is_regexp = True


# Org page titles. The clipped h1 uses these. The visible headings stay the
# sections the org file already had, so the sidebar and the breadcrumb do not move.
_PAGE_TITLE = {
    "index": "ljos",
    "getting-started": "Getting started",
    "howto": "How-to",
    "grok-build": "Grok Build",
    "integration": "Integration",
    "architecture": "Architecture",
    "reference": "Reference",
    "explanation": "Explanation",
    "seat": "In the seat",
}
_OPEN_HEADING = re.compile(r"<h([1-6])((?:\s[^>]*)?)>", re.IGNORECASE)
_CLOSE_HEADING = re.compile(r"</h([1-6])>", re.IGNORECASE)


def _one_heading(body: str, title: str) -> str:
    """One h1 for the page, with the old sections shifted down a level."""
    if len(_OPEN_HEADING.findall(body)) <= 1:
        return body

    def open_tag(match: re.Match[str]) -> str:
        level = int(match.group(1))
        attrs = match.group(2) or ""
        marker = f"ll-h{level}"
        if re.search(r"\bclass=", attrs):
            attrs = re.sub(r'\bclass="', f'class="{marker} ', attrs, count=1)
        else:
            attrs += f' class="{marker}"'
        return f"<h{level + 1}{attrs}>"

    shifted = _OPEN_HEADING.sub(open_tag, body)
    shifted = _CLOSE_HEADING.sub(lambda match: f"</h{int(match.group(1)) + 1}>", shifted)
    return f'<h1 class="ll-page-title">{html.escape(title)}</h1>\n' + shifted


def _social(app, pagename, templatename, context, doctree):
    """Description and preview image on generated pages, and the index title."""
    meta = context.get("meta")
    meta = dict(meta) if isinstance(meta, dict) else {}
    if pagename == "index":
        context["title"] = "ljos"
    if not str(meta.get("description") or "").strip():
        meta["description"] = _PAGE_DESCRIPTION.get(
            pagename,
            "Documentation for ljos, a seat over memory, a tracker, and a command guard.",
        )
    if not str(meta.get("cover") or "").strip():
        meta["cover"] = _OG_IMAGE
    context["meta"] = meta
    body = context.get("body")
    title = _PAGE_TITLE.get(pagename)
    if isinstance(body, str) and title:
        context["body"] = _one_heading(body, title)


def setup(app):
    app.connect("html-page-context", _social)
