---
name: sitting
description: Open a sitting on one issue
argument-hint: <issue-id>
allowed-tools: Bash(ljos:*)
---

Open a sitting on the issue named in the arguments.

Arguments: $ARGUMENTS

Run `ljos sitting` with those arguments and follow what it prints: whether ljos is ready, what needs action, what the seat remembers for the task, and the claim. `--verbose` prints every section. Stop at the first store that does not answer. Do not invent an issue id. When no issue is named, ask for one.
