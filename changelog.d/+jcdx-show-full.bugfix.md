`ljos show` wraps the full ticket title at eighty columns without cutting it, and prints the claim seat and age when claimed, the parent ticket, child tickets, blockers, the full body, and the notes in order with each note's time, age, and author seat.
Long note lists print the eight newest notes and name `--all` for the rest.
`--json` carries `claim`, `parent`, `children`, `blocked_by`, `body`, and `notes` with each note's `time`.
The `ljos_show` tool on `ljos mcp` returns the same ticket page.
