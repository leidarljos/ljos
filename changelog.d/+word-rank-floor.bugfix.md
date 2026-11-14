A word search ranks by BM25 over split identifiers, with a title or a tag counted above the body, a bonus when the query words sit together, and a small boost for this seat.
A query whose rows do not cover enough of its words prints `No close matches for "..."`, and names a broader query.
`ljos search --json` adds `close_matches` when that happens, and keeps `ranking` and `reading`.
