A Claude Code subagent's `ljos vote` binds the ballot to the parent session plus the subagent's `agent_id`, which the `PreToolUse` hook reads from the payload.
Two subagents of one session both count, the parent and a subagent count as two, and one subagent still casts one ballot.
