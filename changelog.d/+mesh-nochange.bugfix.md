A repeated `ljos memory import` and `ljos memory integrate --all` still do no packset calls and take no writer lock, including when a lesson is queued and packsetd is busy.
`ljos memory export` leaves the file in place when its lessons are unchanged, prints `seed unchanged: N memories`, and reports `memories`, `unchanged` and `path` with `--json`.
A busy packsetd is not waited out on that path.
`ljos mesh pull` of unchanged lane files does the same: no packset call and no writer lock.
