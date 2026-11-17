ljos reads and prints its dates and times with the `jiff` crate in place of `chrono`.
Ticket times, sitting times, lesson days and JSON timestamps print and parse the same text as before, and a refused `--deadline`, `--at` or `--valid-at` gives the old error text.
The local zone still follows `TZ` and the system zone file, and a clock reading that happens twice still takes the later instant.
A date in a year past 9999 or before 10000 BC is now refused where it used to read, and `ljos memories prune --stale --days` no longer panics on a count that reaches past that range.
`chrono` is still in the lockfile through `rmcp`, `schemars` and `vissue-core`, so the advisory ignore in `deny.toml` stays.
