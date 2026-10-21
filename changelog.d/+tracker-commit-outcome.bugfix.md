A tracker commit that finds HEAD already holding the file, because another seat's commit carried it, now reads `tracker git: nothing to commit` instead of `commit refused: On branch master`.
A refused commit always says why: git's first line on stderr, else on stdout, else that git was killed by a signal or exited without a word, where the line used to end at `commit refused: `.
The seat-key pin and sync's log commit go through the same staging, with the same backoff on a busy index.
