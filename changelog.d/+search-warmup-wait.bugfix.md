A search waits for the encoder when the memory server reports a remaining load that fits in `LJOS_SEARCH_WARMUP_WAIT` (5 seconds, `0` does not wait), and not past `LJOS_CALL_TIMEOUT`, then ranks by meaning.
A remaining time that is missing or longer than that budget still falls back to word matches.
A tracker ticket whose title matches is still included on that word page and on the meaning page after the wait.
`ljos search --json` adds `waited_ms` when that wait ran.
