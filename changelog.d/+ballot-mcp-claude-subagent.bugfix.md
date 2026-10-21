An MCP ballot from a Claude Code subagent binds the parent session plus the subagent's `agent_id`.
The plugin and `ljos onboard` add a second `PreToolUse` group, matcher `^mcp__.+__ljos_.+$`, whose hook notes which subagent made each seat MCP call, and the server reads it back by the call's `_meta` tool use id.
On those calls the hook says nothing else; the server still judges them.
