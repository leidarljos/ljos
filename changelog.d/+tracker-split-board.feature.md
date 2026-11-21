A tracker board of 1 MiB or more is split into one file per issue under `issues/` on its next tracker write, in one commit with the board and every issue file, as `vissue migrate-ledger` does.
A split whose commit fails is rolled back and the board stays one file. A split left on disk without its commit is committed whole before the next write.
Files a killed split left are removed before the next split, and a write that waits out another writer's split still records its seat line.
Each later claim, note or finish appends to that issue's file and commits it alone, so history grows by about the size of the event instead of the size of the board.
`LJOS_TRACKER_SPLIT_BYTES` sets the size, and `0` never splits.
A rebase that meets the same per-issue file on both sides keeps both sides' records.
