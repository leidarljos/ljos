---
name: doctor
description: Check that the ljos programs are installed and answering
allowed-tools: Bash(ljos:*)
---

Run `ljos doctor` and read it back in plain words: its first line says
whether ljos is ready, and the rows under it are the ones that need action,
each with its fix. `ljos doctor --all` lists every row. `ljos doctor --fix`
refreshes a runner skill and a runner env file ljos wrote. A skill edited
by hand is left in place, and so is an env file ljos did not write; lines
the user added to an env file are kept. A stale skill or env ljos wrote
is a warning. If `ljos` is not installed, follow
the ljos-setup skill.
