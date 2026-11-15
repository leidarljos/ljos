A search waits while the pack is starting, on `LJOS_SEARCH_WARMUP_WAIT` (5 seconds, `0` does not wait), and not past the call deadline.
An empty page is printed only after the pack has answered, in the same words as any other miss.
When the budget runs out the line is `Memory is starting, so try again in a few seconds.` and the command exits non-zero.
The MCP search returns that sentence as an error.
`ljos remember` and a finish lesson wait the same way.
A pack that is already open and busy still queues the lesson, and finish still closes.
