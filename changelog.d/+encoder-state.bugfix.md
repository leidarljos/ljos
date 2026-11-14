`ljos doctor` prints one encoder row when the encoder is loading, has failed, or is not installed, and leaves a ready encoder off that list.
A search that falls back while the encoder is loading names the remaining time when packsetd sends `embedder.remaining_ms`, and the command to rerun, in one sentence.
A failed or missing encoder names the command that fixes it.
The wait stays `LJOS_SEARCH_WAIT`.
