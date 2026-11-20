`ljos sync --compact` removed every snapshot and segment of this machine's atom log even when one did not open on this machine, so atoms that only those pieces carried were lost.
It now exits non-zero, names `ljos sync --key`, and removes nothing when any piece does not open in full.
