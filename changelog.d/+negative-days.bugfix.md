A negative number on `--days`, `--limit`, `--rounds`, `--quorum`, `--timeout` or `--confidence` now exits 2 and names the flag and the value.
So does a span on `--idle`, `--older-than` or `--every` that is negative or not a span.
`ljos memories prune --stale --days -1` used to print "0 stale" and exit 0, and `claims` and `release` exited 1 with different words.
A very large number of days no longer stops the command with a panic.
