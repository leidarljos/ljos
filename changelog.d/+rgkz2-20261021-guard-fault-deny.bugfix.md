A policyd that hangs, crashes, or prints something that is not a
verdict now denies the tool call. Claude Code, Codex and Cursor call
`ljos hook` directly, so that fault used to allow the command, and
Cursor's answer was a bare allow with no note.

`LJOS_GUARD_FAIL_OPEN=1` keeps the old allow with a note. A missing
`ljos-policyd` still allows, and so does a deny that only says the
seat was not ready.
