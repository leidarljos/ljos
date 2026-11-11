A command with no seat no longer stops.
The seat is `--seat`, then `LJOS_SEAT`, then a `seat=` line in `~/.config/ljos/env`, then the seats map for the agent or conversation identifier, then the parent process seat record, then the single seat that wrote most recently from this machine.
Search, recall, status and doctor read every seat when none of those match.
Remember, note and file write as `unknown-` plus the host name, label the memory `seat:unresolved`, and print one line that names the seat and how to set one with `--seat` or `LJOS_SEAT`.
Doctor shows which rule picked the seat.
