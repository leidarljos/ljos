A tracker board of 1 MiB or more is split into one file per issue under `issues/` on its next tracker write, in one commit, as `vissue migrate-ledger` does.
Each later claim, note or finish appends to that issue's file and commits it alone, so history grows by about the size of the event instead of the size of the board.
`LJOS_TRACKER_SPLIT_BYTES` sets the size, and `0` never splits.
A rebase that meets the same per-issue file on both sides keeps both sides' records.
