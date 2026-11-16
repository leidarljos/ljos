`ljos search` drops a row that only shares a stopword or a common weak word. It also drops a row under 34 that does not name a subject word.
A word match and a meaning match both print a score from 0 to 100.
A meaning row from another seat that only shares a generic word is left out unless the encoder's similarity is high.
`--min-score` sets that line, and `--min-score 0` prints every row.
When the floor removes every row the line is `No relevant matches for "QUERY".` and it names `--min-score 0`.
