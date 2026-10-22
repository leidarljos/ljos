The seat book and the claim index are replaced whole: the new bytes
are written to a temp file in the same directory, synced, and renamed
over the old name. A cut during that write leaves the previous file
or the new one. A note queue, a ballot and a sync export already
landed by rename.
