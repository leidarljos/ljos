---
name: doctor
description: Check that the ljos programs are installed and answering
allowed-tools: Bash(ljos:*)
---

Run `ljos doctor` and read it back in plain words: each row that needs
action appears as that check finishes, and the line that says whether
ljos is ready is printed when the checks are done. `ljos doctor --all`
lists every row, in that same order, as each check finishes. `ljos doctor --fix`
refreshes a runner skill and a runner env file ljos wrote, creates a
missing ljos home and config file, moves a stale lock or pid file aside,
starts packsetd with `packset ensure`, and records a runner session seat.
A skill edited by hand is left in place, and so is an env file ljos did
not write; lines the user added to an env file are kept. Git hooks,
runner hooks, policy, credentials and keys are left in place. A stale
skill or env ljos wrote is a warning. A row doctor can repair ends with
``run `ljos doctor --fix` ``. If `ljos` is not installed, follow
the ljos-setup skill.
