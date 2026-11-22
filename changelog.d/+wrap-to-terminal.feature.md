`ljos` now wraps human prose to the terminal width.
`ljos` wraps prose that it prints through its output module when the stream is a terminal.
This covers messages, listings, pages, help and the panic message.
On a terminal, a `COLUMNS` value that is a whole number from 1 up sets the width.
`ljos` raises a width below 20 to 20 and lowers a width above 240 to 240.
When `COLUMNS` is unset, 0 or not a whole number, `ljos` reads the terminal's own width with the same limits.
`ljos` does not wrap on a terminal that reports a width of 0.
`ljos` never wraps machine output, on stdout or on stderr.
Before it dispatches a command or writes, `ljos` decides the output kind once from the command-line arguments.
Machine output is any run with `--json`, `--porcelain` or `--out -`, a hook, the hook server, the tool server, and JSON lines and tab-separated rows.
This includes hook queue admission and its fallback line.
`ljos-mcp` never wraps anything it writes.
Output to a pipe or a file is unchanged.
A program that `ljos` runs with the terminal writes to it directly, and `ljos` does not wrap that output.
The passphrase prompt on `/dev/tty` is not wrapped.
