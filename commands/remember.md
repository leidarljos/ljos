---
name: remember
description: Save a preference, a lesson or a never-run rule in ljos
argument-hint: <what to remember>
allowed-tools: Bash(ljos:*)
---

Save what the arguments say in ljos.

Arguments: $ARGUMENTS

Decide which verb fits, as the ljos-remember skill says: `ljos prefer` for a
standing choice, `ljos remember` for a lesson, `ljos rule PATTERN --verdict deny --why ...`
for a command that must never run. Keep the wording to one or two
sentences. Show the line you will run, run it, and print what it returned.
