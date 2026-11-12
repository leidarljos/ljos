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
| a lesson the person stated | `ljos remember "..."` | at once, when the words are theirs. Under a runner the agent's own words are `ljos note` |
| "never run X", "ask me before X" | `ljos rule 'PATTERN' --verdict deny --why "..."` | the hook warns as the command runs |

Examples:

```
ljos prefer "Push a tag with git push origin TAG; --follow-tags skips lightweight tags."
ljos remember "The CI cache key ignores Cargo.lock. Bump it after a dependency change."
ljos rule '*--force*' --verdict deny --why "Never force push."
```

- Keep each memory to one or two short sentences. Write the fix, not the
  story: what to do, and why.
- Before answering from memory, search: `ljos search TOPIC`. `No hits.`
  means the store has nothing on it. `Nothing relevant was found.` means
  the only rows were another seat's lessons under the similarity floor:
  a hit that names the subject, or an encoder cosine of at least 0.66.
  A failure means the store is down; run `ljos doctor`. A busy pack still
  answers by words.
- When the person states a standing choice in a prompt ("from now on use
  bun", "never push to main"), the prompt hook saves it in their words and
  says `Saved the person's preference`. Do not write it again. A lesson the
  person dictates, written with `ljos remember` in their words, stands at once.
  Under a runner those words have to be in the prompt the hook kept. A
  runner with no prompt hook, `LJOS_CAPTURE=off`, a paraphrase, or a prompt
  older than an hour does not count as theirs. The match is four or more
  contiguous words of that prompt. It does not check that the person
  asserted the claim, and it is left that way because tightening it is
  more than a one-line change. The agent's own words are stored at once as
  a lesson labelled `agent-derived`, for the person to review.
- A correction from the person ("you should have", "I told you") goes in as
  `ljos prefer` with their words, after you confirm the wording with them.
- `--verdict deny` or `ask` is a label the warning carries. Neither stops
  the command.
- When the hook warns on a command, the command still runs. Tell the person
  the exact command and the warning, so a rule they wrote is not passed over.
- When the hook denies a command, the person turned on blocking in policyd.
  Do not run it another way. Tell them the exact command and the reason.

Through the ljos tool server the same verbs are the tools `ljos_prefer`, `ljos_remember`,
`ljos_rule` and `ljos_search`.
