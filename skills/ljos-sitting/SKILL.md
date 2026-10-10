---
name: ljos-sitting
description: Open and close a piece of work on one ticket with ljos sitting and ljos finish. Use when starting work on an issue, when the hook says the conversation has no issue, and when the work is done or handed back.
---

# A sitting: one ticket, start to finish

A sitting ties the work in this conversation to one ticket, so the next
agent, in Cursor or any other tool, can see who has it and what it taught.

## Start

1. Find or make the ticket. `vissue q -p PROJECT "TITLE"` makes one and prints
   its id (see the ljos-file skill).
2. `ljos sitting ISSUE`. It prints, in order: doctor, cards (files the person
   froze, read only), what is due for review, the memories the title brings
   up, the recipe, the working set, the timeline, and the claim. It stops at
   the first store that does not answer. Fix that before going on.
3. Read what it printed. The memories and the cards are the constraints for
   this work.

## During

- Record progress on the ticket: `ljos note ISSUE "what is done, what is left"`.
- Save a lesson as soon as you learn it: `ljos remember "..."`.
- File new work you find instead of doing it on the side (ljos-file skill).
- A choice with more than one defensible answer goes to a vote (ljos-decide skill).

## Finish

```
ljos finish ISSUE --lesson "One or two sentences the next agent can act on."
```

The lesson is filed as a proposal; `ljos accept ID` from the person writes it.
Finishing does not close the ticket. Add `--close` only when the person has
accepted the work. To stop without finishing, `ljos release ISSUE`.

`/sitting ISSUE` and `/finish ISSUE --lesson "..."` run the same verbs.
