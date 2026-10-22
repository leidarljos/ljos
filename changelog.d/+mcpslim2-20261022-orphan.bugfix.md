An `ljos-mcp` whose client has gone, and whose stdin a launcher still
holds open, exits once it has been quiet and no editor or agent is
still above it. `LJOS_MCP_ORPHAN_IDLE`, or `[mcp] orphan_idle` in
`ljos.toml`, sets that quiet window. A bare number is seconds. The
default is six hours. `0` or `off` leaves the server until the client
or stdin ends it.
