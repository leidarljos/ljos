A search while the encoder is still loading waits a few seconds for semantic ranking, and the fallback line says to rerun and about how long that takes.
`LJOS_SEARCH_WAIT` sets that wait, in seconds. The default is 3. `0` does not wait.
Starting the memory writer starts the encoder load, instead of leaving it until the first search.
`ljos doctor` says whether the encoder is loaded, loading, or failed, and a failed row names the reason.
`ljos search --json` keeps the ranking mode in `ranking`.
