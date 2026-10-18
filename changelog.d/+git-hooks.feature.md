`ljos git-hooks install` writes post-commit and post-rewrite hooks that book the commit through `ljos hook --git-commit`.
The hooks chain to each repository's own hooks, skip the tracker and a claimdag or deedar store, skip the middle of a rebase or merge, and skip a Claude Code, Codex, or opencode session.
An amend and a rebase are booked once, a hook failure never fails the commit, and `LJOS_OFF` turns the hooks off.
`ljos onboard --harness grokbot` installs them.
The MCP server tells the client to pass `seat` and that commits are booked automatically.
