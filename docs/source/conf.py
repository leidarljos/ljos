"""Sphinx configuration for the ljos project site (Shibuya theme)."""

from __future__ import annotations

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

# Preview card. docs/scripts/og_image.py paints this from the repo logo.
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


def setup(app):
    app.connect("html-page-context", _social)
