`ljos search` drops a row that only shares a stopword or a common weak word. It also drops a row under 0.34 that does not name a subject word.
`--min-score` sets that line, and `--min-score 0` prints every row.
When the floor removes every row the line is `No relevant matches for "QUERY".` and it names `--min-score 0`.
