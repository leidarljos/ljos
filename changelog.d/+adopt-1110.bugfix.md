A main conversation listed in `seats.toml` is that seat when `LJOS_SEAT` is unset.
An unresolved seat names the conversation id, and `seat-unresolved` and `seat-unlisted` exit 2.
Failed commands stay stored as attempts and are left out of `ljos search` unless `--attempts` is set.
A missing deed tells you to cite the `id=` line `deedar create` prints.
`ljos sitting` and `ljos recall` read the pack listing once per command.
A busy `ljos remember` tries the pack three times inside twenty seconds, queues the lesson, and exits 75.
Default `ljos search` keeps this seat's lessons and preferences that share a topic word, and a refused packsetd connection is one line and exit 75.
