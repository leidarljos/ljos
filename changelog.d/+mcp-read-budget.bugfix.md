An MCP pack read stays about one second.
A seat that raises `LJOS_MCP_TIMEOUT` above thirty seconds and sets `PACKSET_TIMEOUT_MS` may use the shorter of those two for that read.
