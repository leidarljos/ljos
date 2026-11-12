---
name: sitting
description: Open a sitting on one issue
argument-hint: <issue-id>
allowed-tools: Bash(ljos:*)
---

Open a sitting on the issue named in the arguments, or a plan when the arguments are a title and items.

Arguments: $ARGUMENTS

A day's work is `ljos sitting --plan "TITLE" -- "item 1" "item 2"`. It files the parent and the items in one tracker commit, claims the parent, and prints the checklist. `ljos ready --mine` names the next unfinished item. `ljos finish ITEM` ticks it off.

Otherwise run `ljos sitting` with the issue id and follow what it prints: whether ljos is ready, what needs action, what the seat remembers for the task, and the claim. `--verbose` prints every section. Stop at the first store that does not answer. Do not invent an issue id. When no issue and no plan are named, ask for one.
