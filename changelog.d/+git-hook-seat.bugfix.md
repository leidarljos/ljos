A git hook books a commit as the resolved seat: `LJOS_SEAT`, then `seats.toml` through `CURSOR_CONVERSATION_ID`, then `grokbot-` and the first eight characters of that id. The claim, the deed and the tracker record are that name, passed on the child command. The process tree is not a seat, and a commit with none of those names is left unbooked.

A project matches a checkout by a recorded remote only: a `#+REMOTE:` header, a `remote` field in the project's metadata, or the repository name in that remote. A URL in a note does not.

`ljos git-hooks install` writes a stub for every standard hook. The stub runs the repository's own hook, or the previous `core.hooksPath`, with the same arguments and stdin. `post-commit` and `post-rewrite` still book. `[git_hooks] skip` in `ljos.toml` adds paths that are not booked, and a reinstall keeps that file.

`pre-commit install` follows `core.hooksPath` and would replace a stub. Install it into the repository first: `git config --local core.hooksPath "$(git rev-parse --git-dir)/hooks"`, run `pre-commit install`, then `git config --local --unset core.hooksPath`.
