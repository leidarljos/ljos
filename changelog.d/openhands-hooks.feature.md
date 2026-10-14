OpenHands now gets a guard that blocks: `ljos onboard --harness openhands` writes `PreToolUse` on `terminal` and `UserPromptSubmit` to `~/.openhands/hooks.json`, which OpenHands reads at user level too.
OpenHands hears the session-start line on a session's first prompt, since it drops a `SessionStart` hook's output.
