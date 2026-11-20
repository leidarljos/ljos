A sync staged this machine's whole atom directory, so the temp file of a write that was cut off was committed, even by a sync with no new atoms.
A sync now stages only the snapshots, segments and plaintext hash.
