Every call ljos makes to packsetd sends the bearer token from the pack home, so memory, mail and recall work against packsetd 0.15, which answered each of them with a 401.
The token is `PACKSET_TOKEN`, else the `token` file named by `PACKSET_TOKEN_FILE`, else `token` under `PACKSET_HOME` or the default pack home, as packset 0.15 resolves it.
`ljos doctor` prints a `pack token` row that fails, naming the file it looked for, when the writer refuses the seat.
