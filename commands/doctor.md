---
name: doctor
description: Check that the ljos programs are installed and answering
allowed-tools: Bash(ljos:*)
---

Run `ljos doctor` and read it back in plain words: its first line says
whether ljos is ready, and the rows under it are the ones that need action,
each with its fix. `ljos doctor --all` lists every row. `ljos doctor --fix`
refreshes a runner skill ljos wrote; a skill edited by hand is left in place
and that row is a warning. If `ljos` is not installed, follow
the ljos-setup skill.
