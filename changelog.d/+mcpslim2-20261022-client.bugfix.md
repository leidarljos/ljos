`ljos-mcp` stops when the client that started it is gone, even if a
shell, `npx`, or `sudo -n` is still sitting in front of it. The nearest
ancestor that is not one of those launchers is recorded by pid and
start time, so a reused pid is not taken for it. Stdin hanging up ends
the server the same way. On Linux a failed `PR_SET_PDEATHSIG` is logged
once; the signal itself is not the decision.
