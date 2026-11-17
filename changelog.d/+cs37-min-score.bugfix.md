`--min-score` applies to every row of `ljos search`, including a row that shares a distinctive query word and a ticket.
A row whose score is under the value is left out, and when that leaves nothing the line is `No relevant matches for "QUERY".` and it names `--min-score 0`.
Without the flag, a row that names a subject word stays at any score as before.
`--min-score` takes a number from 0 to 100, and a value outside that range such as `--min-score 101` exits 2.
