The PreToolUse hook can now fix a command instead of only refusing it.
When ljos-policyd answers `allow<TAB>rewrite<TAB>LINE`, the guard judges
LINE again. Claude Code, Codex, Grok Build and Cursor's `preToolUse` run
an accepted LINE as an updated tool input. A LINE the guard refuses or
rewrites again refuses the call, and the original line does not run.
A uv refusal of `python x.py` stays a refusal, and it now names
`uv run x.py`.
