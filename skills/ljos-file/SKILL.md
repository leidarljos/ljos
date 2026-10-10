---
name: ljos-file
description: File a ticket on the vissue tracker and note progress on it. Use on found work that is not part of the current task, when the person asks to file or track something, or before starting work that has no ticket.
---

# File work on the tracker

The tracker is vissue: plain Org files in a git repository, read by the
person and by every agent on this machine. Each tracker verb takes a ticket id.

- A new tracker ticket: `vissue q -p PROJECT "One-line title"` prints the new id.
  Add `--parent ID` to put it under an existing ticket, and `-t bug` (or
  `task`, `decision`) to type it.
- Note progress: `ljos note ISSUE "..."` (dated, committed).
- A longer report: `vissue append ISSUE --file report.md`.
- A tag: `vissue update ISSUE -t TAG`.
- Who has claimed what: `ljos claims`.
- What is ready to work on: `vissue ready -p PROJECT`.

A ticket title says what is wrong or what is wanted, in one line a stranger can read:
"Login fails when the password has a colon", not "auth bug".

A ticket title is also the first thing the next agent reads, so write it
before the work. Every piece of work has a ticket before it has a claim. Do not fix found work on the side: file it, and say so.
