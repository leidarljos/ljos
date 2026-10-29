The guard can answer a call with another call to run in its place, such as `uv run pytest` for a bare `pytest`.
That swap now shows one line saying which call ran and which was asked for.
Each swap is kept in `$XDG_DATA_HOME/ljos/rewrite.jsonl`, a row per call, so a session can be read back afterwards.
`rewrite = false` in a repository's `ljos.toml` runs its lines as they were written.
A call the switch declined is kept too, so the log shows what would have been swapped.
