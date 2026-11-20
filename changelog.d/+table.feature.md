`ljos table ISSUE --add FILE.csv [--name NAME]` checks a CSV (a header row, the same column count on every row, UTF-8, at most 8 MiB), stores it in `deedar` and cites it on the ticket in one tracker commit, so a scoreboard is one deed rather than one ticket per row.
Adding the same name again makes the next version, which supersedes the one before, and `ljos current` on an older accession names the newer one.
`ljos table ISSUE` lists the tables with rows, columns, version and accession, and `--show NAME [--version N] [--csv|--json]` prints one after the same check `ljos deed --show` makes.
The `ljos_table` MCP tool does the same.
