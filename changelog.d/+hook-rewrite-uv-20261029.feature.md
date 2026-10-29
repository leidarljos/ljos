The PreToolUse hook now fixes a command instead of only refusing it.
ljos-policyd may answer `allow<TAB>rewrite<TAB>LINE`, and a local table
turns a uv refusal of `python x.py` into `uv run x.py`. Claude Code,
Codex, Grok Build and Cursor's `preToolUse` run the new line as an
updated tool input. Cursor's `beforeShellExecution` and runners behind
the hook relay keep the old answer, and its message names the new line.
