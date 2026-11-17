`ljos mcp` and `ljos-mcp` reject unexpected arguments such as `setup` with exit code 2 and point to `ljos onboard` instead of blocking on standard input.
When standard input is a terminal, `ljos mcp` prints one hint line on standard error explaining how to register with a runner.
