`ljos doctor` fails the runner hook when the hooks file is newer than the exec-daemon that read it at start.
The mcp row on that machine reads the daemon's `--mcp-config` instead of only `~/.cursor/mcp.json`.
A file written after the daemon started is not the config that process is running.
