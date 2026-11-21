`ljos file --top`, `ljos sitting --plan`, `ljos_start` and `ljos_file` no longer file a new ticket into a project named after the tracker's prefix, such as `Software/Software`, when nothing named a project.
A ticket with no `-p`, no `--parent` and no held issue now goes in the checkout's project, `LJOS_PROJECT`, or the `project` key of the seat's `[seats.NAME]` row in `vissue.toml`, and with none of those the command exits 1 and names `-p PROJECT`.
A plan with no `-p` filed under a held issue goes in that issue's project, and a child still goes in its parent's project.
`ljos sitting --plan` refuses a held id that matches more than one ticket, as `ljos file` does, and writes nothing.
`ljos file` and `ljos sitting --plan` without `--top` do not file under a held id the tracker does not have, even on a tracker with no `issues.org`; they keep that hold until a ticket is filed, and a project refusal, in the CLI or from `ljos_file`, names it.
