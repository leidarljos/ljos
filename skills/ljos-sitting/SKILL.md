---
name: ljos-sitting
description: Open and close a piece of work on one ticket with ljos sitting and ljos finish. Use when starting work on an issue, when the hook says the conversation has no issue, and when the work is done or handed back.
---

# A sitting: one ticket, start to finish

A sitting ties the work in this conversation to one ticket, so the next
agent, in the editor harness or any other tool, can see who has it and what it taught.

## Start

1. Open the day's work with `ljos sitting --plan "TITLE" -p PROJECT -- "item 1" "item 2"`.
   That files the parent and the items in one tracker commit, claims the parent,
   and prints the checklist with ids. The next line is the unfinished item.
   `ljos ready --mine` names it again. `ljos finish ITEM` ticks that item off.
   One ticket, with no checklist, is still `ljos file "TITLE" --top -p PROJECT`
   and then `ljos sitting ISSUE`.
2. `ljos sitting ISSUE` prints whether ljos is ready, anything that
   needs action, the memories the title brings up, and the claim. It stops at
   the first store that does not answer. Fix that before going on.
   `ljos sitting ISSUE --verbose` prints every section in full.
3. Read what it printed. The memories and the cards are the constraints for
   this work.

## During

- Record progress on the ticket: `ljos note ISSUE "what is done, what is left"`.
- Save a lesson of your own as soon as you learn it: `ljos note ISSUE "..."`.
  The prompt does not treat that note as a fact. The person's own words
  are `ljos remember "..."`.
- File new work you find instead of doing it on the side (ljos-file skill).
- A choice with more than one defensible answer goes to a vote (ljos-decide skill).

## Finish

```
ljos finish ISSUE --lesson "One or two sentences the next agent can act on."
```

An agent's lesson is stored at once at L0 (`derived`) with origin `agent-derived`, and search finds it right away. `ljos accept` raises it to `explicit`.
A lesson the person types at `ljos finish` in their own terminal is written at once as `user-declared`.
Finishing does not close the ticket. A person closes it when the work is
accepted. To stop without finishing, `ljos release ISSUE`.

`/sitting ISSUE` and `/finish ISSUE --lesson "..."` run the same verbs.
