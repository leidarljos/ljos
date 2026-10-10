---
description: Close a sitting and record its lesson
# proseguard:off rgoswami.EmDashInText
argument-hint: <issue-id> --lesson <text>
# proseguard:on rgoswami.EmDashInText
allowed-tools: Bash(ljos:*)
---

Close the sitting named in the arguments.

Arguments: $ARGUMENTS

Run `ljos finish` with those arguments. A lesson is required. Completing the claim does not close the ticket. Pass `--close` only when the work is accepted.
