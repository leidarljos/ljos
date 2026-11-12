`ljos mesh pull` leaves a lane row already in the pack unchanged, by the text's SHA-256 or by the lane and the row id, and counts it as unchanged.
The same pull integrates `imported,unintegrated` rows into the seat, deduplicated against the seat's existing memories, and relabels them `imported,integrated` with `lane:` and `seat:` tags so search ranks them.
A later import of the same lane file is unchanged as well.
