A ballot cast with no conversation id is bound to its run: the GitHub Actions run, else the Cursor cloud agent (`CURSOR_AGENT_ID` or `CLOUD_AGENT_ID`).
Claude Code's `CLAUDE_CODE_SESSION_ID` and opencode's `OPENCODE_SESSION_ID` name the session even when other runner ids are set.
Two reviewers on one seat, each voting from its own run, both count, and one run still casts one ballot.
