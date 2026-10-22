`ljos-mcp` holds the end of stdin until the longest configured call
can finish. The wait used to be a fixed 35 s, so a pack call allowed
more than that by `PACKSET_TIMEOUT_MS` was cut off and its answer
dropped. The drain is now that timeout, a listing, the guard and a
durable row lock, whichever is longest, plus a few seconds. The
runtime's short shutdown cap stays in place for a dead stdin read, and
grows to the same drain while a write is still in progress.
