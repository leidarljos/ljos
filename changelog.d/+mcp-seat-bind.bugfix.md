An MCP write takes the `seat` argument on that call, or the stamped seat of that session.
A shared server does not reuse a seat from another call, from `LJOS_SEAT`, or from a seats file.
A write with neither is refused in one line that names `seat`, and a read still runs.
`ljos_finish` says in one line whether the ticket was closed, and why not.
