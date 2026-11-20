`ljos sync --compact` removed every snapshot and segment of this machine's atom log even when one did not open on this machine, so atoms that only those pieces carried were lost.
It now exits non-zero, names `ljos sync --key`, and removes nothing when the newest snapshot or a segment after it does not open in full.
A damaged older piece no longer blocks it: once a sync has sealed the atoms whole into a newer snapshot that opens, `ljos sync --compact` folds from that snapshot and removes the damaged piece with the rest.
