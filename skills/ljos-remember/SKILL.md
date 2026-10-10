---
name: ljos-remember
description: Save something the agent should know next time, or a command it must never run. Use when the person says remember, always, never, prefer, from now on, or corrects the agent, and when a piece of work taught a lesson worth keeping.
---

# Remember and rule

ljos keeps what it is told in a local store and hands the parts that bear on
a prompt back to the agent. Three verbs write to it. Pick by what was said.

| the person said | verb | when it comes back |
|---|---|---|
| "I prefer X over Y", "always do X" | `ljos prefer "..."` | from now on |
| a lesson from this work | `ljos remember "..."` | once a review promotes it; `--standing` makes it standing now |
| "never run X", "ask me before X" | `ljos rule 'PATTERN' --verdict deny --why "..."` | the hook stops the command before it runs |

Examples:

```
ljos prefer "Tag a release with git push origin TAG; --follow-tags leaves v-tags behind."
ljos remember "The CI cache key ignores Cargo.lock. Bump it after a dependency change."
ljos rule '*--force*' --verdict deny --why "Never force push."
```

- Keep each memory to one or two short sentences. Write the fix, not the
  story: what to do, and why.
- Before answering from memory, search: `ljos search TOPIC`. An empty answer
  means the store has nothing on it. A failure means the store is down; run
  `ljos doctor`.
- A correction from the person ("you should have", "I told you") goes in as
  `ljos prefer` with their words, after you confirm the wording with them.
- `--verdict ask` hands the command to the person instead of refusing it.
- When the hook refuses a command, do not retry it another way. Tell the
  person the exact command and why it was refused, and leave it to them.

Through the ljos tool server the same verbs are the tools `ljos_prefer`, `ljos_remember`,
`ljos_rule` and `ljos_search`.
