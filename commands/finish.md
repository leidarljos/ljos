---
name: finish
description: Close a sitting and record its lesson
# proseguard:off rgoswami.EmDashInText
argument-hint: <issue-id> --lesson <text>
# proseguard:on rgoswami.EmDashInText
allowed-tools: Bash(ljos:*)
---

Close the sitting named in the arguments.

Arguments: $ARGUMENTS

Run `ljos finish` with those arguments. A lesson is required. It releases the claim and leaves the ticket open. A person closes it when the work is accepted.
